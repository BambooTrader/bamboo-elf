//! Interactive setup wizard for bamboo-elf.
//!
//! Generates `~/.bamboo-elf/config.toml` by prompting the user through
//! exchange, universe, risk, portfolio, and execution-mode configuration.

use std::io::{self, BufRead, Write};
use std::path::PathBuf;

use anyhow::Result;

// ── Public entry point ───────────────────────────────────────────────────────

pub fn run_onboard(force: bool) -> Result<()> {
    println!("\n\u{1f38b} Bamboo Elf \u{2014} Setup Wizard\n");

    let config_dir = get_config_dir();
    let config_path = config_dir.join("config.toml");

    // Check existing config
    if config_path.exists() && !force {
        println!("Config already exists at {}", config_path.display());
        print!("Overwrite? [y/N] ");
        io::stdout().flush()?;
        if !confirm() {
            println!("Aborted.");
            return Ok(());
        }
    }

    // Ensure config directory exists
    std::fs::create_dir_all(&config_dir)?;

    // Step 1: Exchange
    println!("Step 1/5: Exchange Configuration");
    let exchange = prompt_exchange();

    // Step 2: Trading Universe
    println!("\nStep 2/5: Trading Universe");
    let symbols = prompt_symbols();

    // Step 3: Risk Parameters
    println!("\nStep 3/5: Risk Parameters");
    let risk = prompt_risk();

    // Step 4: Portfolio & Strategy
    println!("\nStep 4/5: Portfolio & Strategy");
    let portfolio = prompt_portfolio();

    // Step 5: Execution Mode
    println!("\nStep 5/5: Execution Mode");
    let mode = prompt_execution_mode();

    // Generate and write config
    let config_content = generate_config(&exchange, &symbols, &risk, &portfolio, &mode);
    std::fs::write(&config_path, &config_content)?;

    println!("\n\u{2705} Config written to {}", config_path.display());
    println!("Run `bamboo-elf` to start the terminal.");
    Ok(())
}

// ── Config directory resolution ──────────────────────────────────────────────

/// Returns the bamboo-elf config directory.
/// Checks `BAMBOO_ELF_CONFIG_DIR` first, falls back to `~/.bamboo-elf/`.
pub fn get_config_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("BAMBOO_ELF_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    home.join(".bamboo-elf")
}

/// Resolve the config file path using the standard search order:
/// 1. Explicit CLI `--config` path
/// 2. `$BAMBOO_ELF_CONFIG_DIR/config.toml`
/// 3. `~/.bamboo-elf/config.toml`
/// 4. `./config.toml` (dev fallback)
pub fn resolve_config_path(cli_config: Option<&str>) -> String {
    // 1. Explicit CLI path
    if let Some(path) = cli_config {
        return expand_tilde(path);
    }

    // 2. BAMBOO_ELF_CONFIG_DIR env var
    if let Ok(dir) = std::env::var("BAMBOO_ELF_CONFIG_DIR") {
        let p = PathBuf::from(&dir).join("config.toml");
        if p.exists() {
            return p.to_string_lossy().to_string();
        }
    }

    // 3. ~/.bamboo-elf/config.toml
    if let Ok(home) = std::env::var("HOME") {
        let p = PathBuf::from(&home).join(".bamboo-elf").join("config.toml");
        if p.exists() {
            return p.to_string_lossy().to_string();
        }
    }

    // 4. Dev fallback
    "./config.toml".to_string()
}

/// Expand a leading `~` to the user's home directory.
pub fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    path.to_string()
}

// ── Status subcommand ────────────────────────────────────────────────────────

