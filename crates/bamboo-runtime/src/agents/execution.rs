//! Execution Agent — converts approved RiskDecisions into orders via a VenueAdapter.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bamboo_core::{
    AgentHeartbeat, AgentRunStatus, BusMessage, ClientOrderId, ComponentId, EventBus,
    ExecutionOrderIntent, ExecutionReport, InstrumentId, LiquiditySide, OrderSide, OrderStatus,
    OrderType, Payload, PortfolioIntent, PositionId, PositionSide, PositionUpdate, Price, Quantity,
    RiskDecision, Topic, Venue, VenueAdapter, VenueOrderId,
};
use bamboo_core::config::ExecutionConfig;
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
        agent_name: "ExecutionAgent".to_string(),
        status,
        last_action: action,
        timestamp: now_nanos(),
    };
    let msg = bus_msg(Topic::System, Payload::AgentHeartbeat(hb), "ExecutionAgent");
    let _ = bus.publish(msg).await;
}

// ── Order State ──────────────────────────────────────────────────────────────

/// Internal order state tracked by the execution agent.
#[derive(Debug, Clone)]
pub struct OrderState {
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub instrument_id: InstrumentId,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub quantity: Quantity,
    pub limit_price: Option<Price>,
    pub status: OrderStatus,
    pub filled_quantity: Quantity,
    pub avg_fill_price: Option<Price>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl OrderState {
    /// Transition the order to a new status. Returns true if the transition is valid.
    pub fn transition(&mut self, new_status: OrderStatus) -> bool {
        let valid = match (&self.status, &new_status) {
            (OrderStatus::Initialized, OrderStatus::Submitted) => true,
            (OrderStatus::Submitted, OrderStatus::Accepted) => true,
            (OrderStatus::Submitted, OrderStatus::Rejected) => true,
            (OrderStatus::Accepted, OrderStatus::Filled) => true,
            (OrderStatus::Accepted, OrderStatus::PartiallyFilled) => true,
            (OrderStatus::Accepted, OrderStatus::Canceled) => true,
            (OrderStatus::Accepted, OrderStatus::Expired) => true,
            (OrderStatus::PartiallyFilled, OrderStatus::Filled) => true,
            (OrderStatus::PartiallyFilled, OrderStatus::Canceled) => true,
            // Paper venue can go directly from Submitted to Filled
            (OrderStatus::Submitted, OrderStatus::Filled) => true,
            _ => false,
        };
        if valid {
            self.status = new_status;
            self.updated_at = now_nanos();
        }
        valid
    }
}

/// Execution agent internal state.
#[derive(Debug)]
pub struct ExecutionState {
    pub open_orders: HashMap<ClientOrderId, OrderState>,
    pub completed_orders: Vec<OrderState>,
    pub total_orders: u64,
    pub total_fills: u64,
    /// Pending intents indexed by intent id, used to look up original intent data
    /// when a RiskDecision arrives.
    pub pending_intents: HashMap<Uuid, PortfolioIntent>,
}

impl ExecutionState {
    pub fn new() -> Self {
        Self {
            open_orders: HashMap::new(),
            completed_orders: Vec::new(),
            total_orders: 0,
            total_fills: 0,
            pending_intents: HashMap::new(),
        }
    }
}

impl Default for ExecutionState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Execution Agent Entry Point ──────────────────────────────────────────────

/// Run the execution agent as an async task.
pub async fn run_execution_agent(
    bus: Arc<dyn EventBus>,
    venue: Arc<dyn VenueAdapter>,
    config: ExecutionConfig,
    shutdown: ShutdownSignal,
) {
    let mut state = ExecutionState::new();
    let mut risk_rx = bus.subscribe(Topic::Risk);
    let mut intent_rx = bus.subscribe(Topic::Intent);
    let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(10));

    tracing::info!(
        mode = ?config.mode,
        venue = venue.venue_name(),
        "ExecutionAgent started"
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
                        "Open: {}, Filled: {}",
                        state.open_orders.len(),
                        state.total_fills,
                    )),
                ).await;
            }
            result = intent_rx.recv() => {
                match result {
                    Ok(msg) => {
                        if let Payload::PortfolioIntent(intent) = msg.payload {
                            state.pending_intents.insert(intent.id, intent);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lagged = n, "ExecutionAgent intent_rx lagged");
                    }
                    Err(_) => break,
                }
            }
            result = risk_rx.recv() => {
                match result {
                    Ok(msg) => {
                        match &msg.payload {
                            Payload::RiskDecision(decision) => {
                                if decision.approved {
                                    handle_approved_decision(
                                        &bus, &venue, &config, &mut state, decision,
                                    ).await;
                                } else {
                                    // Clean up pending intent on rejection.
                                    state.pending_intents.remove(&decision.intent_id);
                                }
                            }
                            Payload::EmergencyAction(_emergency) => {
                                handle_emergency(&bus, &venue, &mut state).await;
                            }
                            _ => {}
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lagged = n, "ExecutionAgent risk_rx lagged");
                    }
                    Err(_) => break,
                }
            }
        }
    }

    publish_heartbeat(&bus, AgentRunStatus::Stopped, Some("Shutdown".to_string())).await;
    tracing::info!("ExecutionAgent exiting");
}

