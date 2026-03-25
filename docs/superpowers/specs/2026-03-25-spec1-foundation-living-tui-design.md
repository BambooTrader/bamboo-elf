# Spec 1: Foundation + Living TUI

## Overview

Build the foundational infrastructure for Bamboo Elf: migrate the ZeroClaw bootstrap, create the 3-crate workspace, define Nautilus-aligned domain types, implement the local event bus, connect real-time crypto market data, build a multi-panel ratatui TUI, and validate with a synthetic end-to-end flow.

This spec covers Phase 1A + 1B from PLAN.md, extended with real-time data integration and an enhanced TUI.

## 1. Repository Migration & Workspace Structure

### Migration Strategy

Move `zeroclaw-copy/` contents up into `bamboo-elf/` root, then create the Bamboo Elf workspace on top.

### Target Directory Structure

```
bamboo-elf/
  Cargo.toml          # workspace root (rewritten, not zeroclaw's)
  Cargo.lock
  PLAN.md
  HANDOFF_*.md
  config.example.toml  # example configuration
  src/                 # zeroclaw original code (preserved, feature-gated)
  crates/
    bamboo-core/       # domain types, messages, traits, config
    bamboo-runtime/    # event bus, agent orchestration, data feeds
    bamboo-terminal/   # ratatui TUI application
  build.rs             # zeroclaw original
  .gitignore
  ... remaining zeroclaw files
```

### Key Decisions

- Workspace root `Cargo.toml` is rewritten as a workspace definition.
- ZeroClaw binary/lib remains as a workspace member for now, not on the MVP hot path.
- Irrelevant zeroclaw modules are feature-gated or ignored, not deleted.

## 2. bamboo-core: Domain Types

### Design Principle

All trading domain types are API-compatible with NautilusTrader conventions. Phase 1 uses self-owned implementations; Phase 2/3 can choose to depend on `nautilus-model` directly or implement `From` conversions.

### Fixed-Point Numeric Types

```rust
/// Price with fixed-point precision, aligned with Nautilus FIXED_SCALAR = 10^9
pub struct Price { raw: i64, precision: u8 }

/// Non-negative quantity
pub struct Quantity { raw: u64, precision: u8 }

/// Money = Price + Currency
pub struct Money { amount: Price, currency: Currency }

/// Currency identifier. Covers both fiat and crypto.
/// Nautilus-aligned: matches CurrencyType distinction.
pub struct Currency {
    pub code: String,           // "USD", "USDT", "BTC", "ETH"
    pub precision: u8,          // decimal places (2 for USD, 8 for BTC)
    pub currency_type: CurrencyType,
}

pub enum CurrencyType { Fiat, Crypto }
```

All message structs derive `Debug, Clone, serde::Serialize, serde::Deserialize`. Fields using `f64` (e.g., `ResearchFinding.score`, `StrategySignal.confidence`) are intentionally not fixed-point — these are non-monetary scoring values in the 0.0-1.0 range.

### Strongly-Typed Identifiers

```rust
pub struct InstrumentId(pub String);   // format: "{Symbol}.{Venue}" e.g. "BTCUSDT.BINANCE"
pub struct Venue(pub String);
pub struct StrategyId(pub String);
pub struct ClientOrderId(pub String);  // internal order ID
pub struct VenueOrderId(pub String);   // exchange-assigned order ID
pub struct PositionId(pub String);
pub struct TradeId(pub String);
pub struct AccountId(pub String);
pub struct ComponentId(pub String);    // agent/component identifier
```

### Enums (Nautilus-Aligned)

