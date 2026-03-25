# Bamboo Elf Plan

## Project Positioning

Bamboo Elf is an AI-native quantitative trading operating environment.

It is not a single trading bot. It is a long-running agent system that should:

- monitor markets
- produce research
- generate and evaluate strategy hypotheses
- allocate capital
- enforce risk controls
- execute orders
- learn across trading cycles
- expose the whole system through a native Rust terminal UI

The first product form is a native terminal application built with `ratatui`.

## What This Project Is And Is Not

### It Is

- an agent-driven quant research and trading workstation
- a system that combines deterministic trading infrastructure with LLM-assisted reasoning
- a crypto-first MVP with clean interfaces for future equities, futures, and commodities support

### It Is Not

- a pure "chat with an LLM and maybe trade" app
- a full multi-node distributed research network in MVP
- a fully general Bloomberg replacement in the first release
- a system where every decision depends on an LLM call

## Product Goal

Build a practical MVP that proves four things:

1. Bamboo Elf can scan a tradable universe and choose a small cycle watchlist.
2. Bamboo Elf can turn research into strategy signals and paper-trade or live-trade them on crypto venues.
3. Bamboo Elf can separate strategy, portfolio, risk, and execution concerns correctly.
4. Bamboo Elf can show the full system state in a native TUI.

## MVP Definition

The MVP is deliberately narrower than the long-term vision.

### MVP Includes

- one runtime process
- one local event bus
- crypto market support only
- one or two exchanges at most
- one or two strategy families at most
- paper trading first, limited live trading second
- a minimal but usable `ratatui` workspace
- research, strategy, portfolio, risk, and execution roles

### MVP Excludes

- P2P multi-node networking
- full equities/commodities/futures execution
- broad marketplace of pluggable agent packages
- heavy autonomous self-modification
- complex cross-cluster scheduling

## Key Decisions

- Runtime foundation: start from `zeroclaw-copy/`
- ZeroClaw strategy: keep the codebase available, but feature-gate or ignore irrelevant modules during MVP work
- Trading engine path: use `NautilusTrader` in phases, not all at once
- Initial market focus: crypto first
- Future market readiness: keep interfaces market-agnostic from day one
- UI direction: pure Rust `ratatui` TUI, no Ghostty
- Agent communication for MVP: in-process event bus only
- P2P: deferred until after a stable single-process system exists
- Autonomy target: fully autonomous within explicit controls, but paper trading comes before live autonomy
- Timeline target: MVP in 4-6 months if scope is kept narrow

## Architectural Review Adjustments

The original draft was too broad for the target timeline. The following corrections are now part of the official plan:

- start with 3 crates, not 8
- define explicit deterministic vs LLM boundaries
- do not implement P2P in MVP
- add a minimal TUI in Phase 1 instead of waiting until late
- add feedback loops to the agent system
- add failure-mode handling and test strategy from the start
- use phased NautilusTrader integration
- avoid unnecessary ZeroClaw surface area in the active build

## Current State

Current repository state:

- planning documents exist
- `zeroclaw-copy/` exists and has not been refactored yet
- no Cargo workspace has been created for Bamboo Elf
- no `crates/` directory exists yet
- no Bamboo Elf domain types exist yet
- no event bus exists yet
- no TUI exists yet
- no trading adapter exists yet

This is still a planning-stage project.

## Repository Restructure Plan

`zeroclaw-copy/` is a temporary staging location, not the long-term project layout.

In this plan, "root directory" and "project root" specifically mean:

- `/Users/bamboo/Githubs/BambooTrader/bamboo-elf/`

This local project root is intended to correspond to the remote repository:

- `https://github.com/BambooTrader/bamboo-elf.git`

### Development Start Rule

When implementation begins, enter:

- `/Users/bamboo/Githubs/BambooTrader/bamboo-elf/`

Then move the contents of `zeroclaw-copy/` to the parent directory so they land in `bamboo-elf/`, conceptually like:

```bash
mv zeroclaw-copy/* ..
```

After that, continue development directly from `bamboo-elf/` root.

Important: the real intent is not the exact shell spelling, but the repository outcome:

