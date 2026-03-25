use serde::Deserialize;
use std::path::Path;

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
    pub max_concentration_pct: u8,
    pub max_drawdown_pct: u8,
    pub order_rate_limit_per_min: u32,
    pub kill_switch_enabled: bool,
}

/// Portfolio configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct PortfolioConfig {
    #[serde(deserialize_with = "deserialize_usd_money")]
    pub initial_capital_usd: Money,
    pub max_positions: usize,
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