```rust
// Order domain
pub enum OrderSide { Buy, Sell }
pub enum OrderType { Market, Limit, StopMarket, StopLimit }
pub enum TimeInForce { GTC, IOC, FOK, GTD, DAY }
pub enum OrderStatus {
    Initialized, Submitted, Accepted, Rejected,
    Canceled, Expired, Triggered,
    PendingUpdate, PendingCancel,
    PartiallyFilled, Filled,
}
pub enum PositionSide { Flat, Long, Short }
pub enum LiquiditySide { Maker, Taker }
pub enum AggressorSide { Buyer, Seller }

// Instrument domain
pub enum AssetClass { Cryptocurrency, Equity, Commodity, Fx, Index }
pub enum InstrumentClass { Spot, Future, Perpetual, Option, Swap }

// Account domain
pub enum AccountType { Cash, Margin }

// Market data
pub enum BookAction { Add, Update, Delete, Clear }
pub enum PriceType { Bid, Ask, Mid, Last, Mark }

// System
pub enum TradingMode { Backtest, Paper, LiveConstrained, LiveFull }
pub enum CycleStage { Scan, Focus, Review }
```

### Core Message Types

Messages that flow on the event bus:

| Message | Direction | Topic | Purpose |
|---------|-----------|-------|---------|
| `MarketTick` | DataFeed -> global | MarketData | Real-time price tick |
| `KlineBar` | DataFeed -> global | MarketData | OHLCV bar |
| `NewsItem` | NewsFeed -> global | News | News headline + summary |
| `ResearchFinding` | Research -> Strategy | Signal | Market discovery, watchlist recommendation |
| `StrategySignal` | Strategy -> Portfolio | Signal | Entry/exit signal + rationale |
| `PortfolioIntent` | Portfolio -> Risk | Intent | Position-sized trade intent |
| `RiskDecision` | Risk -> Execution | Risk | Approve/reject with constraints |
| `ExecutionOrderIntent` | Execution internal | Execution | Concrete order |
| `ExecutionReport` | Execution -> global | Execution | Fill, reject, cancel |
| `PositionUpdate` | global | Execution | Position change notification |
| `CycleSummary` | CycleManager -> global | System | Cycle review results |
| `EmergencyAction` | Risk -> Execution | Risk | Emergency liquidation / circuit breaker |

### Message Struct Definitions

```rust
/// Real-time price tick from exchange
pub struct MarketTick {
    pub instrument_id: InstrumentId,
    pub bid: Price,
    pub ask: Price,
    pub last: Price,
    pub volume_24h: Quantity,
    pub timestamp: u64,           // unix nanos, aligned with Nautilus UnixNanos
}

/// OHLCV candlestick bar
pub struct KlineBar {
    pub instrument_id: InstrumentId,
    pub open: Price,
    pub high: Price,
    pub low: Price,
    pub close: Price,
    pub volume: Quantity,
    pub interval: BarInterval,     // e.g. Min1, Min5, Hour1, Day1
    pub open_time: u64,
    pub close_time: u64,
}

pub enum BarInterval { Min1, Min5, Min15, Hour1, Hour4, Day1 }

/// News headline from aggregator
pub struct NewsItem {
    pub title: String,
    pub source: String,
    pub url: Option<String>,
    pub related_instruments: Vec<InstrumentId>,
    pub timestamp: u64,
}

/// Research agent output
pub struct ResearchFinding {
    pub id: Uuid,
    pub instrument_id: InstrumentId,
    pub thesis: String,              // human-readable research summary
    pub score: f64,                  // relevance/conviction score 0.0-1.0
    pub recommended_action: Option<OrderSide>,
    pub timestamp: u64,
}

/// Strategy agent signal
pub struct StrategySignal {
    pub id: Uuid,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub side: OrderSide,
    pub entry_price: Option<Price>,   // None = market order
    pub exit_price: Option<Price>,
    pub stop_loss: Option<Price>,
    pub rationale: String,
    pub confidence: f64,              // 0.0-1.0
    pub horizon_hours: u64,
    pub timestamp: u64,
}

/// Portfolio agent sized intent
pub struct PortfolioIntent {
    pub id: Uuid,
    pub signal_id: Uuid,              // links back to StrategySignal
    pub instrument_id: InstrumentId,
    pub side: OrderSide,
    pub quantity: Quantity,
    pub order_type: OrderType,
    pub limit_price: Option<Price>,
    pub stop_price: Option<Price>,
    pub time_in_force: TimeInForce,
    pub timestamp: u64,
}

/// Risk agent decision
pub struct RiskDecision {
    pub id: Uuid,
    pub intent_id: Uuid,              // links back to PortfolioIntent
    pub approved: bool,
    pub reason: String,
    pub adjusted_quantity: Option<Quantity>,  // risk may reduce size
    pub constraints: Vec<String>,            // e.g. "max 50% of daily limit"
    pub timestamp: u64,
}

/// Concrete order for execution
pub struct ExecutionOrderIntent {
    pub id: Uuid,
    pub decision_id: Uuid,            // links back to RiskDecision
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

/// Execution result report
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

/// Position state change
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

/// End-of-cycle summary
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

/// Emergency risk action
pub struct EmergencyAction {
    pub id: Uuid,
    pub action_type: EmergencyActionType,
    pub reason: String,
    pub affected_instruments: Vec<InstrumentId>,
    pub timestamp: u64,
}

pub enum EmergencyActionType { KillSwitch, ForceLiquidate, ReduceExposure, HaltNewOrders }
```

