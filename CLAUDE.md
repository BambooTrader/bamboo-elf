# CLAUDE.md — Bamboo Elf

## Commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace
cargo test --workspace
cargo run --bin bamboo-elf
```

## Project Snapshot

Bamboo Elf is an AI-native quantitative trading terminal built in Rust. It combines a real-time TUI with a cycle-based agent pipeline for crypto trading.

## Repository Map

- `crates/bamboo-core/` — Domain types, messages, EventBus trait, config
- `crates/bamboo-runtime/` — LocalBus, agents, venues, feeds, persistence
- `crates/bamboo-terminal/` — TUI, CLI (clap), onboard wizard
- `config.example.toml` — Example config
- `docs/superpowers/specs/` — Design specs

## Key Extension Points

- `bamboo-core/src/bus.rs` — `EventBus` trait
- `bamboo-core/src/venue.rs` — `VenueAdapter` trait
- `bamboo-runtime/src/agents/` — Agent implementations
- `bamboo-runtime/src/venues/` — Venue adapters (paper, binance)
- `bamboo-runtime/src/feeds/` — Market data feeds

## Architecture

Agent pipeline: MarketData → Research → Strategy → Portfolio → Risk → Execution

All communication through EventBus topics. All network is outbound-only, loopback-safe.

## Workflow

1. Read before write — inspect existing code first
2. Run tests after changes: `cargo test --workspace`
3. Run clippy: `cargo clippy --workspace`
4. No secrets in code — API keys via environment variables
