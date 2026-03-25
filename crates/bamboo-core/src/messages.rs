use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::enums::{
    BarInterval, CycleStage, EmergencyActionType, LiquiditySide, OrderSide, OrderStatus,
    OrderType, PositionSide, TimeInForce,
};
use crate::identifiers::{
    ClientOrderId, InstrumentId, PositionId, StrategyId, VenueOrderId, Venue,
};
use crate::types::{Money, Price, Quantity};

/// Real-time price tick from exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketTick {
    pub instrument_id: InstrumentId,
    pub bid: Price,
    pub ask: Price,
    pub last: Price,
    pub volume_24h: Quantity,
    pub timestamp: u64,
}

/// OHLCV candlestick bar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KlineBar {
    pub instrument_id: InstrumentId,
    pub open: Price,
    pub high: Price,
    pub low: Price,
    pub close: Price,
    pub volume: Quantity,
    pub interval: BarInterval,
    pub open_time: u64,
    pub close_time: u64,
}

/// News headline from aggregator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsItem {
    pub title: String,
    pub source: String,
    pub url: Option<String>,
    pub related_instruments: Vec<InstrumentId>,
    pub timestamp: u64,
}

/// Research agent output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchFinding {
    pub id: Uuid,
    pub instrument_id: InstrumentId,
    pub thesis: String,
    pub score: f64,
    pub recommended_action: Option<OrderSide>,
    pub timestamp: u64,
}

/// Strategy agent signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategySignal {
    pub id: Uuid,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub side: OrderSide,
    pub entry_price: Option<Price>,
    pub exit_price: Option<Price>,
    pub stop_loss: Option<Price>,
    pub rationale: String,
    pub confidence: f64,
    pub horizon_hours: u64,
    pub timestamp: u64,
}

/// Portfolio agent sized intent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioIntent {
    pub id: Uuid,
    pub signal_id: Uuid,
    pub instrument_id: InstrumentId,
    pub side: OrderSide,
    pub quantity: Quantity,
    pub order_type: OrderType,
    pub limit_price: Option<Price>,
    pub stop_price: Option<Price>,
    pub time_in_force: TimeInForce,
    pub timestamp: u64,
}

/// Risk agent decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskDecision {
    pub id: Uuid,
    pub intent_id: Uuid,
    pub approved: bool,
    pub reason: String,
    pub adjusted_quantity: Option<Quantity>,
    pub constraints: Vec<String>,
    pub timestamp: u64,
}

/// Concrete order for execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionOrderIntent {
    pub id: Uuid,
    pub decision_id: Uuid,
    pub client_order_id: ClientOrderId,
    pub instrument_id: InstrumentId,
    pub venue: Venue,
    pub side: OrderSide,
    pub order_type: OrderType,
    pub quantity: Quantity,
    pub limit_price: Option<Price>,
    pub stop_price: Option<Price>,
    pub time_in_force: TimeInForce,
    pub timestamp: u64,
}

/// Execution result report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionReport {
    pub client_order_id: ClientOrderId,
    pub venue_order_id: Option<VenueOrderId>,
    pub instrument_id: InstrumentId,
    pub status: OrderStatus,
    pub side: OrderSide,
    pub filled_quantity: Quantity,
    pub avg_fill_price: Option<Price>,
    pub commission: Option<Money>,
    pub liquidity_side: Option<LiquiditySide>,
    pub timestamp: u64,
}

/// Position state change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PositionUpdate {
    pub position_id: PositionId,
    pub instrument_id: InstrumentId,
    pub side: PositionSide,
    pub quantity: Quantity,
    pub avg_entry_price: Price,
    pub unrealized_pnl: Option<Money>,
    pub realized_pnl: Option<Money>,
    pub timestamp: u64,
}

/// End-of-cycle summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleSummary {
    pub cycle_id: Uuid,
    pub stage_completed: CycleStage,
    pub focus_set: Vec<InstrumentId>,
    pub signals_generated: u32,
    pub trades_executed: u32,
    pub pnl: Option<Money>,
    pub notes: String,
    pub timestamp: u64,
}

/// Emergency risk action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmergencyAction {
    pub id: Uuid,
    pub action_type: EmergencyActionType,
    pub reason: String,
    pub affected_instruments: Vec<InstrumentId>,
    pub timestamp: u64,
}
