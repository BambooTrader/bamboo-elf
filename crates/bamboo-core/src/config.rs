use serde::Deserialize;
use std::path::Path;

use crate::enums::TradingMode;
use crate::error::BambooResult;
use crate::types::{Currency, Money, Price};

/// Top-level application configuration, loaded from TOML.
#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub exchanges: Vec<ExchangeConfig>,
    pub universe: UniverseConfig,
    pub cycle: CycleConfig,
    pub risk: RiskLimitsConfig,
    pub portfolio: PortfolioConfig,
    pub tui: TuiConfig,
    pub research: Option<ResearchConfig>,
    pub strategy: Option<StrategyConfig>,
    pub execution: Option<ExecutionConfig>,
    pub paper: Option<PaperConfig>,
    pub persistence: Option<PersistenceConfig>,
}

impl AppConfig {
    /// Load configuration from a TOML file at the given path.
    pub fn load(path: impl AsRef<Path>) -> BambooResult<Self> {
        let contents = std::fs::read_to_string(path.as_ref()).map_err(|e| {
            crate::error::BambooError::Config(format!(
                "failed to read config file {}: {e}",
                path.as_ref().display()
            ))
        })?;
        let config: AppConfig = toml::from_str(&contents).map_err(|e| {
            crate::error::BambooError::Config(format!("failed to parse config: {e}"))
        })?;
        Ok(config)
    }
}

/// Exchange connection configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ExchangeConfig {
    pub name: String,
    pub ws_url: String,
    pub rest_url: String,
    pub api_key_env: String,
    pub api_secret_env: String,
}

/// Universe / instrument selection configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct UniverseConfig {
    pub default_symbols: Vec<String>,
    pub max_focus_set: usize,
}

/// Cycle timing configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct CycleConfig {
    pub default_duration_hours: u64,
    pub auto_advance: bool,
}

/// Risk limits configuration.
/// Money fields are deserialized from f64 in TOML and converted to fixed-point Money (USD, precision 2).
#[derive(Debug, Clone, Deserialize)]
pub struct RiskLimitsConfig {
    #[serde(deserialize_with = "deserialize_usd_money")]
    pub max_position_size_usd: Money,
    #[serde(deserialize_with = "deserialize_usd_money")]
    pub max_portfolio_exposure_usd: Money,
    pub max_concentration_pct: f64,
    pub max_drawdown_pct: f64,
    pub order_rate_limit_per_min: u32,
    pub kill_switch_enabled: bool,
}

/// Portfolio configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct PortfolioConfig {
    #[serde(deserialize_with = "deserialize_usd_money")]
    pub initial_capital_usd: Money,
    pub max_positions: usize,
    /// Risk percentage per trade (e.g. 1.0 means 1% of portfolio).
    #[serde(default = "default_risk_pct_per_trade")]
    pub risk_pct_per_trade: f64,
}

fn default_risk_pct_per_trade() -> f64 {
    1.0
}

/// Research agent configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ResearchConfig {
    pub min_volume_usd: f64,
    pub max_candidates: usize,
    pub scan_interval_secs: u64,
}

impl Default for ResearchConfig {
    fn default() -> Self {
        Self {
            min_volume_usd: 1_000_000.0,
            max_candidates: 10,
            scan_interval_secs: 300,
        }
    }
}

/// Strategy agent configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct StrategyConfig {
    pub enabled_strategies: Vec<String>,
    pub max_concurrent_signals: usize,
    pub momentum: MomentumParams,
    pub mean_reversion: MeanReversionParams,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        Self {
            enabled_strategies: vec!["momentum".to_string(), "mean_reversion".to_string()],
            max_concurrent_signals: 5,
            momentum: MomentumParams::default(),
            mean_reversion: MeanReversionParams::default(),
        }
    }
}

/// Momentum strategy parameters.
#[derive(Debug, Clone, Deserialize)]
pub struct MomentumParams {
    pub min_change_pct: f64,
    pub hold_hours: u64,
    pub stop_loss_pct: f64,
}

impl Default for MomentumParams {
    fn default() -> Self {
        Self {
            min_change_pct: 2.0,
            hold_hours: 24,
            stop_loss_pct: 3.0,
        }
    }
}

/// Mean reversion strategy parameters.
#[derive(Debug, Clone, Deserialize)]
pub struct MeanReversionParams {
    pub min_drop_pct: f64,
    pub target_recovery_pct: f64,
    pub hold_hours: u64,
    pub stop_loss_pct: f64,
}

impl Default for MeanReversionParams {
    fn default() -> Self {
        Self {
            min_drop_pct: 5.0,
            target_recovery_pct: 3.0,
            hold_hours: 48,
            stop_loss_pct: 5.0,
        }
    }
}

/// Execution agent configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ExecutionConfig {
    pub mode: TradingMode,
    pub max_open_orders: usize,
    pub order_timeout_secs: u64,
    pub retry_failed_orders: bool,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            mode: TradingMode::Paper,
            max_open_orders: 10,
            order_timeout_secs: 300,
            retry_failed_orders: false,
        }
    }
}

/// Paper trading simulation configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct PaperConfig {
    pub slippage_bps: u32,
    pub latency_ms: u64,
}

impl Default for PaperConfig {
    fn default() -> Self {
        Self {
            slippage_bps: 5,
            latency_ms: 100,
        }
    }
}

/// State persistence configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct PersistenceConfig {
    pub db_path: String,
    pub save_interval_secs: u64,
}

impl Default for PersistenceConfig {
    fn default() -> Self {
        Self {
            db_path: "./bamboo-elf.db".to_string(),
            save_interval_secs: 30,
        }
    }
}

/// TUI display configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct TuiConfig {
    pub tick_rate_ms: u64,
    pub sparkline_window: usize,
}

/// Serde helper: deserialize an f64 from TOML and convert it to Money with USD currency and precision 2.
fn deserialize_usd_money<'de, D>(deserializer: D) -> Result<Money, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = f64::deserialize(deserializer)?;
    let currency = Currency::usd();
    Ok(Money::new(Price::from_f64(value, currency.precision), currency))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_config() {
        let toml_str = r#"
[[exchanges]]
name = "binance"
ws_url = "wss://stream.binance.com:9443/ws"
rest_url = "https://api.binance.com"
api_key_env = "BINANCE_API_KEY"
api_secret_env = "BINANCE_API_SECRET"

[universe]
default_symbols = ["BTCUSDT", "ETHUSDT"]
max_focus_set = 10

[cycle]
default_duration_hours = 4
auto_advance = true

[risk]
max_position_size_usd = 10000.0
max_portfolio_exposure_usd = 50000.0
max_concentration_pct = 25
max_drawdown_pct = 10
order_rate_limit_per_min = 60
kill_switch_enabled = true

[portfolio]
initial_capital_usd = 100000.0
max_positions = 5

[tui]
tick_rate_ms = 250
sparkline_window = 120
"#;
        let config: AppConfig = toml::from_str(toml_str).expect("failed to parse config");
        assert_eq!(config.exchanges.len(), 1);
        assert_eq!(config.exchanges[0].name, "binance");
        assert_eq!(config.universe.default_symbols.len(), 2);
        assert!((config.risk.max_position_size_usd.amount.as_f64() - 10000.0).abs() < 1e-6);
        assert!((config.portfolio.initial_capital_usd.amount.as_f64() - 100000.0).abs() < 1e-6);
        assert_eq!(config.tui.tick_rate_ms, 250);
    }
}