Note: All timestamps use `u64` unix nanoseconds, aligned with Nautilus `UnixNanos` convention. This avoids chrono serialization overhead on the hot path.

### Configuration Model

```rust
pub struct AppConfig {
    pub exchanges: Vec<ExchangeConfig>,
    pub universe: UniverseConfig,
    pub cycle: CycleConfig,
    pub risk: RiskLimitsConfig,
    pub portfolio: PortfolioConfig,
    pub tui: TuiConfig,
}

pub struct ExchangeConfig {
    pub name: String,           // "binance"
    pub ws_url: String,
    pub rest_url: String,
    pub api_key_env: String,    // env var name, not the key itself
    pub api_secret_env: String,
}

pub struct UniverseConfig {
    pub default_symbols: Vec<String>,
    pub max_focus_set: usize,
}

pub struct CycleConfig {
    pub default_duration_hours: u64,
    pub auto_advance: bool,
}

/// Config values are parsed as f64 from TOML, then converted to fixed-point
/// Money/Price at load time using Price::from_f64(value, precision).
/// Default precision for USD config values: 2 (cents).
pub struct RiskLimitsConfig {
    pub max_position_size_usd: Money,
    pub max_portfolio_exposure_usd: Money,
    pub max_concentration_pct: u8,      // 0-100 integer percent
    pub max_drawdown_pct: u8,           // 0-100 integer percent
    pub order_rate_limit_per_min: u32,
    pub kill_switch_enabled: bool,
}

pub struct PortfolioConfig {
    pub initial_capital_usd: Money,
    pub max_positions: usize,
}

pub struct TuiConfig {
    pub tick_rate_ms: u64,
    pub sparkline_window: usize,    // number of ticks to show in sparkline
}
```

Configuration is loaded from TOML. Default path: `./config.toml`, overridable with `--config <path>` CLI argument. Secrets are read from environment variables, never stored in config files.

## 3. Local Event Bus

### Design Choice

Option B: unified Bus trait with topic-based registry (over raw tokio channels), for observability and clean interfaces.

### Core Types

```rust
pub struct BusMessage {
    pub id: Uuid,
    pub topic: Topic,
    pub payload: Payload,
    pub timestamp: u64,              // unix nanos, consistent with all message payloads
    pub source: ComponentId,
}

pub enum Topic {
    MarketData,
    News,
    Signal,
    Intent,
    Risk,
    Execution,
    System,
}

pub enum Payload {
    MarketTick(MarketTick),
    KlineBar(KlineBar),
    NewsItem(NewsItem),
    ResearchFinding(ResearchFinding),
    StrategySignal(StrategySignal),
    PortfolioIntent(PortfolioIntent),
    RiskDecision(RiskDecision),
    ExecutionOrderIntent(ExecutionOrderIntent),
    ExecutionReport(ExecutionReport),
    PositionUpdate(PositionUpdate),
    CycleSummary(CycleSummary),
    EmergencyAction(EmergencyAction),
}
```

