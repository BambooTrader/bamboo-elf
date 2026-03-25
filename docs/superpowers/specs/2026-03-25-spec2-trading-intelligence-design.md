# Spec 2: Trading Intelligence

## Overview

Build the agent intelligence layer: Research Agent, Strategy Agent, Cycle Manager, Portfolio Agent, and Risk Agent. This covers PLAN.md Phase 2 + Phase 3.

Depends on Spec 1 infrastructure (EventBus, domain types, TUI shell).

## 1. Cycle Manager

The Cycle Manager owns the trading lifecycle and drives the agent pipeline.

### Responsibilities

- Define cycle boundaries (configurable duration, default 24h)
- Move system through Scan → Focus → Review stages
- Maintain the active watchlist (focus set)
- Maintain a `must_monitor` set for all instruments with open positions
- Trigger exception handling for non-focus instruments needing attention

### Implementation

```rust
pub struct CycleManager {
    bus: Arc<dyn EventBus>,
    config: CycleConfig,
    state: CycleState,
    shutdown: ShutdownSignal,
}

pub struct CycleState {
    pub current_stage: CycleStage,
    pub cycle_id: Uuid,
    pub cycle_start: u64,           // unix nanos
    pub focus_set: Vec<InstrumentId>,
    pub must_monitor: HashSet<InstrumentId>,  // open positions
    pub cycle_count: u32,
}
```

### Cycle Flow

```
CycleManager starts
  → publishes SystemEvent(CycleStageChanged(Scan))
  → Research Agent activates on Scan
  → Research publishes ResearchFindings
  → CycleManager collects findings, builds focus set
  → publishes SystemEvent(CycleStageChanged(Focus))
  → Strategy Agent activates on Focus
  → Normal trading pipeline runs during Focus
  → After configured duration, transitions to Review
  → publishes SystemEvent(CycleStageChanged(Review))
  → Collects results, publishes CycleSummary
  → Loops back to Scan
```

### New Messages

```rust
pub struct CycleStageChanged {
    pub cycle_id: Uuid,
    pub new_stage: CycleStage,
    pub focus_set: Vec<InstrumentId>,
    pub timestamp: u64,
}
```

Add `CycleStageChanged` to the `Payload` enum and route it on `Topic::System`.

## 2. Research Agent

### Purpose

Broad market scanning during Scan phase; continuous monitoring of focus set during Focus phase.

### Deterministic Components

- Load market universe from config (all available symbols on configured exchanges)
- Fetch 24h price changes, volume, and basic metrics via REST API
- Rank assets by a simple scoring formula: `score = volume_rank * 0.4 + change_rank * 0.3 + volatility_rank * 0.3`
- Filter by minimum volume threshold

### LLM-Assisted Components (Phase 2 stub)

- Synthesize research narrative from data (stubbed with template strings for now)
- Explain why assets deserve focus
- The LLM integration point is defined but uses a simple template in Spec 2

### Implementation

```rust
pub struct ResearchAgent {
    bus: Arc<dyn EventBus>,
    config: ResearchConfig,
    shutdown: ShutdownSignal,
    http_client: reqwest::Client,
}

pub struct ResearchConfig {
    pub min_volume_usd: f64,
    pub max_candidates: usize,      // top N to report
    pub scan_interval_secs: u64,    // how often to scan during Scan phase
}
```

### Behavior

- **Scan phase**: Fetch all tickers from Binance REST API, score and rank, publish top N as `ResearchFinding` messages
- **Focus phase**: Monitor focus set tickers for significant changes (>3% move), publish alert findings
- **Review phase**: Idle

### Data Source

Binance REST API: `GET /api/v3/ticker/24hr` returns all 24h ticker data in one call.

## 3. Strategy Agent

### Purpose

Convert research findings into actionable trading signals.

### Deterministic Components

- Strategy template execution (predefined rule-based strategies)
- Signal generation from price patterns
- Parameter validation

### Strategy Templates (Spec 2 ships with 2)

**1. Momentum Strategy**
- Entry: asset in top N by 24h change AND volume above threshold
- Direction: Long if 24h change > 0, Short if < 0
- Exit: time-based (hold for configured hours) or stop-loss
- Stop loss: configurable % from entry

**2. Mean Reversion Strategy**
- Entry: asset dropped >X% in last 24h with high volume (potential oversold bounce)
- Direction: Long only
- Exit: price recovers Y% or time limit
- Stop loss: additional Z% drop

### Implementation

```rust
pub struct StrategyAgent {
    bus: Arc<dyn EventBus>,
    config: StrategyConfig,
    shutdown: ShutdownSignal,
    active_signals: HashMap<InstrumentId, StrategySignal>,
}

pub struct StrategyConfig {
    pub enabled_strategies: Vec<StrategyType>,
    pub momentum: MomentumParams,
    pub mean_reversion: MeanReversionParams,
    pub max_concurrent_signals: usize,
}

pub enum StrategyType { Momentum, MeanReversion }

pub struct MomentumParams {
    pub min_change_pct: f64,
    pub hold_hours: u64,
    pub stop_loss_pct: f64,
}

pub struct MeanReversionParams {
    pub min_drop_pct: f64,
    pub target_recovery_pct: f64,
    pub hold_hours: u64,
    pub stop_loss_pct: f64,
}
```

### Behavior

- Subscribes to `Topic::Signal` for `ResearchFinding` messages
- On each finding, evaluates all enabled strategy templates
- If a strategy triggers, publishes `StrategySignal`
- Tracks active signals to avoid duplicates per instrument
- Strategy does NOT place orders directly (mandatory constraint from PLAN.md)

## 4. Portfolio Agent

### Purpose

