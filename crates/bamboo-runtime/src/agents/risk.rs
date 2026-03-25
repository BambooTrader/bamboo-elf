//! Risk Agent — deterministic risk enforcement with hard controls.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bamboo_core::{
    AgentHeartbeat, AgentRunStatus, BusMessage, ComponentId, EventBus, InstrumentId, OrderSide,
    OrderStatus, Payload, PortfolioIntent, RiskDecision, RiskLimitsConfig, Topic,
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
        agent_name: "RiskAgent".to_string(),
        status,
        last_action: action,
        timestamp: now_nanos(),
    };
    let msg = bus_msg(Topic::System, Payload::AgentHeartbeat(hb), "RiskAgent");
    let _ = bus.publish(msg).await;
}

/// Risk agent internal state.
#[derive(Debug)]
pub struct RiskState {
    pub peak_equity: f64,
    pub current_equity: f64,
    pub orders_this_minute: u32,
    pub last_minute_reset: u64,
    pub kill_switch: bool,
    pub total_exposure: f64,
    pub exposures: HashMap<InstrumentId, f64>,
}

impl RiskState {
    pub fn new(initial_equity: f64) -> Self {
        Self {
            peak_equity: initial_equity,
            current_equity: initial_equity,
            orders_this_minute: 0,
            last_minute_reset: now_nanos(),
            kill_switch: false,
            total_exposure: 0.0,
            exposures: HashMap::new(),
        }
    }
}

/// Result of a risk check: either approved or rejected with a reason.
#[derive(Debug, Clone)]
pub struct RiskCheckResult {
    pub approved: bool,
    pub reason: String,
}

/// Run all 6 risk checks against an intent. Returns the first failure, or approval.
pub fn check_risk(
    intent: &PortfolioIntent,
    state: &mut RiskState,
    config: &RiskLimitsConfig,
) -> RiskCheckResult {
    // 1. Kill switch.
    if state.kill_switch && config.kill_switch_enabled {
        return RiskCheckResult {
            approved: false,
            reason: "Kill switch is active".to_string(),
        };
    }

    // 2. Order rate limit.
    let now = now_nanos();
    let one_minute_nanos: u64 = 60_000_000_000;
    if now - state.last_minute_reset > one_minute_nanos {
        state.orders_this_minute = 0;
        state.last_minute_reset = now;
    }
    if state.orders_this_minute >= config.order_rate_limit_per_min {
        return RiskCheckResult {
            approved: false,
            reason: format!(
                "Rate limit exceeded: {} orders/min",
                config.order_rate_limit_per_min
            ),
        };
    }

    // 3. Position size check.
    let intent_value = intent.quantity.as_f64();
    let max_pos = config.max_position_size_usd.amount.as_f64();
    if intent_value > max_pos {
        return RiskCheckResult {
            approved: false,
            reason: format!(
                "Position size ${:.2} exceeds max ${:.2}",
                intent_value, max_pos,
            ),
        };
    }

    // 4. Total exposure check.
    let max_exposure = config.max_portfolio_exposure_usd.amount.as_f64();
    if state.total_exposure + intent_value > max_exposure {
        return RiskCheckResult {
            approved: false,
            reason: format!(
                "Total exposure ${:.2} + ${:.2} exceeds max ${:.2}",
                state.total_exposure, intent_value, max_exposure,
            ),
        };
    }

    // 5. Concentration check.
    let existing = state
        .exposures
        .get(&intent.instrument_id)
        .copied()
        .unwrap_or(0.0);
    let new_exposure = existing + intent_value;
    let concentration_pct = if state.current_equity > 0.0 {
        new_exposure / state.current_equity * 100.0
    } else {
        100.0
    };
    if concentration_pct > config.max_concentration_pct {
        return RiskCheckResult {
            approved: false,
            reason: format!(
                "Concentration {:.1}% exceeds max {:.1}%",
                concentration_pct, config.max_concentration_pct,
            ),
        };
    }

    // 6. Drawdown check.
    if state.peak_equity > 0.0 {
        let drawdown_pct =
            (state.peak_equity - state.current_equity) / state.peak_equity * 100.0;
        if drawdown_pct > config.max_drawdown_pct {
            return RiskCheckResult {
                approved: false,
                reason: format!(
                    "Drawdown {:.1}% exceeds max {:.1}%",
                    drawdown_pct, config.max_drawdown_pct,
                ),
            };
        }
    }

    // All checks passed.
    state.orders_this_minute += 1;
    state.total_exposure += intent_value;
    let entry = state.exposures.entry(intent.instrument_id.clone()).or_insert(0.0);
    *entry += intent_value;

    RiskCheckResult {
        approved: true,
        reason: "All risk checks passed".to_string(),
    }
}

