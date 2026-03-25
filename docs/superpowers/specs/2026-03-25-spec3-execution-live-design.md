# Spec 3: Execution + Live Trading

## Overview

Build the execution layer: Paper trading Execution Agent, order lifecycle management, feedback loops, real exchange integration, restart recovery, and safe mode. Covers PLAN.md Phase 4 + Phase 5.

Depends on Spec 1 (infrastructure) and Spec 2 (agents).

## 1. Execution Agent

### Purpose

Convert approved `RiskDecision` into orders, track their lifecycle, and report results back to the system.

### Implementation

```rust
pub struct ExecutionAgent {
    bus: Arc<dyn EventBus>,
    config: ExecutionConfig,
    shutdown: ShutdownSignal,
    venue: Arc<dyn VenueAdapter>,
    state: ExecutionState,
}

pub struct ExecutionConfig {
    pub mode: TradingMode,          // Paper or LiveConstrained
    pub max_open_orders: usize,
    pub order_timeout_secs: u64,
    pub retry_failed_orders: bool,
}

pub struct ExecutionState {
    pub open_orders: HashMap<ClientOrderId, OrderState>,
    pub completed_orders: Vec<OrderState>,
    pub total_orders: u64,
    pub total_fills: u64,
}

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
```

### Behavior

- Subscribes to `Topic::Risk` for approved `RiskDecision`
- On approved decision: create `ExecutionOrderIntent`, submit to venue adapter
- Track order state transitions: Submitted → Accepted → Filled/Rejected/Canceled
- On fill: publish `ExecutionReport` and `PositionUpdate`
- On reject/cancel: publish `ExecutionReport` with error details
- Handle `EmergencyAction`: cancel all open orders, close positions
- Publishes `AgentHeartbeat`

### Order Lifecycle FSM

```
Created → Submitted → Accepted → PartialFill → Filled
                   ↘ Rejected    ↘ Canceled
                                  ↘ Expired
```

## 2. Venue Adapter Trait

Abstract the exchange interface so paper and live trading share the same agent code.

```rust
#[async_trait]
pub trait VenueAdapter: Send + Sync {
    /// Submit a new order
    async fn submit_order(&self, order: &ExecutionOrderIntent) -> Result<VenueOrderId, VenueError>;

    /// Cancel an existing order
    async fn cancel_order(&self, venue_order_id: &VenueOrderId) -> Result<(), VenueError>;

    /// Get current order status
    async fn order_status(&self, venue_order_id: &VenueOrderId) -> Result<OrderStatus, VenueError>;

    /// Get venue name
    fn venue_name(&self) -> &str;

    /// Get trading mode
    fn trading_mode(&self) -> TradingMode;
}

pub enum VenueError {
    OrderRejected(String),
    ConnectionError(String),
    RateLimit,
    InsufficientFunds,
    InvalidOrder(String),
    Unknown(String),
}
```

## 3. Paper Trading Venue

Simulates order execution using real market data.

```rust
pub struct PaperVenue {
    bus: Arc<dyn EventBus>,
    fills: Arc<Mutex<Vec<PaperFill>>>,
    slippage_bps: u32,        // basis points of slippage (default 5 = 0.05%)
    latency_ms: u64,          // simulated latency (default 100ms)
}
```

### Behavior

- On `submit_order`: simulate latency, apply slippage to current market price, generate fill
- Market orders: fill immediately at last price ± slippage
- Limit orders: fill when market price crosses limit (subscribe to MarketTick)
- Stop orders: trigger when price crosses stop level, then fill as market
- Track all paper fills for audit trail
- Return simulated VenueOrderId

### Audit Trail

```rust
pub struct PaperFill {
    pub client_order_id: ClientOrderId,
    pub venue_order_id: VenueOrderId,
    pub instrument_id: InstrumentId,
    pub side: OrderSide,
    pub quantity: Quantity,
    pub fill_price: Price,
    pub slippage_applied: f64,
    pub simulated_commission: Money,
    pub timestamp: u64,
}
```

All paper fills are logged and available for review.

## 4. Live Trading Venue (Binance)

Real exchange integration for constrained live trading.

```rust
pub struct BinanceLiveVenue {
    rest_client: reqwest::Client,
    config: ExchangeConfig,
    api_key: String,
    api_secret: String,
}
```

### Behavior

- Submit orders via Binance REST API: `POST /api/v3/order`
- Cancel orders: `DELETE /api/v3/order`
- Query status: `GET /api/v3/order`
- Sign requests with HMAC-SHA256
- Respect rate limits (Binance: 1200 requests/min)

### Safety

- Only enabled when `TradingMode::LiveConstrained`
- Additional confirmation check: position value must be within configured limits
- All live orders are logged before submission

## 5. Feedback Loops

Complete the information flow back to upstream agents.

### Required Feedback Paths