### Bus Trait

```rust
#[async_trait]
pub trait EventBus: Send + Sync {
    /// Publish a message. Returns the number of active subscribers that received it.
    /// If a subscriber has lagged (buffer full), it is counted in metrics but not in the return value.
    async fn publish(&self, msg: BusMessage) -> Result<usize, BusError>;
    fn subscribe(&self, topic: Topic) -> BusReceiver;
    fn subscribe_all(&self) -> BusReceiver;
    fn metrics(&self) -> BusMetrics;
}

pub enum BusError {
    NoSubscribers,
    ChannelClosed,
}

pub struct BusMetrics {
    pub messages_published: u64,
    pub messages_per_topic: HashMap<Topic, u64>,
    pub queue_depth: HashMap<Topic, usize>,
}
```

### LocalBus Implementation

- Uses `tokio::broadcast` per topic with bounded capacity (default 1024 per topic).
- `subscribe_all()` uses an additional broadcast channel that receives copies of all messages (for TUI and observability).
- `BusMetrics` exposed via atomic counters, no lock contention on the publish path.
- Optional `RingBuffer<BusMessage>` for future replay testing support.

## 4. Real-Time Data Integration

### Market Data Feed Trait

```rust
#[async_trait]
pub trait MarketDataFeed: Send + Sync {
    async fn connect(&mut self) -> Result<()>;
    async fn subscribe(&mut self, instruments: &[InstrumentId]) -> Result<()>;
    async fn unsubscribe(&mut self, instruments: &[InstrumentId]) -> Result<()>;
    fn status(&self) -> FeedStatus;
}

pub enum FeedStatus { Connected, Reconnecting, Disconnected, Error(String) }
```

### BinanceFeed (Phase 1 Implementation)

- Connects to Binance spot WebSocket (`wss://stream.binance.com:9443/ws`).
- Subscribes to ticker, kline, and depth streams.
- Converts Binance JSON to `MarketTick` / `KlineBar` and publishes to `EventBus(MarketData)`.
- Auto-reconnect with exponential backoff (initial 1s, max 60s) + heartbeat/pong handling.
- Max reconnect attempts: unlimited, but logs warning after 5 consecutive failures.
- On persistent disconnection: `FeedStatus::Disconnected` propagated to TUI, watchlist shows stale indicator.
- Works in all trading modes (real market data, even in paper mode).

### News Feed

- HTTP polling: CoinGecko trending API, CryptoCompare news API.
- Converts to `NewsItem` messages published to `EventBus(News)`.
- Low frequency: every 5-15 minutes.
- No WebSocket needed.

### Data Flow

```
Binance WS --> BinanceFeed --> EventBus(MarketData)
                                   |-> TUI watchlist (live prices)
                                   |-> TUI sparkline (price trends)
                                   |-> future: Research/Strategy agents

News HTTP --> NewsFeed --> EventBus(News)
                              |-> TUI news panel
```

### Configuration

```toml
[[exchanges]]
name = "binance"
ws_url = "wss://stream.binance.com:9443/ws"
rest_url = "https://api.binance.com"
api_key_env = "BINANCE_API_KEY"
api_secret_env = "BINANCE_API_SECRET"

[universe]
default_symbols = ["BTCUSDT", "ETHUSDT", "SOLUSDT"]
max_focus_set = 10
```

## 5. TUI Design

### Overall Architecture

Multi-panel financial terminal using ratatui, with tab-based navigation.

### Default Layout (Market Tab)