- the ZeroClaw bootstrap files should live directly under `bamboo-elf/`
- `bamboo-elf/` becomes the only active project root
- `zeroclaw-copy/` stops being the place where development happens

### Why

- Bamboo Elf should become the real root project
- `zeroclaw-copy/` is only a bootstrap source, not a permanent subproject
- future workspace files, crates, config, and docs should live directly under `bamboo-elf/`

### Practical Intent

After migration, Bamboo Elf should look like a project that started from ZeroClaw and then evolved in place, not like a wrapper around a nested copy.

### Migration Guidance

- preserve the original ZeroClaw files as the initial codebase
- move files first, then refactor incrementally
- do not spend early effort on large cleanup passes before the runtime builds from the new root
- once the move is complete, treat `bamboo-elf/` as the only active project root
- in practice, think of the migration as "move `zeroclaw-copy/*` to the parent directory so the files end up directly under `bamboo-elf/`, then start development there"

## Core System Principle

The system must combine:

- deterministic infrastructure for anything safety-critical or latency-sensitive
- LLM reasoning for synthesis, interpretation, prioritization, and strategy ideation

This principle should drive every implementation decision.

## LLM Boundary By Agent

This is the single most important architectural rule.

### Research Agent

#### Deterministic

- market universe loading
- screener metrics
- news collection
- macro data ingestion
- ranking inputs and filters

#### LLM-Assisted

- research synthesis
- explaining why assets deserve focus
- regime interpretation
- hypothesis framing

### Strategy Agent

#### Deterministic

- strategy template execution
- indicator calculation
- backtest dispatch
- parameter range validation

#### LLM-Assisted

- proposing strategy hypotheses
- selecting or mutating strategy templates
- explaining signal rationale
- choosing which tests to run next

### Portfolio Agent

#### Deterministic

- capital allocation rules
- position sizing math
- cash and margin accounting
- rebalance triggers

#### LLM-Assisted

- optional portfolio commentary
- optional allocation explanations

Important: portfolio allocation should not depend on LLM availability.

### Risk Agent

#### Deterministic

- hard risk limits
- drawdown checks
- exposure checks
- concentration limits
- correlation limits
- kill-switch triggers

#### LLM-Assisted

- risk narrative
- soft warnings
- regime-sensitive risk interpretation

Important: approvals and emergency stops must be possible without any LLM.

### Execution Agent

#### Deterministic

- order creation
- venue routing
- order state tracking
- fill handling
- retries and cancellations

#### LLM-Assisted

- none on the critical execution path for MVP

## Agent Topology

The core pipeline is:

`Research -> Strategy -> Portfolio -> Risk -> Execution`

This is the correct forward decision path.

### Research Agent

- broad market scanning at cycle start
- news and macro digestion
- watchlist candidate discovery
- produces structured research findings

### Strategy Agent

- converts research into hypotheses and signals
- defines entry, exit, horizon, invalidation, and rationale
- requests backtests and evaluations

### Portfolio Agent

- portfolio allocation
- capital allocation
- position sizing
- rebalance decisions
- ensures strategy does not directly become order flow

### Risk Agent

- exposure checks
- drawdown and concentration checks
- correlation and regime checks
- approval or rejection with constraints
- circuit breaker and emergency liquidation authority

### Execution Agent

- converts approved intent into orders
- venue routing
- execution monitoring
- fill and order lifecycle management

## Agent Feedback Loops

The system must not be modeled as a one-way pipeline only.

### Required Feedback Paths

- Execution -> Portfolio: actual fills, partial fills, fees, slippage
- Execution -> Strategy: signal outcome, execution quality, cancel/reject outcomes
- Execution -> Risk: live exposure changes, liquidation status, venue errors
- Risk -> Execution: emergency liquidation or forced reduction
- Review cycle -> Research and Strategy: what worked, what failed, what to keep

### Required Core Messages

- `ResearchFinding`
- `StrategySignal`
- `PortfolioIntent`
- `RiskDecision`
- `ExecutionOrderIntent`
- `ExecutionReport`
- `PositionUpdate`
- `CycleSummary`
- `EmergencyAction`

## Trading Cycle Model

The system should reduce cost by using cycle-based attention instead of always-on full-universe deep analysis.

### Cycle Stages

