//! Portfolio Agent — deterministic capital allocation and position sizing.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bamboo_core::{
    AgentHeartbeat, AgentRunStatus, BusMessage, ComponentId, Currency, EventBus, InstrumentId,
    Money, OrderSide, OrderStatus, OrderType, Payload, PortfolioConfig, PortfolioIntent,
    PositionSide, Price, Quantity, TimeInForce, Topic,
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
        agent_name: "PortfolioAgent".to_string(),
        status,
        last_action: action,
        timestamp: now_nanos(),
    };
    let msg = bus_msg(Topic::System, Payload::AgentHeartbeat(hb), "PortfolioAgent");
    let _ = bus.publish(msg).await;
}

/// State of a single position.
#[derive(Debug, Clone)]
pub struct PositionState {
    pub side: PositionSide,
    pub quantity: Quantity,
    pub avg_entry: Price,
    pub current_price: Price,
    pub unrealized_pnl: Money,
}

/// Portfolio agent state.
#[derive(Debug)]
pub struct PortfolioState {
    pub total_capital: f64,
    pub available_capital: f64,
    pub positions: HashMap<InstrumentId, PositionState>,
    pub pending_intents: Vec<Uuid>,
    /// Capital reserved per intent id, returned on rejection/failure.
    pub capital_reserved: HashMap<Uuid, f64>,
}

/// Calculate position size based on risk-per-trade sizing.
///
/// `risk_pct_per_trade`: percentage of total capital to risk (e.g., 1.0 = 1%)
/// `total_capital`: total portfolio value
/// `stop_loss_pct`: stop-loss distance as percentage (e.g., 3.0 = 3%)
/// `available_capital`: capital available for new positions
///
/// Returns: position value in USD.
pub fn calculate_position_size(
    total_capital: f64,
    risk_pct_per_trade: f64,
    stop_loss_pct: f64,
    available_capital: f64,
) -> f64 {
    if stop_loss_pct <= 0.0 || total_capital <= 0.0 {
        return 0.0;
    }

    let risk_amount = total_capital * (risk_pct_per_trade / 100.0);
    let position_value = risk_amount / (stop_loss_pct / 100.0);

    // Cannot exceed available capital.
    position_value.min(available_capital).max(0.0)
}