/// Run the risk agent as an async task.
pub async fn run_risk_agent(
    bus: Arc<dyn EventBus>,
    config: RiskLimitsConfig,
    initial_equity: f64,
    shutdown: ShutdownSignal,
) {
    let mut state = RiskState::new(initial_equity);
    let mut intent_rx = bus.subscribe(Topic::Intent);
    let mut exec_rx = bus.subscribe(Topic::Execution);
    let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(10));

    tracing::info!(equity = initial_equity, "RiskAgent started");
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
                    Some(format!(
                        "Exposure: ${:.2}, DD: {:.1}%",
                        state.total_exposure,
                        if state.peak_equity > 0.0 {
                            (state.peak_equity - state.current_equity) / state.peak_equity * 100.0
                        } else {
                            0.0
                        }
                    )),
                ).await;
            }
            result = intent_rx.recv() => {
                match result {
                    Ok(msg) => {
                        let intent = match &msg.payload {
                            Payload::PortfolioIntent(i) => i.clone(),
                            _ => continue,
                        };

                        let check_result = check_risk(&intent, &mut state, &config);

                        let decision = RiskDecision {
                            id: Uuid::new_v4(),
                            intent_id: intent.id,
                            approved: check_result.approved,
                            reason: check_result.reason.clone(),
                            adjusted_quantity: None,
                            constraints: if check_result.approved {
                                vec![]
                            } else {
                                vec![check_result.reason.clone()]
                            },
                            timestamp: now_nanos(),
                        };

                        tracing::info!(
                            intent_id = %intent.id,
                            approved = check_result.approved,
                            reason = %check_result.reason,
                            "RiskDecision"
                        );

                        let msg = bus_msg(
                            Topic::Risk,
                            Payload::RiskDecision(decision),
                            "RiskAgent",
                        );
                        let _ = bus.publish(msg).await;

                        publish_heartbeat(
                            &bus,
                            AgentRunStatus::Running,
                            Some(format!(
                                "Decision: {}",
                                if check_result.approved { "approved" } else { "rejected" }
                            )),
                        ).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lagged = n, "RiskAgent intent_rx lagged");
                    }
                    Err(_) => break,
                }
            }
            result = exec_rx.recv() => {
                match result {
                    Ok(msg) => {
                        match &msg.payload {
                            Payload::ExecutionReport(report) => {
                                // Decrement exposure on sell fills.
                                if report.side == OrderSide::Sell
                                    && report.status == OrderStatus::Filled
                                {
                                    let fill_value = report.filled_quantity.as_f64();
                                    state.total_exposure =
                                        (state.total_exposure - fill_value).max(0.0);
                                    if let Some(exp) =
                                        state.exposures.get_mut(&report.instrument_id)
                                    {
                                        *exp = (*exp - fill_value).max(0.0);
                                        if *exp <= 0.0 {
                                            state
                                                .exposures
                                                .remove(&report.instrument_id);
                                        }
                                    }
                                }
                            }
                            Payload::PositionUpdate(pos) => {
                                // Remove exposure when position is closed (quantity 0).
                                if pos.quantity.as_f64() <= 0.0 {
                                    if let Some(removed) =
                                        state.exposures.remove(&pos.instrument_id)
                                    {
                                        state.total_exposure =
                                            (state.total_exposure - removed).max(0.0);
                                    }
                                }
                                // Update equity tracking from unrealized PnL.
                                if let Some(pnl) = &pos.unrealized_pnl {
                                    let pnl_value = pnl.amount.as_f64();
                                    state.current_equity = state.peak_equity + pnl_value;
                                    if state.current_equity > state.peak_equity {
                                        state.peak_equity = state.current_equity;
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
        }
    }

    publish_heartbeat(&bus, AgentRunStatus::Stopped, Some("Shutdown".to_string())).await;
    tracing::info!("RiskAgent exiting");
}

#[cfg(test)]
mod tests {
    use super::*;
    use bamboo_core::{Currency, InstrumentId, Money, OrderSide, OrderType, Quantity, TimeInForce};

    fn test_config() -> RiskLimitsConfig {
        RiskLimitsConfig {
            max_position_size_usd: Money::from_f64(10_000.0, Currency::usd()),
            max_portfolio_exposure_usd: Money::from_f64(50_000.0, Currency::usd()),
            max_concentration_pct: 25.0,
            max_drawdown_pct: 10.0,
            order_rate_limit_per_min: 60,
            kill_switch_enabled: true,
        }
    }

    fn test_intent(instrument: &str, value: f64) -> PortfolioIntent {
        PortfolioIntent {
            id: Uuid::new_v4(),
            signal_id: Uuid::new_v4(),
            instrument_id: InstrumentId::from_parts(instrument, "BINANCE"),
            side: OrderSide::Buy,
            quantity: Quantity::from_f64(value, 8),
            order_type: OrderType::Market,
            limit_price: None,
            stop_price: None,
            time_in_force: TimeInForce::GTC,
            timestamp: now_nanos(),
        }
    }

    #[test]
    fn risk_approves_valid_intent() {
        let config = test_config();
        let mut state = RiskState::new(100_000.0);
        let intent = test_intent("BTCUSDT", 5_000.0);
        let result = check_risk(&intent, &mut state, &config);
        assert!(result.approved, "reason: {}", result.reason);
    }

    #[test]
    fn risk_rejects_kill_switch() {
        let config = test_config();
        let mut state = RiskState::new(100_000.0);
        state.kill_switch = true;
        let intent = test_intent("BTCUSDT", 1_000.0);
        let result = check_risk(&intent, &mut state, &config);
        assert!(!result.approved);
        assert!(result.reason.contains("Kill switch"));
    }

    #[test]
    fn risk_rejects_rate_limit() {
        let config = test_config();
        let mut state = RiskState::new(100_000.0);
        state.orders_this_minute = 60;
        let intent = test_intent("BTCUSDT", 1_000.0);
        let result = check_risk(&intent, &mut state, &config);
        assert!(!result.approved);
        assert!(result.reason.contains("Rate limit"));
    }

    #[test]
    fn risk_rejects_oversized_position() {
        let config = test_config();
        let mut state = RiskState::new(100_000.0);
        let intent = test_intent("BTCUSDT", 15_000.0); // Exceeds 10k max.
        let result = check_risk(&intent, &mut state, &config);
        assert!(!result.approved);
        assert!(result.reason.contains("Position size"));
    }

    #[test]
    fn risk_rejects_excess_exposure() {
        let config = test_config();
        let mut state = RiskState::new(100_000.0);
        state.total_exposure = 48_000.0;
        let intent = test_intent("BTCUSDT", 5_000.0); // 48k + 5k > 50k max.
        let result = check_risk(&intent, &mut state, &config);
        assert!(!result.approved);
        assert!(result.reason.contains("Total exposure"));
    }

    #[test]
    fn risk_rejects_concentration() {
        let config = test_config();
        let mut state = RiskState::new(100_000.0);
        // Pre-existing exposure of 20k for BTCUSDT.
        state.exposures.insert(
            InstrumentId::from_parts("BTCUSDT", "BINANCE"),
            20_000.0,
        );
        state.total_exposure = 20_000.0;
        // Adding 8k would make 28k / 100k = 28% > 25% max.
        let intent = test_intent("BTCUSDT", 8_000.0);
        let result = check_risk(&intent, &mut state, &config);
        assert!(!result.approved);
        assert!(result.reason.contains("Concentration"));
    }

    #[test]
    fn risk_rejects_drawdown() {
        let config = test_config();
        let mut state = RiskState::new(100_000.0);
        // Simulate 15% drawdown.
        state.current_equity = 85_000.0;
        let intent = test_intent("BTCUSDT", 1_000.0);
        let result = check_risk(&intent, &mut state, &config);
        assert!(!result.approved);
        assert!(result.reason.contains("Drawdown"));
    }

    #[test]
    fn risk_updates_state_on_approval() {
        let config = test_config();
        let mut state = RiskState::new(100_000.0);
        let intent = test_intent("BTCUSDT", 5_000.0);
        let result = check_risk(&intent, &mut state, &config);
        assert!(result.approved);
        assert_eq!(state.orders_this_minute, 1);
        assert!((state.total_exposure - 5_000.0).abs() < 1e-6);
        assert!(state.exposures.contains_key(&InstrumentId::from_parts("BTCUSDT", "BINANCE")));
    }

    #[test]
    fn risk_rate_limit_resets_after_minute() {
        let config = test_config();
        let mut state = RiskState::new(100_000.0);
        state.orders_this_minute = 60;
        // Set last reset to more than a minute ago.
        state.last_minute_reset = now_nanos() - 61_000_000_000;
        let intent = test_intent("BTCUSDT", 1_000.0);
        let result = check_risk(&intent, &mut state, &config);
        assert!(result.approved, "reason: {}", result.reason);
        assert_eq!(state.orders_this_minute, 1); // Reset and incremented.
    }
}