pub fn run_status(config_path: &str) {
    println!("Bamboo Elf \u{2014} Status\n");

    let expanded = expand_tilde(config_path);
    let path = std::path::Path::new(&expanded);

    if !path.exists() {
        println!("  Config:  {} (NOT FOUND)", expanded);
        println!("\n  Run `bamboo-elf onboard` to create a config.");
        return;
    }

    println!("  Config:  {expanded}");

    // Try to parse and show key info
    match std::fs::read_to_string(path) {
        Ok(contents) => {
            if let Ok(config) = toml::from_str::<toml::Value>(&contents) {
                // Trading mode
                let mode = config
                    .get("execution")
                    .and_then(|e| e.get("mode"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("paper");
                println!("  Mode:    {mode}");

                // Symbols
                if let Some(syms) = config
                    .get("universe")
                    .and_then(|u| u.get("default_symbols"))
                    .and_then(|s| s.as_array())
                {
                    let names: Vec<&str> = syms.iter().filter_map(|v| v.as_str()).collect();
                    println!("  Symbols: {}", names.join(", "));
                }

                // DB path
                let db = config
                    .get("persistence")
                    .and_then(|p| p.get("db_path"))
                    .and_then(|d| d.as_str())
                    .unwrap_or("(not configured)");
                println!("  DB:      {db}");
            } else {
                println!("  (config exists but failed to parse)");
            }
        }
        Err(e) => println!("  (could not read config: {e})"),
    }
}

// ── Interactive prompts ──────────────────────────────────────────────────────

fn read_line() -> String {
    let stdin = io::stdin();
    let mut line = String::new();
    let _ = stdin.lock().read_line(&mut line);
    line.trim().to_string()
}

fn confirm() -> bool {
    let input = read_line();
    matches!(input.to_lowercase().as_str(), "y" | "yes")
}

fn prompt(label: &str, default: &str) -> String {
    print!("  {label} [{default}]: ");
    let _ = io::stdout().flush();
    let input = read_line();
    if input.is_empty() {
        default.to_string()
    } else {
        input
    }
}

struct ExchangeInfo {
    name: String,
    ws_url: String,
    rest_url: String,
    api_key_env: String,
    api_secret_env: String,
}

fn prompt_exchange() -> ExchangeInfo {
    let name = prompt("Exchange name", "binance");
    let (ws_url, rest_url, key_env, secret_env) = match name.as_str() {
        "binance" => (
            "wss://stream.binance.com:9443/ws".to_string(),
            "https://api.binance.com".to_string(),
            "BINANCE_API_KEY".to_string(),
            "BINANCE_API_SECRET".to_string(),
        ),
        other => {
            let ws = prompt("WebSocket URL", "wss://example.com/ws");
            let rest = prompt("REST URL", "https://example.com");
            let key = prompt(
                "API key env var",
                &format!("{}_API_KEY", other.to_uppercase()),
            );
            let secret = prompt(
                "API secret env var",
                &format!("{}_API_SECRET", other.to_uppercase()),
            );
            (ws, rest, key, secret)
        }
    };
    ExchangeInfo {
        name,
        ws_url,
        rest_url,
        api_key_env: key_env,
        api_secret_env: secret_env,
    }
}

fn prompt_symbols() -> Vec<String> {
    let raw = prompt("Symbols (comma-separated)", "BTCUSDT,ETHUSDT,SOLUSDT");
    raw.split(',').map(|s| s.trim().to_uppercase()).collect()
}

struct RiskInfo {
    max_position_size_usd: f64,
    max_portfolio_exposure_usd: f64,
    max_concentration_pct: f64,
    max_drawdown_pct: f64,
    order_rate_limit_per_min: u32,
}

fn prompt_risk() -> RiskInfo {
    let max_pos = prompt("Max position size (USD)", "10000")
        .parse::<f64>()
        .unwrap_or(10000.0);
    let max_exp = prompt("Max portfolio exposure (USD)", "50000")
        .parse::<f64>()
        .unwrap_or(50000.0);
    let max_conc = prompt("Max concentration (%)", "25")
        .parse::<f64>()
        .unwrap_or(25.0);
    let max_dd = prompt("Max drawdown (%)", "10")
        .parse::<f64>()
        .unwrap_or(10.0);
    let rate = prompt("Order rate limit (per min)", "10")
        .parse::<u32>()
        .unwrap_or(10);
    RiskInfo {
        max_position_size_usd: max_pos,
        max_portfolio_exposure_usd: max_exp,
        max_concentration_pct: max_conc,
        max_drawdown_pct: max_dd,
        order_rate_limit_per_min: rate,
    }
}

struct PortfolioInfo {
    initial_capital_usd: f64,
    max_positions: usize,
    risk_pct_per_trade: f64,
}

fn prompt_portfolio() -> PortfolioInfo {
    let cap = prompt("Initial capital (USD)", "100000")
        .parse::<f64>()
        .unwrap_or(100000.0);
    let max_pos = prompt("Max positions", "10")
        .parse::<usize>()
        .unwrap_or(10);
    let risk_pct = prompt("Risk per trade (%)", "1.0")
        .parse::<f64>()
        .unwrap_or(1.0);
    PortfolioInfo {
        initial_capital_usd: cap,
        max_positions: max_pos,
        risk_pct_per_trade: risk_pct,
    }
}

fn prompt_execution_mode() -> String {
    prompt("Execution mode (paper / live_constrained)", "paper")
}

// ── Config generation ────────────────────────────────────────────────────────

fn generate_config(
    exchange: &ExchangeInfo,
    symbols: &[String],
    risk: &RiskInfo,
    portfolio: &PortfolioInfo,
    mode: &str,
) -> String {
    let symbols_toml: Vec<String> = symbols.iter().map(|s| format!("\"{s}\"")).collect();

    format!(
        r#"# Bamboo Elf Configuration
# Generated by `bamboo-elf onboard`
# Install: cargo install --git https://github.com/bamboo-elf/bamboo-elf --bin bamboo-elf

[[exchanges]]
name = "{exchange_name}"
ws_url = "{ws_url}"
rest_url = "{rest_url}"
api_key_env = "{api_key_env}"
api_secret_env = "{api_secret_env}"

[universe]
default_symbols = [{symbols}]
max_focus_set = 10

[cycle]
default_duration_hours = 24
auto_advance = false

[risk]
max_position_size_usd = {max_pos:.1}
max_portfolio_exposure_usd = {max_exp:.1}
max_concentration_pct = {max_conc:.1}
max_drawdown_pct = {max_dd:.1}
order_rate_limit_per_min = {rate_limit}
kill_switch_enabled = true

[portfolio]
initial_capital_usd = {capital:.1}
max_positions = {max_positions}
risk_pct_per_trade = {risk_pct:.2}

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

[execution]
mode = "{mode}"
max_open_orders = 10
order_timeout_secs = 300
retry_failed_orders = false

[paper]
slippage_bps = 5
latency_ms = 100

[persistence]
db_path = "./bamboo-elf.db"
save_interval_secs = 30

[tui]
tick_rate_ms = 250
sparkline_window = 120
"#,
        exchange_name = exchange.name,
        ws_url = exchange.ws_url,
        rest_url = exchange.rest_url,
        api_key_env = exchange.api_key_env,
        api_secret_env = exchange.api_secret_env,
        symbols = symbols_toml.join(", "),
        max_pos = risk.max_position_size_usd,
        max_exp = risk.max_portfolio_exposure_usd,
        max_conc = risk.max_concentration_pct,
        max_dd = risk.max_drawdown_pct,
        rate_limit = risk.order_rate_limit_per_min,
        capital = portfolio.initial_capital_usd,
        max_positions = portfolio.max_positions,
        risk_pct = portfolio.risk_pct_per_trade,
        mode = mode,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_works() {
        // When HOME is set, ~ should expand
        if std::env::var("HOME").is_ok() {
            let expanded = expand_tilde("~/foo/bar.toml");
            assert!(!expanded.starts_with('~'));
            assert!(expanded.ends_with("/foo/bar.toml"));
        }
    }

    #[test]
    fn expand_tilde_no_tilde() {
        assert_eq!(expand_tilde("/etc/config.toml"), "/etc/config.toml");
    }

    #[test]
    fn resolve_config_explicit_path() {
        let path = resolve_config_path(Some("/tmp/test.toml"));
        assert_eq!(path, "/tmp/test.toml");
    }

    #[test]
    fn generate_config_is_valid_toml() {
        let exchange = ExchangeInfo {
            name: "binance".into(),
            ws_url: "wss://stream.binance.com:9443/ws".into(),
            rest_url: "https://api.binance.com".into(),
            api_key_env: "BINANCE_API_KEY".into(),
            api_secret_env: "BINANCE_API_SECRET".into(),
        };
        let symbols = vec!["BTCUSDT".into(), "ETHUSDT".into()];
        let risk = RiskInfo {
            max_position_size_usd: 10000.0,
            max_portfolio_exposure_usd: 50000.0,
            max_concentration_pct: 25.0,
            max_drawdown_pct: 10.0,
            order_rate_limit_per_min: 10,
        };
        let portfolio = PortfolioInfo {
            initial_capital_usd: 100000.0,
            max_positions: 10,
            risk_pct_per_trade: 1.0,
        };
        let config_str = generate_config(&exchange, &symbols, &risk, &portfolio, "paper");

        // Should parse as valid TOML
        let parsed: toml::Value = toml::from_str(&config_str).expect("generated config must be valid TOML");
        assert_eq!(
            parsed["exchanges"][0]["name"].as_str().unwrap(),
            "binance"
        );
        assert_eq!(parsed["execution"]["mode"].as_str().unwrap(), "paper");
    }
}