#### 1. Scan

- broad universe screening
- identify assets worth attention
- choose the focus set for the cycle

#### 2. Focus

- deeply monitor only selected assets during the cycle
- adjust strategy using price action, news, macro, and portfolio context
- avoid unnecessary focus rotation mid-cycle

#### 3. Review

- evaluate outcomes at cycle end
- record what worked and failed
- decide what to keep or replace next cycle

## Cycle Manager

Cycle behavior needs an explicit owner.

### Responsibilities

- define cycle boundaries
- move the system between Scan, Focus, and Review
- keep the active watchlist
- maintain a `must_monitor` set for all assets with open positions
- trigger exception handling when non-focus assets need urgent attention

### Initial Policy

- default cycle duration should be configurable
- crypto can start with daily or multi-day cycles
- focus set should be intentionally small
- open positions must remain monitored even if the asset is no longer in the next focus set

### Exception Triggers

Examples:

- large price gap or volatility spike
- exchange outage or delisting event
- major breaking news on held asset
- risk limit breach

## Architecture Overview

The architecture should be described as dependency layers, not a fake linear call chain.

```text
terminal/ui  ----\
gateway       ----+-----------------------> bamboo-core
runtime       ----/                            |
agents        -------------------------------> |
trading       -------------------------------> |
memory        -------------------------------> |
data          -------------------------------> |
                                                v
                                      shared types + traits
```

For MVP, the actual implementation should remain simpler than this conceptual model.

## Initial Crate Strategy

Do not start with 8 crates.

### Start With 3 Crates

```text
bamboo-elf/
  PLAN.md
  HANDOFF_*.md
  zeroclaw-copy/
  crates/
    bamboo-core/
    bamboo-runtime/
    bamboo-terminal/
```

### `bamboo-core`

- domain types
- event/message schemas
- shared traits
- configuration models
- risk and execution enums

### `bamboo-runtime`

- ZeroClaw-based runtime work
- agent orchestration
- local event bus
- trading tools
- data ingestion coordination
- persistence integration
- gateway integration if needed

### `bamboo-terminal`

- `ratatui` application shell
- log view
- agent status
- watchlist panel
- position panel

## Planned Future Extractions

Only split these out when the code justifies it:

- `bamboo-trading`
- `bamboo-memory`
- `bamboo-data`
- `bamboo-gateway`
- `bamboo-bus`
- `bamboo-agents`

Extraction should follow real pressure from code size, compile times, or ownership boundaries.

## ZeroClaw Reuse Strategy

ZeroClaw provides useful foundations, but not a full quant architecture.

### Reusable Areas

- agent loop patterns
- provider abstraction
- tool dispatch
- memory trait patterns
- observability patterns
- scheduling and heartbeat ideas
- gateway scaffolding

### Low-Relevance Areas For MVP

- most chat channels
- hardware peripherals
- Arduino and serial integrations
- hardware-oriented RAG
- onboarding wizard
- most tunnels

### Rule

Do not spend time deleting everything immediately.

Instead:

- keep the copy available
- avoid integrating irrelevant modules into MVP build paths
- add feature flags later if necessary

After the initial repository migration, this guidance still applies to the moved code now living under `bamboo-elf/` root.

## Reference Reuse From `library/`

The `library/` directory is not just for inspiration. It should actively inform implementation choices.

### Available References

- `/Users/bamboo/Githubs/BambooTrader/library/zeroclaw/`
- `/Users/bamboo/Githubs/BambooTrader/library/nautilus_trader/`
- `/Users/bamboo/Githubs/BambooTrader/library/OpenAlice/`
- `/Users/bamboo/Githubs/BambooTrader/library/OpenViking/`
- `/Users/bamboo/Githubs/BambooTrader/library/agi/`
- `/Users/bamboo/Githubs/BambooTrader/library/ratatui/`

### How To Reuse Each Reference

#### `library/zeroclaw/`

Borrow:

- provider abstraction patterns
- tool-dispatch architecture
- agent-loop structure
- heartbeat and cron ideas
- observability patterns
- gateway scaffolding

Do not copy its product assumptions blindly.

Do not let Bamboo Elf become a chat-channel aggregation project or a hardware runtime.

