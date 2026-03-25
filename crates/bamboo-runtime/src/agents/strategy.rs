//! Strategy Agent — converts research findings into actionable trading signals.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bamboo_core::{
    AgentHeartbeat, AgentRunStatus, BusMessage, ComponentId, EventBus, InstrumentId, OrderSide,
    Payload, ResearchFinding, StrategyConfig, StrategyId, StrategySignal, Topic,
};
use uuid::Uuid;

use crate::shutdown::ShutdownSignal;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn now_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

fn bus_msg(topic: Topic, payload: Payload, source: &str) -> BusMessage {
    BusMessage {
        id: Uuid::new_v4(),
        topic,
        payload,
        timestamp: now_nanos(),
        source: ComponentId::new(source),
    }
}

async fn publish_heartbeat(bus: &Arc<dyn EventBus>, status: AgentRunStatus, action: Option<String>) {
    let hb = AgentHeartbeat {
        agent_name: "StrategyAgent".to_string(),
        status,
        last_action: action,
        timestamp: now_nanos(),
    };
    let msg = bus_msg(Topic::System, Payload::AgentHeartbeat(hb), "StrategyAgent");
    let _ = bus.publish(msg).await;
}

/// Evaluate the momentum strategy against a research finding.
/// Returns a StrategySignal if the criteria are met.
pub fn evaluate_momentum(
    finding: &ResearchFinding,
    config: &StrategyConfig,
) -> Option<StrategySignal> {
    let min_change = config.momentum.min_change_pct;

    // Parse change_pct from thesis (format: "24h change X.XX%, ...").
    let change_pct = parse_change_pct(&finding.thesis)?;

    if change_pct.abs() < min_change {
        return None;
    }

    let side = if change_pct > 0.0 {
        OrderSide::Buy
    } else {
        OrderSide::Sell
    };

    Some(StrategySignal {
        id: Uuid::new_v4(),
        strategy_id: StrategyId::new("momentum"),
        instrument_id: finding.instrument_id.clone(),
        side,
        entry_price: None,
        exit_price: None,
        stop_loss: None,
        rationale: format!(
            "Momentum: {:.2}% change exceeds {:.2}% threshold",
            change_pct, min_change,
        ),
        confidence: (change_pct.abs() / 10.0).min(1.0),
        horizon_hours: config.momentum.hold_hours,
        timestamp: now_nanos(),
    })
}

/// Evaluate the mean reversion strategy against a research finding.
/// Returns a StrategySignal if the criteria are met.
pub fn evaluate_mean_reversion(
    finding: &ResearchFinding,
    config: &StrategyConfig,
) -> Option<StrategySignal> {
    let min_drop = config.mean_reversion.min_drop_pct;

    let change_pct = parse_change_pct(&finding.thesis)?;

    // Mean reversion: only trigger on drops (negative change) that exceed the threshold.
    if change_pct >= 0.0 || change_pct.abs() < min_drop {
        return None;
    }

    // Estimate stop loss price (relative to a hypothetical current price from the finding).
    let stop_loss_pct = config.mean_reversion.stop_loss_pct;

    Some(StrategySignal {
        id: Uuid::new_v4(),
        strategy_id: StrategyId::new("mean_reversion"),
        instrument_id: finding.instrument_id.clone(),
        side: OrderSide::Buy, // Mean reversion is long-only.
        entry_price: None,
        exit_price: None,
        stop_loss: None,
        rationale: format!(
            "MeanReversion: {:.2}% drop exceeds {:.2}% threshold, stop={:.2}%",
            change_pct.abs(),
            min_drop,
            stop_loss_pct,
        ),
        confidence: (change_pct.abs() / 15.0).min(1.0),
        horizon_hours: config.mean_reversion.hold_hours,
        timestamp: now_nanos(),
    })
}

/// Parse change percentage from a thesis string.
/// Expected format: "24h change X.XX%, ..."
pub fn parse_change_pct(thesis: &str) -> Option<f64> {
    // Look for "change " followed by a number and "%".
    let idx = thesis.find("change ")?;
    let rest = &thesis[idx + 7..];
    let end = rest.find('%')?;
    rest[..end].trim().parse::<f64>().ok()
}