| From | To | Message | Purpose |
|------|-----|---------|---------|
| Execution | Portfolio | ExecutionReport | Actual fills, fees, slippage |
| Execution | Strategy | ExecutionReport | Signal outcome |
| Execution | Risk | ExecutionReport | Live exposure changes |
| Risk | Execution | EmergencyAction | Force liquidation |

### Implementation

These feedback paths already work through the EventBus — each agent subscribes to relevant topics. Spec 3 ensures:

- PortfolioAgent processes all ExecutionReport to update capital and positions
- StrategyAgent tracks signal outcomes (was the signal profitable?)
- RiskAgent updates exposure on every fill
- ExecutionAgent responds to EmergencyAction by canceling all open orders

### Signal Outcome Tracking

Add to StrategyAgent:

```rust
pub struct SignalOutcome {
    pub signal_id: Uuid,
    pub instrument_id: InstrumentId,
    pub entry_price: Price,
    pub exit_price: Option<Price>,
    pub pnl: Option<Money>,
    pub status: SignalOutcomeStatus,
}

pub enum SignalOutcomeStatus { Open, ProfitTarget, StopLoss, TimedOut, ForceClosed }
```

## 6. State Persistence

Critical trading state must survive restarts.

### What to Persist

- Open positions
- Open orders
- Portfolio state (capital, positions)
- Current cycle state
- Audit trail (paper fills, order history)

### Storage

Use SQLite via `rusqlite` for simplicity:

```rust
pub struct StateStore {
    conn: Connection,
}

impl StateStore {
    pub fn open(path: &str) -> Result<Self>;
    pub fn save_position(&self, pos: &PositionUpdate) -> Result<()>;
    pub fn save_order(&self, order: &OrderState) -> Result<()>;
    pub fn save_portfolio(&self, state: &PortfolioState) -> Result<()>;
    pub fn save_cycle(&self, state: &CycleState) -> Result<()>;
    pub fn load_positions(&self) -> Result<Vec<PositionUpdate>>;
    pub fn load_open_orders(&self) -> Result<Vec<OrderState>>;
    pub fn load_portfolio(&self) -> Result<Option<PortfolioState>>;
    pub fn load_cycle(&self) -> Result<Option<CycleState>>;
}
```

Tables: `positions`, `orders`, `portfolio_state`, `cycle_state`, `audit_trail`

### Recovery on Restart

1. Load persisted state
2. Reconcile open orders with venue (query status)
3. Resume cycle from saved stage
4. Portfolio agent starts with saved state instead of defaults

## 7. Safe Mode

When critical failures occur, the system enters safe mode.

### Triggers

- Kill switch activated
- Drawdown limit breached
- Exchange connectivity lost for >5 minutes
- LLM provider unavailable (future)

### Safe Mode Behavior

- Stop all new order submissions
- Cancel all pending orders
- Optionally close all positions (configurable)
- Continue monitoring (market data feed stays active)
- TUI shows SAFE MODE indicator prominently
- Require manual intervention to exit safe mode

### Implementation

```rust
pub struct SafeMode {
    pub active: AtomicBool,
    pub reason: Mutex<String>,
    pub activated_at: AtomicU64,
}
```

Published as `EmergencyAction` with `EmergencyActionType::HaltNewOrders`.

## 8. Config Extensions

```toml
[execution]
mode = "paper"                    # "paper" or "live_constrained"
max_open_orders = 10
order_timeout_secs = 300
retry_failed_orders = false

[paper]
slippage_bps = 5
latency_ms = 100

[persistence]
db_path = "./bamboo-elf.db"
save_interval_secs = 30
```

## 9. TUI Enhancements

- **Market tab**: Add order status indicator next to positions (Pending/Filled)
- **Portfolio tab**: Show order history, paper fill audit trail
- **New status bar element**: Trading mode indicator (PAPER / LIVE / SAFE MODE)
- Safe mode: red banner across top of TUI

## 10. Testing Strategy

- **Unit tests**: Order FSM transitions, paper venue fill simulation, position sizing with slippage
- **Integration tests**: Full pipeline through paper venue — signal → intent → risk → execution → fill → portfolio update
- **Paper trading cycle test**: Run a complete cycle with synthetic data, verify P&L tracking
- **Persistence tests**: Save state, restart, verify recovery

## 11. Deliverables

| Deliverable | Location |
|-------------|----------|
| ExecutionAgent | bamboo-runtime |
| VenueAdapter trait | bamboo-core |
| PaperVenue | bamboo-runtime |
| BinanceLiveVenue (constrained) | bamboo-runtime |
| StateStore (SQLite persistence) | bamboo-runtime |
| Safe mode system | bamboo-runtime |
| Signal outcome tracking | bamboo-runtime |
| Feedback loop wiring | bamboo-runtime |
| Config extensions | bamboo-core |
| TUI: trading mode indicator, safe mode banner | bamboo-terminal |
| Unit + integration + persistence tests | all crates |

## 12. Non-Goals

- Full backtesting engine
- Multi-exchange simultaneous trading
- P2P networking
- Complex order types (iceberg, TWAP)
- Historical data storage beyond audit trail