#### `library/nautilus_trader/`

Borrow:

- domain modeling patterns for instruments, prices, quantities, orders, and fills
- adapter design for venues
- backtest and live-trading mental model
- portfolio and risk concepts

Use in phases:

- early stage: learn from types and architecture, selectively use lighter crates
- later stage: integrate deeper live/backtest components only if justified

Do not make MVP depend on the heaviest parts before the event flow and domain model are stable.

#### `library/OpenAlice/`

Borrow:

- guardrail patterns
- trade audit-trail ideas
- heartbeat and scheduled-agent ideas
- product-level thinking around AI trading workflows

OpenAlice is the closest application-level benchmark, but Bamboo Elf should improve on:

- portfolio-level risk separation
- deterministic risk controls
- structured multi-agent workflow

#### `library/OpenViking/`

Borrow:

- hierarchical memory loading concepts
- long-term research organization patterns
- ideas for cycle summaries, strategy journals, and memory tiers

Do not overbuild memory infrastructure before the core trading loop exists.

#### `library/agi/`

Borrow:

- multi-agent coordination concepts
- research-network thinking
- future P2P ideas

Important:

- this is a future reference, not an MVP dependency
- do not build distributed orchestration before the single-process local system works

#### `library/ratatui/`

Borrow:

- terminal layout patterns
- widget usage
- rendering structure for status panels, tables, and charts
- practical implementation examples for a Rust-native TUI

This is the direct UI implementation reference for Bamboo Elf.

### Reuse Policy

- borrow architecture and patterns first
- borrow code selectively when it clearly reduces risk
- avoid importing complexity just because a reference project has it
- every borrowed concept must justify itself against MVP scope

## Trading Infrastructure Strategy

Use NautilusTrader in phases.

### Phase A - Lightweight MVP Use

Prefer using:

- `nautilus-core`
- `nautilus-model`

These provide useful domain types without forcing the full framework into the MVP.

### Phase B - Broader Trading Integration

Evaluate adding:

- selected adapter crates
- backtest components
- live node components

Only do this after Bamboo Elf has:

- stable domain types
- stable event flow
- stable paper trading loop

### Fallback For MVP

If integration friction is too high, use direct exchange REST/WebSocket APIs while preserving Bamboo Elf's market abstraction layer.

## Market Scope

### MVP Live Scope

- crypto only

### Architecture Scope

- crypto first
- stocks prepared
- futures prepared
- commodities prepared

### Guardrail

Do not implement market-specific complexity for non-crypto markets during MVP unless it is required by a shared abstraction.

## Local Event Bus Strategy

For MVP use a local in-process bus only.

### Required Bus Capabilities

- publish/subscribe
- request/reply where useful
- bounded queues
- observability hooks
- replay-friendly event recording if possible

### P2P Policy

- not part of MVP
- not part of Phase 1 implementation
- only revisit after single-process architecture is proven stable

## Configuration And State Management

This was missing in the original draft and is now required.

### Configuration Domains

- exchange credentials
- market universe settings
- cycle settings
- agent prompts and operating modes
- risk limits
- portfolio sizing rules
- TUI layout defaults

### State Domains

- portfolio and positions
- orders and fills
- cycle state
- research findings
- strategy evaluations
- audit logs

### Rules

- use explicit structured configuration files
- keep secrets separate from normal config
- use ZeroClaw security patterns where practical for secrets
- do not rely on transient agent memory for critical trading state

## Safety Model

Safety is not optional.

### Hard Safety Controls

- max position size
- max portfolio exposure
- per-asset concentration limit
- drawdown guard
- symbol allowlist/blocklist
- order rate limiting
- exchange connectivity health checks
- global kill switch

### Operational Modes

- backtest
- paper trading
- limited live trading
- broader live autonomy

Progress through these modes sequentially.

## Failure Modes

This must be designed before implementation goes too far.

### Failures To Handle

- agent crash
- invalid LLM response
- provider outage or rate limit
- exchange API failure
- stale market data
- order reject or partial fill
- bus backlog or message loss
- restart during open positions
- cycle transition during pending orders

### Minimum Failure Policy

