use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! define_id {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }
    };
}

define_id!(InstrumentId, "Instrument identifier, format: \"{Symbol}.{Venue}\" e.g. \"BTCUSDT.BINANCE\"");
define_id!(Venue, "Trading venue identifier, e.g. \"BINANCE\"");
define_id!(StrategyId, "Strategy identifier");
define_id!(ClientOrderId, "Internal order ID");
define_id!(VenueOrderId, "Exchange-assigned order ID");
define_id!(PositionId, "Position identifier");
define_id!(TradeId, "Trade identifier");
define_id!(AccountId, "Account identifier");
define_id!(ComponentId, "Agent/component identifier");

impl InstrumentId {
    /// Create an InstrumentId from symbol and venue parts.
    /// e.g. `InstrumentId::from_parts("BTCUSDT", "BINANCE")` -> "BTCUSDT.BINANCE"
    pub fn from_parts(symbol: &str, venue: &str) -> Self {
        Self(format!("{symbol}.{venue}"))
    }

    /// Extract the symbol part (before the dot).
    pub fn symbol(&self) -> &str {
        self.0.split('.').next().unwrap_or(&self.0)
    }

    /// Extract the venue part (after the dot).
    pub fn venue(&self) -> &str {
        self.0.split('.').nth(1).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instrument_id_from_parts() {
        let id = InstrumentId::from_parts("BTCUSDT", "BINANCE");
        assert_eq!(id.to_string(), "BTCUSDT.BINANCE");
        assert_eq!(id.symbol(), "BTCUSDT");
        assert_eq!(id.venue(), "BINANCE");
    }
}