Deterministic capital allocation and position sizing. Mandatory intermediary between Strategy and Risk.

### Deterministic Components (no LLM)

- Capital allocation: equal-weight across max_positions
- Position sizing: risk-based sizing (risk per trade = X% of portfolio)
- Rebalance: check if new signal fits within portfolio constraints
- Cash accounting: track available capital

### Implementation

```rust
pub struct PortfolioAgent {
    bus: Arc<dyn EventBus>,
    config: PortfolioConfig,
    shutdown: ShutdownSignal,
    state: PortfolioState,
}

pub struct PortfolioState {
    pub total_capital: Money,
    pub available_capital: Money,
    pub positions: HashMap<InstrumentId, PositionState>,
    pub pending_intents: Vec<Uuid>,  // awaiting risk/execution
}

pub struct PositionState {
    pub side: PositionSide,
    pub quantity: Quantity,
    pub avg_entry: Price,
    pub current_price: Price,
    pub unrealized_pnl: Money,
}
```

### Sizing Logic

```
risk_per_trade = total_capital * risk_pct_per_trade (default 1%)
position_value = min(risk_per_trade / stop_loss_distance, max_position_size)
quantity = position_value / current_price

Check:
- positions_count < max_positions
- position_value + existing_exposure < max_portfolio_exposure
- single_position_value / total_capital < max_concentration_pct
```

### Behavior

- Subscribes to `Topic::Signal` for `StrategySignal`
- Subscribes to `Topic::Execution` for `ExecutionReport` and `PositionUpdate` (feedback loop)
- On StrategySignal: compute position size, check constraints, publish `PortfolioIntent`
- On ExecutionReport: update available capital, position state
- On PositionUpdate: update current position tracking

## 5. Risk Agent

### Purpose

Deterministic risk enforcement. Must function without LLM.

### Hard Risk Controls

- Max position size (from config)
- Max portfolio exposure (from config)
- Per-asset concentration limit (from config)
- Drawdown guard: if portfolio drops X% from peak, halt new entries
- Order rate limiting: max N orders per minute
- Kill switch: if triggered, reject all new intents and publish EmergencyAction

### Implementation

```rust
pub struct RiskAgent {
    bus: Arc<dyn EventBus>,
    config: RiskLimitsConfig,
    shutdown: ShutdownSignal,
    state: RiskState,
}

pub struct RiskState {
    pub peak_equity: Money,
    pub current_equity: Money,
    pub orders_this_minute: u32,
    pub last_minute_reset: u64,
    pub kill_switch_active: bool,
    pub total_exposure: Money,
    pub position_exposures: HashMap<InstrumentId, Money>,
}
```

### Behavior

- Subscribes to `Topic::Intent` for `PortfolioIntent`
- Subscribes to `Topic::Execution` for position/equity updates
- On PortfolioIntent:
  1. Check kill switch → reject if active
  2. Check order rate limit → reject if exceeded
  3. Check position size vs max → adjust or reject
  4. Check total exposure vs max → reject if exceeded
  5. Check concentration vs max → reject if exceeded
  6. Check drawdown vs max → reject if exceeded
  7. If all pass → publish `RiskDecision(approved=true)`
  8. If any fail → publish `RiskDecision(approved=false, reason=...)`
- On equity changes: update peak_equity, check drawdown

## 6. Agent Lifecycle Management

### Agent Status Reporting

Each agent periodically publishes a heartbeat status to `Topic::System`:

```rust
pub struct AgentHeartbeat {
    pub agent_name: String,
    pub status: AgentRunStatus,
    pub last_action: Option<String>,
    pub timestamp: u64,
}

pub enum AgentRunStatus { Starting, Running, Idle, Error(String), Stopped }
```

The TUI's Agent Status panel subscribes to these to show real-time agent state.

## 7. Config Extensions

Add to `AppConfig`:

```toml
[research]
min_volume_usd = 1000000.0
max_candidates = 10
scan_interval_secs = 300

[strategy]
enabled_strategies = ["momentum", "mean_reversion"]
max_concurrent_signals = 5

[strategy.momentum]
min_change_pct = 2.0
hold_hours = 24
stop_loss_pct = 3.0

[strategy.mean_reversion]
min_drop_pct = 5.0
target_recovery_pct = 3.0
hold_hours = 48
stop_loss_pct = 5.0

[portfolio]
risk_pct_per_trade = 1.0
```

## 8. TUI Enhancements

- **Market tab**: Watchlist now shows cycle stage indicator and focus set highlighting
- **Agents tab**: Now functional — shows each agent's status, last action, and message counts
- **Portfolio tab**: Now shows portfolio state — positions, P&L, available capital, exposure

## 9. Testing Strategy

- **Unit tests**: Portfolio sizing math, risk check logic, strategy signal generation
- **Integration tests**: Full pipeline flow (Research → Strategy → Portfolio → Risk) with deterministic inputs
- **Synthetic cycle test**: CycleManager drives a complete Scan → Focus → Review cycle

## 10. Deliverables

| Deliverable | Location |
|-------------|----------|
| CycleManager | bamboo-runtime |
| ResearchAgent (deterministic + Binance REST) | bamboo-runtime |
| StrategyAgent (2 strategy templates) | bamboo-runtime |
| PortfolioAgent (deterministic sizing) | bamboo-runtime |
| RiskAgent (hard controls) | bamboo-runtime |
| Agent heartbeat system | bamboo-runtime |
| Config extensions | bamboo-core |
| TUI: functional Agents + Portfolio tabs | bamboo-terminal |
| Unit + integration tests | all crates |

## 11. Non-Goals

- LLM calls (stubs only, real LLM integration deferred)
- Backtesting engine
- Order placement
- Multi-exchange support
- Historical data storage