```
+-- Tabs: [Market] [Portfolio] [Agents] [Logs] --------------------+
+----------------------------------+-------------------------------+
|  Watchlist                       |  News Feed                    |
|  BTC/USDT  68,432.50 ^+2.1% ####|  15:03 BTC breaks 68k...     |
|  ETH/USDT   3,841.20 v-0.8% ####|  14:58 Fed holds rates...    |
|  SOL/USDT     187.30 ^+5.2% ####|  14:42 SOL DEX volume...     |
+----------------------------------+-------------------------------+
|  Positions                       |  Agent Status                 |
|  BTC  Long 0.5 +$1,240  +3.6%   |  Research   * idle            |
|  ETH  Short 2.0 -$180   -2.3%   |  Strategy   * idle            |
|                                  |  Portfolio  * idle            |
|  Total P&L: +$1,060             |  Risk       * ok              |
|                                  |  Execution  * ready           |
+----------------------------------+-------------------------------+
|  Event Log (subscribe_all)                                       |
|  15:03:01 [MarketData] BTC tick 68432.50                        |
|  15:03:00 [System] BinanceFeed connected                        |
|  15:02:58 [System] EventBus started, 6 topics                   |
+------------------------------------------------------------------+
```

### Panel Specifications

| Panel | Widget | Data Source | Update Frequency |
|-------|--------|-------------|------------------|
| Watchlist | Table + Sparkline | EventBus(MarketData) | Real-time tick |
| News | List | EventBus(News) | Poll 5-15min |
| Positions | Table | EventBus(Execution) | Event-driven |
| Agent Status | Table + status indicator | EventBus(System) | Heartbeat |
| Event Log | Scrollable List | subscribe_all() | Real-time |
| Tabs | Tabs widget | Keyboard | User action |

### Tab Pages

- **Market** - default layout above (watchlist + news + positions + agents + log)
- **Portfolio** - detailed positions, P&L curve (populated in Spec 2)
- **Agents** - agent detailed status, recent decisions (populated in Spec 2)
- **Logs** - full-screen log view with filtering

### Keyboard Navigation

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Switch tab |
| `1-4` | Jump to tab directly |
| `j/k` or arrow keys | Scroll within focused panel |
| `Ctrl+arrow` | Switch panel focus |
| `q` | Quit |

### App Structure

```rust
pub struct App {
    bus_rx: BusReceiver,
    watchlist: WatchlistState,      // prices + sparkline ring buffers
    positions: Vec<PositionRow>,
    agents: Vec<AgentStatus>,
    events: RingBuffer<LogEntry>,   // capacity: 500 entries
    news: Vec<NewsItem>,
    active_tab: usize,
    focused_panel: PanelId,
}

/// Watchlist state: one entry per subscribed instrument
pub struct WatchlistState {
    pub entries: Vec<WatchlistEntry>,
}

pub struct WatchlistEntry {
    pub instrument_id: InstrumentId,
    pub last_price: Price,
    pub change_pct: f64,                       // display-only, ok as f64
    pub sparkline: RingBuffer<f64>,            // last N prices, sized to TuiConfig::sparkline_window (default: 120)
    pub feed_status: FeedStatus,               // shows stale indicator if disconnected
}
```

### Main Loop

```rust
async fn run(terminal, bus_rx, tick_rate: Duration) {
    loop {
        tokio::select! {
            msg = bus_rx.recv() => app.handle_bus_message(msg),
            Ok(true) = crossterm_event_available() => app.handle_input(),
            _ = tick_interval.tick() => {}  // force redraw
        }
        terminal.draw(|f| app.render(f))?;
    }
}
```

## 6. Synthetic End-to-End Flow

### Purpose

Validate the full message pipeline before connecting real exchange integration.

### Flow

```
SyntheticFeed --> MarketTick --> Bus
                                 |
MockResearch <-------------------+
    |-> ResearchFinding --> Bus
                             |
MockStrategy <---------------+
    |-> StrategySignal --> Bus
                            |
MockPortfolio <-------------+
    |-> PortfolioIntent --> Bus
                             |
MockRisk <-------------------+
    |-> RiskDecision --> Bus
                          |
MockExecution <-----------+
    |-> ExecutionReport --> Bus
    |-> PositionUpdate --> Bus
```

### Implementation

