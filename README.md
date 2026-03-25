# Bamboo Elf

AI-native quantitative trading terminal. Real-time TUI with cycle-based trading, deterministic risk controls, and paper/live execution.

## Install

```bash
cargo install --git https://github.com/BambooTrader/bamboo-elf --bin bamboo-elf
```

Or build from source:

```bash
git clone https://github.com/BambooTrader/bamboo-elf.git
cd bamboo-elf
cargo install --path crates/bamboo-terminal
```

## Quick Start

```bash
# Interactive setup — generates ~/.bamboo-elf/config.toml
bamboo-elf onboard

# Launch the terminal
bamboo-elf
```

## Commands

```
bamboo-elf              Start the trading terminal (default)
bamboo-elf run          Same as above, with --config override
bamboo-elf onboard      Interactive 5-step setup wizard
bamboo-elf status       Show config and system status
bamboo-elf version      Print version info
bamboo-elf --help       Full help
```

## Architecture

```
bamboo-core        Domain types, messages, EventBus trait, config
bamboo-runtime     LocalBus, agents, venues, feeds, persistence
bamboo-terminal    TUI, CLI, onboard wizard
```

### Agent Pipeline

```
MarketData ──> Research ──> Strategy ──> Portfolio ──> Risk ──> Execution
                  |                                              |
                  <───────────── Feedback Loop ─────────────────<
```

### Trading Cycle

```
Scan ──> Focus ──> Review ──> (repeat)
```

- **Scan**: Research agent scans market, ranks assets by volume/momentum/volatility
- **Focus**: Strategy evaluates top picks, generates signals for the focus set
- **Review**: Cycle summary, P&L review, prepare for next cycle

### Agents

| Agent | Role |
|-------|------|
| CycleManager | Drives Scan/Focus/Review lifecycle |
| Research | Broad market scanning, asset ranking |
| Strategy | Momentum + mean-reversion signal generation |
| Portfolio | Risk-based position sizing, capital allocation |
| Risk | Hard limits: exposure, concentration, drawdown, rate limiting |
| Execution | Order lifecycle, venue dispatch, paper/live trading |

## TUI

4-tab layout with keyboard navigation:

| Tab | Content |
|-----|---------|
| Market | Watchlist + sparklines, news feed, positions, agent status, event log |
| Portfolio | Capital summary, detailed positions with P&L |
| Agents | Agent status, last actions, message counts |
| Logs | Full event log with topic-based color coding |

**Keys**: `1-4` switch tabs, `j/k` scroll, `Tab` cycle tabs, `q` quit

## Configuration

Config lives at `~/.bamboo-elf/config.toml`. Generate it with `bamboo-elf onboard` or copy from `config.example.toml`.

Key sections:

```toml
[[exchanges]]         # Exchange connections (Binance WS/REST)
[universe]            # Trading universe (symbols, focus set size)
[cycle]               # Cycle duration and auto-advance
[risk]                # Hard risk limits
[portfolio]           # Capital and position limits
[research]            # Scan parameters
[strategy]            # Strategy templates and params
[execution]           # Paper or live mode
[persistence]         # SQLite state storage
[tui]                 # UI tick rate and sparkline window
```

## Security

- All network activity is outbound only (to exchange APIs)
- No inbound server ports — loopback only
- API keys read from environment variables, never stored in config
- SQLite persistence is local-file only
- Paper mode is the default — live trading requires explicit config

## Development

```bash
cargo test --workspace       # Run all tests
cargo clippy --workspace     # Lint
cargo run --bin bamboo-elf   # Run from source
```

## License

MIT OR Apache-2.0