async fn handle_approved_decision(
    bus: &Arc<dyn EventBus>,
    venue: &Arc<dyn VenueAdapter>,
    config: &ExecutionConfig,
    state: &mut ExecutionState,
    decision: &RiskDecision,
) {
    // Check max open orders limit.
    if state.open_orders.len() >= config.max_open_orders {
        tracing::warn!(
            max = config.max_open_orders,
            "Max open orders reached, skipping"
        );
        return;
    }

    // Look up the original PortfolioIntent by intent_id.
    let original_intent = match state.pending_intents.remove(&decision.intent_id) {
        Some(i) => i,
        None => {
            tracing::warn!(
                intent_id = %decision.intent_id,
                "No pending intent found for approved decision, skipping"
            );
            return;
        }
    };

    // Build an ExecutionOrderIntent from the original PortfolioIntent and decision.
    let client_order_id = ClientOrderId::new(format!("EXE-{}", Uuid::new_v4()));
    let quantity = decision
        .adjusted_quantity
        .unwrap_or(original_intent.quantity);
    let venue_id = Venue::new(original_intent.instrument_id.venue());
    let intent = ExecutionOrderIntent {
        id: Uuid::new_v4(),
        decision_id: decision.id,
        client_order_id: client_order_id.clone(),
        instrument_id: original_intent.instrument_id.clone(),
        venue: venue_id,
        side: original_intent.side,
        order_type: original_intent.order_type,
        quantity,
        limit_price: original_intent.limit_price,
        stop_price: original_intent.stop_price,
        time_in_force: original_intent.time_in_force,
        timestamp: now_nanos(),
    };

    // Create internal order state.
    let mut order_state = OrderState {
        client_order_id: client_order_id.clone(),
        venue_order_id: None,
        instrument_id: intent.instrument_id.clone(),
        side: intent.side,
        order_type: intent.order_type,
        quantity: intent.quantity,
        limit_price: intent.limit_price,
        status: OrderStatus::Initialized,
        filled_quantity: Quantity::zero(8),
        avg_fill_price: None,
        created_at: now_nanos(),
        updated_at: now_nanos(),
    };

    // Transition to Submitted.
    order_state.transition(OrderStatus::Submitted);

    // Submit to venue.
    match venue.submit_order(&intent).await {
        Ok(venue_order_id) => {
            order_state.venue_order_id = Some(venue_order_id.clone());
            order_state.transition(OrderStatus::Filled);
            order_state.filled_quantity = intent.quantity;

            // Query fill price from venue (for paper, fills are instant).
            let fill_price = venue.last_fill_price(&venue_order_id).await;
            order_state.avg_fill_price = fill_price;

            state.total_orders += 1;
            state.total_fills += 1;

            // Publish ExecutionReport (Filled).
            let report = ExecutionReport {
                client_order_id: client_order_id.clone(),
                venue_order_id: Some(venue_order_id),
                instrument_id: intent.instrument_id.clone(),
                status: OrderStatus::Filled,
                side: intent.side,
                filled_quantity: intent.quantity,
                avg_fill_price: fill_price,
                commission: None,
                liquidity_side: Some(LiquiditySide::Taker),
                timestamp: now_nanos(),
            };
            let msg = bus_msg(Topic::Execution, Payload::ExecutionReport(report), "ExecutionAgent");
            let _ = bus.publish(msg).await;

            // Publish PositionUpdate.
            let pos_side = match intent.side {
                OrderSide::Buy => PositionSide::Long,
                OrderSide::Sell => PositionSide::Short,
            };
            let pos_update = PositionUpdate {
                position_id: PositionId::new(format!("POS-{}", intent.instrument_id)),
                instrument_id: intent.instrument_id.clone(),
                side: pos_side,
                quantity: intent.quantity,
                avg_entry_price: fill_price.unwrap_or(Price::zero(2)),
                unrealized_pnl: None,
                realized_pnl: None,
                timestamp: now_nanos(),
            };
            let msg = bus_msg(Topic::Execution, Payload::PositionUpdate(pos_update), "ExecutionAgent");
            let _ = bus.publish(msg).await;

            // Move to completed.
            state.completed_orders.push(order_state);

            tracing::info!(
                client_order_id = %client_order_id,
                "Order filled"
            );
        }
        Err(e) => {
            order_state.transition(OrderStatus::Rejected);
            state.total_orders += 1;

            // Publish ExecutionReport (Rejected).
            let report = ExecutionReport {
                client_order_id: client_order_id.clone(),
                venue_order_id: None,
                instrument_id: intent.instrument_id.clone(),
                status: OrderStatus::Rejected,
                side: intent.side,
                filled_quantity: Quantity::zero(8),
                avg_fill_price: None,
                commission: None,
                liquidity_side: None,
                timestamp: now_nanos(),
            };
            let msg = bus_msg(Topic::Execution, Payload::ExecutionReport(report), "ExecutionAgent");
            let _ = bus.publish(msg).await;

            state.completed_orders.push(order_state);

            tracing::warn!(
                client_order_id = %client_order_id,
                error = %e,
                "Order rejected by venue"
            );
        }
    }
}