Each mock agent is a tokio task that subscribes to its upstream topic and publishes downstream messages with simple logic and realistic delays.

### Validation Checklist

- All 6 core message types flow through the bus successfully.
- Every TUI panel receives and displays its corresponding messages.
- Event Log shows the complete message chain.
- Agent Status panel reflects each mock agent's running state.
- Watchlist sparkline renders trend lines from synthetic price data.

### Transition to Real Data

Replace `SyntheticFeed` with `BinanceFeed`. The rest of the pipeline remains unchanged. Mock agents are replaced with real implementations in Spec 2/3.

## 7. Error Strategy

All three crates use `thiserror` for typed errors, with a shared base in `bamboo-core`:

```rust
// bamboo-core
#[derive(Debug, thiserror::Error)]
pub enum BambooError {
    #[error("config: {0}")]
    Config(String),
    #[error("bus: {0}")]
    Bus(#[from] BusError),
    #[error("feed: {0}")]
    Feed(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

pub type BambooResult<T> = Result<T, BambooError>;
```

`bamboo-runtime` and `bamboo-terminal` extend with crate-specific error variants as needed, all convertible to `BambooError` via `From`.

## 8. Shutdown Strategy

Graceful shutdown sequence triggered by `q` key or SIGINT/SIGTERM:

1. Set global `AtomicBool` shutdown flag.
2. Data feeds: close WebSocket connections, stop polling.
3. Mock agents: exit task loops on shutdown flag.
4. Event bus: drain remaining messages (bounded wait, max 2 seconds).
5. TUI: restore terminal (disable raw mode, show cursor, disable mouse capture).
6. Exit process.

No persistent state to save in Spec 1 (no real positions). Pattern established here for Spec 2/3 to add state persistence before step 5.

## 9. Tab Page Stubs

Portfolio and Agents tabs render a centered placeholder in Spec 1:

```
┌─ Portfolio ──────────────────────────────────────────┐
│                                                       │
│           Portfolio details coming in Spec 2          │
│                                                       │
└───────────────────────────────────────────────────────┘
```

The Logs tab is functional from Spec 1: full-screen scrollable view of all bus messages with topic-based color coding.

## 10. Deliverables Summary

| Deliverable | Crate | Required |
|-------------|-------|----------|
| Cargo workspace + 3 crates compile | workspace root | yes |
| Domain types (Price/Quantity/Order enums, Nautilus-aligned) | bamboo-core | yes |
| 10+ core message types + BusMessage wrapper | bamboo-core | yes |
| AppConfig + TOML loading | bamboo-core | yes |
| EventBus trait + LocalBus implementation | bamboo-runtime | yes |
| MarketDataFeed trait + BinanceFeed | bamboo-runtime | yes |
| NewsFeed (HTTP polling) | bamboo-runtime | yes |
| Synthetic flow (6 mock agent tasks) | bamboo-runtime | yes |
| TUI 4-tab layout + 5 panels + sparkline | bamboo-terminal | yes |
| Keyboard navigation (tab switch, scroll, quit) | bamboo-terminal | yes |
| zeroclaw code migration to root | repo-level | yes |

## 11. Explicit Non-Goals

- Real Research/Strategy/Portfolio/Risk/Execution agent logic
- LLM calls
- Order placement (paper or live)
- Backtesting
- Draggable/resizable layout
- P2P networking
- Multi-exchange support beyond Binance

## 12. Dependencies

```
bamboo-terminal --> bamboo-runtime --> bamboo-core
```

External crates: `tokio`, `serde`, `toml`, `ratatui`, `crossterm`, `tokio-tungstenite`, `reqwest`, `uuid`, `chrono`, `thiserror`, `anyhow`

## 13. Testing Strategy

- **Unit tests:** Price/Quantity arithmetic, message serialization, config parsing, OrderStatus FSM transitions
- **Integration tests:** EventBus publish/subscribe flow, mock agent pipeline end-to-end
- **Manual verification:** TUI startup with synthetic data flowing across all panels
