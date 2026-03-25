mod app;
mod ui;
mod widgets;

use std::io;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::CrosstermBackend;
use ratatui::Terminal;

use bamboo_core::{AppConfig, EventBus};
use bamboo_runtime::{LocalBus, ShutdownSignal};

use crate::app::App;

fn main() -> Result<()> {
    // Parse CLI args
    let config_path = parse_config_arg();

    // Initialize tracing (to file, not stdout, since stdout is used by TUI)
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Loading config from: {}", config_path);

    // Load configuration
    let config = AppConfig::load(&config_path)?;

    // Build the tokio runtime manually so we control shutdown
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async { run_app(config).await })?;

    Ok(())
}

fn parse_config_arg() -> String {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--config" {
            if i + 1 < args.len() {
                return args[i + 1].clone();
            }
        }
        i += 1;
    }
    "./config.toml".to_string()
}

async fn run_app(config: AppConfig) -> Result<()> {
    // Create bus and shutdown signal
    let bus = Arc::new(LocalBus::new());
    let shutdown = ShutdownSignal::new();

    // Subscribe to all bus messages for the TUI
    let mut bus_rx = bus.subscribe_all();

    // Spawn mock agents
    let symbols = config.universe.default_symbols.clone();
    let tick_interval = Duration::from_millis(config.tui.tick_rate_ms.max(500));

    bamboo_runtime::mock_agents::spawn_synthetic_feed(
        bus.clone(),
        shutdown.clone(),
        symbols.clone(),
        tick_interval,
    );

    bamboo_runtime::mock_agents::spawn_mock_news_feed(bus.clone(), shutdown.clone());

    // Spawn the full mock agent pipeline: Research -> Strategy -> Portfolio -> Risk -> Execution
    {
        let b = bus.clone();
        let s = shutdown.clone();
        tokio::spawn(async move { bamboo_runtime::mock_agents::mock_research(b, s).await });
    }
    {
        let b = bus.clone();
        let s = shutdown.clone();
        tokio::spawn(async move { bamboo_runtime::mock_agents::mock_strategy(b, s).await });
    }
    {
        let b = bus.clone();
        let s = shutdown.clone();
        tokio::spawn(async move { bamboo_runtime::mock_agents::mock_portfolio(b, s).await });
    }
    {
        let b = bus.clone();
        let s = shutdown.clone();
        tokio::spawn(async move { bamboo_runtime::mock_agents::mock_risk(b, s).await });
    }
    {
        let b = bus.clone();
        let s = shutdown.clone();
        tokio::spawn(async move { bamboo_runtime::mock_agents::mock_execution(b, s).await });
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create App state
    let sparkline_window = config.tui.sparkline_window;
    let tick_rate = Duration::from_millis(config.tui.tick_rate_ms);
    let mut app = App::new(&symbols, sparkline_window);

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