/// Run the strategy agent as an async task.
pub async fn run_strategy_agent(
    bus: Arc<dyn EventBus>,
    config: StrategyConfig,
    shutdown: ShutdownSignal,
) {
    let mut rx = bus.subscribe(Topic::Signal);
    let mut active_signals: HashMap<InstrumentId, StrategySignal> = HashMap::new();
    let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(10));

    tracing::info!("StrategyAgent started");
    publish_heartbeat(&bus, AgentRunStatus::Running, Some("Started".to_string())).await;

    loop {
        if shutdown.is_shutdown() {
            break;
        }

        tokio::select! {
            _ = shutdown.wait_for_shutdown() => { break; }
            _ = heartbeat_interval.tick() => {
                publish_heartbeat(
                    &bus,
                    AgentRunStatus::Running,
                    Some(format!("Active signals: {}", active_signals.len())),
                ).await;
            }
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        let finding = match &msg.payload {
                            Payload::ResearchFinding(f) => f.clone(),
                            _ => continue,
                        };

                        // Skip if we already have an active signal for this instrument.
                        if active_signals.contains_key(&finding.instrument_id) {
                            continue;
                        }

                        // Check max concurrent signals.
                        if active_signals.len() >= config.max_concurrent_signals {
                            continue;
                        }

                        // Evaluate enabled strategies.
                        let mut signal: Option<StrategySignal> = None;

                        if config.enabled_strategies.iter().any(|s| s == "momentum") {
                            signal = evaluate_momentum(&finding, &config);
                        }

                        if signal.is_none()
                            && config.enabled_strategies.iter().any(|s| s == "mean_reversion")
                        {
                            signal = evaluate_mean_reversion(&finding, &config);
                        }

                        if let Some(sig) = signal {
                            active_signals.insert(sig.instrument_id.clone(), sig.clone());
                            let msg = bus_msg(
                                Topic::Signal,
                                Payload::StrategySignal(sig),
                                "StrategyAgent",
                            );
                            let _ = bus.publish(msg).await;

                            publish_heartbeat(
                                &bus,
                                AgentRunStatus::Running,
                                Some(format!(
                                    "Signal for {}",
                                    finding.instrument_id
                                )),
                            )
                            .await;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lagged = n, "StrategyAgent lagged");
                    }
                    Err(_) => break,
                }
            }
        }
    }

    publish_heartbeat(&bus, AgentRunStatus::Stopped, Some("Shutdown".to_string())).await;
    tracing::info!("StrategyAgent exiting");
}

#[cfg(test)]
mod tests {
    use super::*;
    use bamboo_core::{MeanReversionParams, MomentumParams};

    fn test_config() -> StrategyConfig {
        StrategyConfig {
            enabled_strategies: vec!["momentum".to_string(), "mean_reversion".to_string()],
            max_concurrent_signals: 5,
            momentum: MomentumParams {
                min_change_pct: 2.0,
                hold_hours: 24,
                stop_loss_pct: 3.0,
            },
            mean_reversion: MeanReversionParams {
                min_drop_pct: 5.0,
                target_recovery_pct: 3.0,
                hold_hours: 48,
                stop_loss_pct: 5.0,
            },
        }
    }

    fn finding_with_change(change_pct: f64) -> ResearchFinding {
        ResearchFinding {
            id: Uuid::new_v4(),
            instrument_id: InstrumentId::from_parts("BTCUSDT", "BINANCE"),
            thesis: format!(
                "24h change {:.2}%, vol $1000000, volatility 5.00%",
                change_pct
            ),
            score: 0.8,
            recommended_action: Some(OrderSide::Buy),
            timestamp: 0,
        }
    }

    #[test]
    fn parse_change_pct_positive() {
        let thesis = "24h change 5.25%, vol $1000000, volatility 3.00%";
        assert!((parse_change_pct(thesis).unwrap() - 5.25).abs() < 1e-6);
    }

    #[test]
    fn parse_change_pct_negative() {
        let thesis = "24h change -7.50%, vol $500000, volatility 8.00%";
        assert!((parse_change_pct(thesis).unwrap() - (-7.50)).abs() < 1e-6);
    }

    #[test]
    fn momentum_triggers_on_large_positive_change() {
        let config = test_config();
        let finding = finding_with_change(5.0);
        let signal = evaluate_momentum(&finding, &config);
        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert_eq!(sig.side, OrderSide::Buy);
        assert_eq!(sig.strategy_id.as_str(), "momentum");
    }

    #[test]
    fn momentum_triggers_on_large_negative_change() {
        let config = test_config();
        let finding = finding_with_change(-3.5);
        let signal = evaluate_momentum(&finding, &config);
        assert!(signal.is_some());
        assert_eq!(signal.unwrap().side, OrderSide::Sell);
    }

    #[test]
    fn momentum_skips_small_change() {
        let config = test_config();
        let finding = finding_with_change(1.0);
        assert!(evaluate_momentum(&finding, &config).is_none());
    }

    #[test]
    fn mean_reversion_triggers_on_large_drop() {
        let config = test_config();
        let finding = finding_with_change(-6.0);
        let signal = evaluate_mean_reversion(&finding, &config);
        assert!(signal.is_some());
        let sig = signal.unwrap();
        assert_eq!(sig.side, OrderSide::Buy); // long-only
        assert_eq!(sig.strategy_id.as_str(), "mean_reversion");
    }

    #[test]
    fn mean_reversion_skips_positive_change() {
        let config = test_config();
        let finding = finding_with_change(6.0);
        assert!(evaluate_mean_reversion(&finding, &config).is_none());
    }

    #[test]
    fn mean_reversion_skips_small_drop() {
        let config = test_config();
        let finding = finding_with_change(-2.0);
        assert!(evaluate_mean_reversion(&finding, &config).is_none());
    }
}
