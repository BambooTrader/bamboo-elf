// Security policy: All network binds are loopback-only (127.0.0.1).
// No external inbound connections. Outbound WebSocket/REST to exchanges only.
// SQLite persistence is local-file-only.

mod app;
mod onboard;
mod ui;
mod widgets;

use std::io;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand};
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::CrosstermBackend;
use ratatui::Terminal;

use bamboo_core::{AppConfig, EventBus, TradingMode};
use bamboo_runtime::{LocalBus, ShutdownSignal};

use crate::app::App;
use crate::onboard::{expand_tilde, resolve_config_path, run_onboard, run_status};

// ── CLI definition ───────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "bamboo-elf", version, about = "AI-native quantitative trading terminal")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Config file path
    #[arg(short, long, default_value = "~/.bamboo-elf/config.toml")]
    config: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the trading terminal (default if no subcommand)
    Run {
        /// Config file path override
        #[arg(short, long)]
        config: Option<String>,
    },
    /// Interactive setup wizard — configure exchanges, risk, and strategy
    Onboard {
        /// Overwrite existing config
        #[arg(long)]
        force: bool,
    },
    /// Show current configuration and system status
    Status,
    /// Print version and build info
    Version,
}

// ── Entry point ──────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Onboard { force }) => {
            run_onboard(force)?;
        }
        Some(Commands::Status) => {
            let config_path = resolve_config_path(Some(&cli.config));
            run_status(&config_path);
        }
        Some(Commands::Version) => {
            println!(
                "bamboo-elf {} ({})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
            );
        }
        Some(Commands::Run { config }) => {
            let config_path = match config {
                Some(ref p) => expand_tilde(p),
                None => resolve_config_path(Some(&cli.config)),
            };
            start_terminal(&config_path)?;
        }
        None => {
            // Default: run the terminal
            let config_path = resolve_config_path(Some(&cli.config));
            start_terminal(&config_path)?;
        }
    }

    Ok(())
}

// ── Terminal startup ─────────────────────────────────────────────────────────

fn start_terminal(config_path: &str) -> Result<()> {
    // Initialize tracing (to stderr, not stdout — stdout is used by TUI)
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Loading config from: {}", config_path);

    // Load configuration
    let config = AppConfig::load(config_path)?;

    // Build the tokio runtime manually so we control shutdown
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async { run_app(config).await })?;

    Ok(())
}