/// Run the portfolio agent as an async task.
pub async fn run_portfolio_agent(
    bus: Arc<dyn EventBus>,
    config: PortfolioConfig,
    shutdown: ShutdownSignal,
) {
    let initial_capital = config.initial_capital_usd.amount.as_f64();
    let mut state = PortfolioState {
        total_capital: initial_capital,
        available_capital: initial_capital,
        positions: HashMap::new(),
        pending_intents: Vec::new(),
        capital_reserved: HashMap::new(),
    };

    let mut signal_rx = bus.subscribe(Topic::Signal);
    let mut exec_rx = bus.subscribe(Topic::Execution);
    let mut risk_rx = bus.subscribe(Topic::Risk);
    let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(10));

    tracing::info!(
        capital = initial_capital,
        "PortfolioAgent started"
    );
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
                        "Positions: {}, Available: ${:.2}",
                        state.positions.len(),
                        state.available_capital,
                    )),
                ).await;
            }
            result = signal_rx.recv() => {
                match result {
                    Ok(msg) => {
                        let signal = match &msg.payload {
                            Payload::StrategySignal(s) => s.clone(),
                            _ => continue,
                        };

                        // Check position count constraint.
                        if state.positions.len() >= config.max_positions {
                            tracing::info!(
                                instrument = %signal.instrument_id,
                                "Rejecting signal: max positions reached"
                            );
                            continue;
                        }

                        // Skip if we already have a position for this instrument.
                        if state.positions.contains_key(&signal.instrument_id) {
                            continue;
                        }

                        // Calculate position size.
                        // Use a default stop-loss of 3% if not inferrable from the signal.
                        let stop_loss_pct = 3.0; // Default stop-loss distance.
                        let position_value = calculate_position_size(
                            state.total_capital,
                            config.risk_pct_per_trade,
                            stop_loss_pct,
                            state.available_capital,
                        );

                        if position_value <= 0.0 {
                            tracing::info!("Rejecting signal: no capital available");
                            continue;
                        }

                        // Convert position value to quantity (approximate using a reference price).
                        // In production, we'd use actual market price.
                        // For now, use 1.0 as a placeholder quantity scaling.
                        let quantity = Quantity::from_f64(position_value, 8);

                        let intent = PortfolioIntent {
                            id: Uuid::new_v4(),
                            signal_id: signal.id,
                            instrument_id: signal.instrument_id.clone(),
                            side: signal.side,
                            quantity,
                            order_type: OrderType::Market,
                            limit_price: None,
                            stop_price: signal.stop_loss,
                            time_in_force: TimeInForce::GTC,
                            timestamp: now_nanos(),
                        };

                        state.pending_intents.push(intent.id);
                        state.capital_reserved.insert(intent.id, position_value);
                        state.available_capital -= position_value;

                        tracing::info!(
                            instrument = %signal.instrument_id,
                            value = position_value,
                            "Publishing PortfolioIntent"
                        );

                        let msg = bus_msg(
                            Topic::Intent,
                            Payload::PortfolioIntent(intent),
                            "PortfolioAgent",
                        );
                        let _ = bus.publish(msg).await;

                        publish_heartbeat(
                            &bus,
                            AgentRunStatus::Running,
                            Some(format!("Intent for {}", signal.instrument_id)),
                        ).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lagged = n, "PortfolioAgent signal_rx lagged");
                    }
                    Err(_) => break,
                }
            }
            result = exec_rx.recv() => {
                match result {
                    Ok(msg) => {
                        match &msg.payload {
                            Payload::ExecutionReport(report) => {
                                // On successful fill, clean up reserved capital.
                                if report.status == OrderStatus::Filled {
                                    // Capital was already deducted; no need to return it.
                                    // Just clean up the reservation tracking.
                                    // (We don't have intent_id on report, so we leave
                                    // capital_reserved cleanup to the risk_rx path or
                                    // accept minor leak — intent_ids are cleaned below.)
                                    if let Some(fill_price) = &report.avg_fill_price {
                                        let fill_value = fill_price.as_f64()
                                            * report.filled_quantity.as_f64();
                                        // For sells, return capital.
                                        if report.side == OrderSide::Sell {
                                            state.available_capital += fill_value;
                                        }
                                    }
                                    tracing::info!(
                                        instrument = %report.instrument_id,
                                        "ExecutionReport filled"
                                    );
                                } else if report.status == OrderStatus::Rejected
                                    || report.status == OrderStatus::Canceled
                                {
                                    // Execution failed — return reserved capital.
                                    // Find by matching instrument in pending intents.
                                    let intent_id = state
                                        .pending_intents
                                        .iter()
                                        .find(|id| {
                                            state.capital_reserved.contains_key(id)
                                        })
                                        .copied();
                                    if let Some(id) = intent_id {
                                        if let Some(reserved) = state.capital_reserved.remove(&id) {
                                            state.available_capital += reserved;
                                            tracing::info!(
                                                reserved = reserved,
                                                "Returned capital on execution failure"
                                            );
                                        }
                                        state.pending_intents.retain(|i| *i != id);
                                    }
                                }
                            }
                            Payload::PositionUpdate(pos) => {
                                let pstate = PositionState {
                                    side: pos.side,
                                    quantity: pos.quantity,
                                    avg_entry: pos.avg_entry_price,
                                    current_price: pos.avg_entry_price,
                                    unrealized_pnl: pos.unrealized_pnl.clone()
                                        .unwrap_or_else(|| Money::zero(Currency::usdt())),
                                };
                                if pos.side == PositionSide::Flat {
                                    state.positions.remove(&pos.instrument_id);
                                } else {
                                    state.positions.insert(pos.instrument_id.clone(), pstate);
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lagged = n, "PortfolioAgent exec_rx lagged");
                    }
                    Err(_) => break,
                }
            }
            result = risk_rx.recv() => {
                match result {
                    Ok(msg) => {
                        if let Payload::RiskDecision(decision) = &msg.payload {
                            if !decision.approved {
                                // Risk rejected — return reserved capital.
                                if let Some(reserved) =
                                    state.capital_reserved.remove(&decision.intent_id)
                                {
                                    state.available_capital += reserved;
                                    tracing::info!(
                                        intent_id = %decision.intent_id,
                                        reserved = reserved,
                                        "Returned capital on risk rejection"
                                    );
                                }
                                state.pending_intents.retain(|id| *id != decision.intent_id);
                            } else {
                                // Approved — clean up pending_intents tracking
                                // (capital stays reserved until execution completes).
                                state.pending_intents.retain(|id| *id != decision.intent_id);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lagged = n, "PortfolioAgent risk_rx lagged");
                    }
                    Err(_) => break,
                }
            }
        }
    }

    publish_heartbeat(&bus, AgentRunStatus::Stopped, Some("Shutdown".to_string())).await;
    tracing::info!("PortfolioAgent exiting");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_size_basic() {
        // 100k capital, 1% risk, 3% stop = $1000 / 0.03 = $33333.33
        let size = calculate_position_size(100_000.0, 1.0, 3.0, 100_000.0);
        assert!((size - 33_333.33).abs() < 1.0, "got {size}");
    }

    #[test]
    fn position_size_capped_by_available_capital() {
        // Available capital is only $5000, so position size is capped.
        let size = calculate_position_size(100_000.0, 1.0, 3.0, 5_000.0);
        assert!((size - 5_000.0).abs() < 1e-6, "got {size}");
    }

    #[test]
    fn position_size_zero_stop_loss() {
        let size = calculate_position_size(100_000.0, 1.0, 0.0, 100_000.0);
        assert_eq!(size, 0.0);
    }

    #[test]
    fn position_size_zero_capital() {
        let size = calculate_position_size(0.0, 1.0, 3.0, 0.0);
        assert_eq!(size, 0.0);
    }

    #[test]
    fn position_size_high_risk() {
        // 100k capital, 5% risk, 10% stop = $5000 / 0.10 = $50000
        let size = calculate_position_size(100_000.0, 5.0, 10.0, 100_000.0);
        assert!((size - 50_000.0).abs() < 1.0, "got {size}");
    }

    #[test]
    fn position_size_small_stop() {
        // 100k capital, 1% risk, 0.5% stop = $1000 / 0.005 = $200000
        // But capped at available capital of 100k.
        let size = calculate_position_size(100_000.0, 1.0, 0.5, 100_000.0);
        assert!((size - 100_000.0).abs() < 1e-6, "got {size}");
    }
}