async fn handle_emergency(
    bus: &Arc<dyn EventBus>,
    venue: &Arc<dyn VenueAdapter>,
    state: &mut ExecutionState,
) {
    tracing::warn!("Emergency action received — canceling all open orders");

    let open_ids: Vec<(ClientOrderId, Option<VenueOrderId>)> = state
        .open_orders
        .iter()
        .map(|(cid, os)| (cid.clone(), os.venue_order_id.clone()))
        .collect();

    for (client_id, venue_id) in open_ids {
        if let Some(vid) = venue_id {
            if let Err(e) = venue.cancel_order(&vid).await {
                tracing::error!(venue_order_id = %vid, error = %e, "Failed to cancel order");
            }
        }
        if let Some(mut order) = state.open_orders.remove(&client_id) {
            order.transition(OrderStatus::Canceled);

            let report = ExecutionReport {
                client_order_id: client_id,
                venue_order_id: order.venue_order_id.clone(),
                instrument_id: order.instrument_id.clone(),
                status: OrderStatus::Canceled,
                side: order.side,
                filled_quantity: order.filled_quantity,
                avg_fill_price: order.avg_fill_price,
                commission: None,
                liquidity_side: None,
                timestamp: now_nanos(),
            };
            let msg = bus_msg(Topic::Execution, Payload::ExecutionReport(report), "ExecutionAgent");
            let _ = bus.publish(msg).await;

            state.completed_orders.push(order);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_state_valid_transitions() {
        let mut os = OrderState {
            client_order_id: ClientOrderId::new("test"),
            venue_order_id: None,
            instrument_id: InstrumentId::new("BTCUSDT.BINANCE"),
            side: OrderSide::Buy,
            order_type: OrderType::Market,
            quantity: Quantity::from_f64(1.0, 8),
            limit_price: None,
            status: OrderStatus::Initialized,
            filled_quantity: Quantity::zero(8),
            avg_fill_price: None,
            created_at: 0,
            updated_at: 0,
        };

        assert!(os.transition(OrderStatus::Submitted));
        assert_eq!(os.status, OrderStatus::Submitted);

        assert!(os.transition(OrderStatus::Accepted));
        assert_eq!(os.status, OrderStatus::Accepted);

        assert!(os.transition(OrderStatus::Filled));
        assert_eq!(os.status, OrderStatus::Filled);
    }

    #[test]
    fn order_state_invalid_transition() {
        let mut os = OrderState {
            client_order_id: ClientOrderId::new("test"),
            venue_order_id: None,
            instrument_id: InstrumentId::new("BTCUSDT.BINANCE"),
            side: OrderSide::Buy,
            order_type: OrderType::Market,
            quantity: Quantity::from_f64(1.0, 8),
            limit_price: None,
            status: OrderStatus::Initialized,
            filled_quantity: Quantity::zero(8),
            avg_fill_price: None,
            created_at: 0,
            updated_at: 0,
        };

        // Cannot go directly from Initialized to Filled.
        assert!(!os.transition(OrderStatus::Filled));
        assert_eq!(os.status, OrderStatus::Initialized);
    }

    #[test]
    fn order_state_paper_fast_path() {
        let mut os = OrderState {
            client_order_id: ClientOrderId::new("test"),
            venue_order_id: None,
            instrument_id: InstrumentId::new("BTCUSDT.BINANCE"),
            side: OrderSide::Buy,
            order_type: OrderType::Market,
            quantity: Quantity::from_f64(1.0, 8),
            limit_price: None,
            status: OrderStatus::Initialized,
            filled_quantity: Quantity::zero(8),
            avg_fill_price: None,
            created_at: 0,
            updated_at: 0,
        };

        assert!(os.transition(OrderStatus::Submitted));
        // Paper venue fast-path: Submitted -> Filled directly.
        assert!(os.transition(OrderStatus::Filled));
        assert_eq!(os.status, OrderStatus::Filled);
        assert!(os.status.is_terminal());
    }

    #[test]
    fn order_state_rejection_path() {
        let mut os = OrderState {
            client_order_id: ClientOrderId::new("test"),
            venue_order_id: None,
            instrument_id: InstrumentId::new("BTCUSDT.BINANCE"),
            side: OrderSide::Buy,
            order_type: OrderType::Market,
            quantity: Quantity::from_f64(1.0, 8),
            limit_price: None,
            status: OrderStatus::Initialized,
            filled_quantity: Quantity::zero(8),
            avg_fill_price: None,
            created_at: 0,
            updated_at: 0,
        };

        assert!(os.transition(OrderStatus::Submitted));
        assert!(os.transition(OrderStatus::Rejected));
        assert_eq!(os.status, OrderStatus::Rejected);
        assert!(os.status.is_terminal());
    }
}