async fn run_app(config: AppConfig) -> Result<()> {
    // Create bus and shutdown signal
    let bus = Arc::new(LocalBus::new());
    let shutdown = ShutdownSignal::new();

    // Subscribe to all bus messages for the TUI
    let mut bus_rx = bus.subscribe_all();

    // ── Data sources ────────────────────────────────────────────────────────
    // Keep synthetic feed and mock news as data sources (they simulate the exchange).
    let symbols = config.universe.default_symbols.clone();
    let tick_interval = Duration::from_millis(config.tui.tick_rate_ms.max(500));

    bamboo_runtime::mock_agents::spawn_synthetic_feed(
        bus.clone(),
        shutdown.clone(),
        symbols.clone(),
        tick_interval,
    );

    bamboo_runtime::mock_agents::spawn_mock_news_feed(bus.clone(), shutdown.clone());

    // ── Real agents (Spec 2) ────────────────────────────────────────────────

    // Cycle Manager
    {
        let b = bus.clone();
        let s = shutdown.clone();
        let c = config.cycle.clone();
        let max_focus = config.universe.max_focus_set;
        tokio::spawn(async move {
            bamboo_runtime::agents::run_cycle_manager(b, c, max_focus, s).await;
        });
    }

    // Research Agent
    {
        let b = bus.clone();
        let s = shutdown.clone();
        let rc = config.research.clone().unwrap_or_default();
        tokio::spawn(async move {
            bamboo_runtime::agents::run_research_agent(b, rc, s).await;
        });
    }

    // Strategy Agent
    {
        let b = bus.clone();
        let s = shutdown.clone();
        let sc = config.strategy.clone().unwrap_or_default();
        tokio::spawn(async move {
            bamboo_runtime::agents::run_strategy_agent(b, sc, s).await;
        });
    }

    // Portfolio Agent
    {
        let b = bus.clone();
        let s = shutdown.clone();
        let pc = config.portfolio.clone();
        tokio::spawn(async move {
            bamboo_runtime::agents::run_portfolio_agent(b, pc, s).await;
        });
    }

    // Risk Agent
    {
        let b = bus.clone();
        let s = shutdown.clone();
        let rc = config.risk.clone();
        let initial_equity = config.portfolio.initial_capital_usd.amount.as_f64();
        tokio::spawn(async move {
            bamboo_runtime::agents::run_risk_agent(b, rc, initial_equity, s).await;
        });
    }

    // ── Execution Agent (Spec 3) ──────────────────────────────────────────
    let execution_config = config.execution.clone().unwrap_or_default();
    let paper_config = config.paper.clone().unwrap_or_default();

    let trading_mode_str = match execution_config.mode {
        TradingMode::Paper => "PAPER",
        TradingMode::LiveConstrained => "LIVE",
        _ => "PAPER",
    };

    // Create venue adapter based on config
    let venue: Arc<dyn bamboo_core::VenueAdapter> = match execution_config.mode {
        TradingMode::Paper => {
            let paper = bamboo_runtime::venues::PaperVenue::new(paper_config);
            paper.start_price_listener(bus.clone());
            Arc::new(paper)
        }
        _ => {
            // For now, default to paper for all non-paper modes too
            let paper = bamboo_runtime::venues::PaperVenue::new(paper_config);
            paper.start_price_listener(bus.clone());
            Arc::new(paper)
        }
    };

    // Spawn execution agent
    {
        let b = bus.clone();
        let s = shutdown.clone();
        let ec = execution_config.clone();
        let v = venue.clone();
        tokio::spawn(async move {
            bamboo_runtime::agents::run_execution_agent(b, v, ec, s).await;
        });
    }

    // Create persistence store if configured
    if let Some(ref persistence_config) = config.persistence {
        if let Ok(store) = bamboo_runtime::persistence::StateStore::open(&persistence_config.db_path) {
            tracing::info!("State persistence enabled: {}", persistence_config.db_path);
            // Store is available for recovery — agents will use it via bus events
            drop(store);
        } else {
            tracing::warn!("Failed to open state store at {}", persistence_config.db_path);
        }
    }

    // ── Terminal setup ──────────────────────────────────────────────────────
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create App state
    let sparkline_window = config.tui.sparkline_window;
    let tick_rate = Duration::from_millis(config.tui.tick_rate_ms);
    let mut app = App::new(&symbols, sparkline_window, trading_mode_str.to_string());

    // Initialize portfolio summary from config
    app.init_portfolio(config.portfolio.initial_capital_usd.amount.as_f64());

    // Spawn a task to listen for SIGINT/SIGTERM
    let sig_shutdown = shutdown.clone();
    tokio::spawn(async move {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm = signal(SignalKind::terminate()).expect("failed to listen for SIGTERM");
            tokio::select! {
                _ = ctrl_c => {},
                _ = sigterm.recv() => {},
            }
        }
        #[cfg(not(unix))]
        {
            let _ = ctrl_c.await;
        }
        sig_shutdown.trigger();
    });

    // Main event loop
    let mut tick_interval = tokio::time::interval(tick_rate);
    tick_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        // Draw
        terminal.draw(|f| ui::render(f, &mut app))?;

        // Handle events via tokio::select!
        tokio::select! {
            // Bus messages
            msg_result = bus_rx.recv() => {
                match msg_result {
                    Ok(msg) => app.handle_bus_message(msg),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("TUI bus receiver lagged by {n} messages");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
            // Terminal events (keyboard, etc.)
            _ = poll_crossterm_event() => {
                if let Ok(true) = event::poll(Duration::ZERO) {
                    if let Ok(Event::Key(key)) = event::read() {
                        if key.kind == KeyEventKind::Press {
                            app.handle_key_event(key);
                        }
                    }
                }
            }
            // Tick for forced redraws
            _ = tick_interval.tick() => {}
        }

        if app.should_quit || shutdown.is_shutdown() {
            break;
        }
    }

    // Shutdown sequence
    shutdown.trigger();

    // Brief wait for tasks to clean up
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    tracing::info!("Bamboo Elf shut down cleanly.");

    Ok(())
}

/// Async-compatible polling for crossterm events.
/// Returns when an event might be available, or after a brief timeout.
async fn poll_crossterm_event() {
    // Use tokio::task::spawn_blocking to avoid blocking the async runtime
    let _ = tokio::task::spawn_blocking(|| {
        let _ = event::poll(Duration::from_millis(50));
    })
    .await;
}