- critical state must be persisted
- execution state must be recoverable after restart
- risk controls must remain functional without LLM calls
- the system must be able to enter a safe mode
- safe mode should at minimum stop new entries and allow controlled unwind

## Testing Strategy

This is mandatory for a trading system.

### Test Layers

#### Unit Tests

- domain math
- risk rules
- sizing logic
- message validation

#### Integration Tests

- event flow between agents
- adapter behavior against mocked exchange APIs
- persistence and restart recovery

#### Simulation And Backtest

- signal generation against historical data
- portfolio/risk/execution interaction

#### Paper Trading

- required before live trading
- should be an explicit milestone, not an afterthought

#### Replay Testing

- ability to feed recorded events back through the pipeline

## LLM Cost Model

LLM cost must be treated as a system resource.

### Cost Controls

- use cycle-based scanning instead of continuous full-market reasoning
- keep execution and hard risk off the LLM path
- summarize and compact research context aggressively
- prefer structured inputs over raw dumps
- cap agent invocation frequency

### MVP Policy

- only Research and Strategy should make regular LLM calls
- Portfolio and Risk should be mostly deterministic
- Execution should be deterministic on the critical path

## TUI Strategy

The TUI should start early as a development tool, not appear at the end.

### Minimal Phase 1 TUI

- log/events panel
- agent status panel
- watchlist panel
- position summary panel

### Later TUI Additions

- richer layout manager
- charts and performance views
- deeper inspection panels
- command palette and natural language interaction

## Development Plan

### Phase 0 - Planning Lock

- finalize architecture documents
- define success criteria for MVP
- define anti-scope for MVP

### Phase 1A - Core Skeleton

- create Cargo workspace
- create `bamboo-core`, `bamboo-runtime`, `bamboo-terminal`
- define domain models and message contracts
- define configuration models
- decide how `zeroclaw-copy/` maps into active runtime code

### Phase 1B - Runtime And Visibility

- implement local event bus
- build minimal ratatui shell
- wire basic runtime startup and logging
- prove one running agent loop with one synthetic workflow

### Phase 2 - Research And Strategy MVP

- implement Research Agent
- implement Strategy Agent
- implement cycle manager
- connect crypto market data and news ingestion
- define one or two strategy templates
- support historical replay and backtest requests

### Phase 3 - Portfolio And Risk MVP

- implement Portfolio Agent
- implement deterministic sizing and allocation
- implement deterministic Risk Agent checks
- add kill-switch and safe-mode behaviors
- validate end-to-end signal approval flow

### Phase 4 - Paper Execution MVP

- implement Execution Agent in paper mode
- record orders, fills, slippage, and audit trail
- feed execution reports back upstream
- run full paper-trading cycle

### Phase 5 - Limited Live Trading

- add one live crypto venue
- enable constrained live trading
- harden restart recovery and monitoring

### Phase 6 - Expansion

- revisit NautilusTrader deeper integration
- consider splitting more crates
- consider P2P only after single-node success
- add more venues and more advanced UI

## Immediate Next Build Targets

1. Create workspace skeleton under `bamboo-elf/`
2. Create `bamboo-core` with first-pass domain types:
   - `ResearchFinding`
   - `StrategySignal`
   - `PortfolioIntent`
   - `RiskDecision`
   - `ExecutionOrderIntent`
   - `ExecutionReport`
3. Define configuration structs and operating modes
4. Implement local event bus in `bamboo-runtime`
5. Build minimal `ratatui` shell
6. Build one synthetic end-to-end flow before any real exchange integration
7. Perform the repository-root migration from `zeroclaw-copy/` into `bamboo-elf/` before large-scale implementation begins

## Anti-Goals For Early Development

- do not build 8 crates first
- do not implement P2P first
- do not start with live trading first
- do not put execution-critical logic behind LLM calls
- do not over-model every future asset class before crypto works
- do not spend weeks pruning ZeroClaw before Bamboo Elf runs at all

## Important Design Constraints

- Strategy agent must not directly place orders
- Portfolio agent is mandatory
- broad market search should happen mainly at cycle boundaries
- open positions must remain monitored even outside focus rotation
- risk and execution safety must survive LLM failure
- paper trading must precede live trading
- the first MVP must remain operationally simple
