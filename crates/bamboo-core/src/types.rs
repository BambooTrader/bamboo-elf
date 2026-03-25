use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, Sub};

/// Fixed scalar aligned with Nautilus: 10^9
pub const FIXED_SCALAR: i64 = 1_000_000_000;

/// Price with fixed-point precision, aligned with Nautilus FIXED_SCALAR = 10^9.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Price {
    pub raw: i64,
    pub precision: u8,
}

impl Price {
    /// Create a new Price from raw fixed-point value and precision.
    pub fn new(raw: i64, precision: u8) -> Self {
        Self { raw, precision }
    }

    /// Create a Price from an f64 value. The value is scaled by FIXED_SCALAR.
    pub fn from_f64(value: f64, precision: u8) -> Self {
        let raw = (value * FIXED_SCALAR as f64).round() as i64;
        Self { raw, precision }
    }

    /// Convert back to f64 for display purposes.
    pub fn as_f64(&self) -> f64 {
        self.raw as f64 / FIXED_SCALAR as f64
    }

    /// Zero price with the given precision.
    pub fn zero(precision: u8) -> Self {
        Self { raw: 0, precision }
    }
}

impl fmt::Display for Price {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.as_f64();
        write!(f, "{:.prec$}", value, prec = self.precision as usize)
    }
}

impl Add for Price {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            raw: self.raw + rhs.raw,
            precision: self.precision.max(rhs.precision),
        }
    }
}

impl Sub for Price {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            raw: self.raw - rhs.raw,
            precision: self.precision.max(rhs.precision),
        }
    }
}

impl From<f64> for Price {
    fn from(value: f64) -> Self {
        Price::from_f64(value, 2)
    }
}

/// Non-negative quantity with fixed-point precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Quantity {
    pub raw: u64,
    pub precision: u8,
}

impl Quantity {
    /// Create a new Quantity from raw fixed-point value and precision.
    pub fn new(raw: u64, precision: u8) -> Self {
        Self { raw, precision }
    }

    /// Create a Quantity from an f64 value. The value is scaled by FIXED_SCALAR.
    pub fn from_f64(value: f64, precision: u8) -> Self {
        let raw = (value * FIXED_SCALAR as f64).round() as u64;
        Self { raw, precision }
    }

    /// Convert back to f64 for display purposes.
    pub fn as_f64(&self) -> f64 {
        self.raw as f64 / FIXED_SCALAR as f64
    }

    /// Zero quantity with the given precision.
    pub fn zero(precision: u8) -> Self {
        Self { raw: 0, precision }
    }
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.as_f64();
        write!(f, "{:.prec$}", value, prec = self.precision as usize)
    }
}

impl Add for Quantity {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self {
            raw: self.raw + rhs.raw,
            precision: self.precision.max(rhs.precision),
        }
    }
}

impl Sub for Quantity {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            raw: self.raw - rhs.raw,
            precision: self.precision.max(rhs.precision),
        }
    }
}

impl From<f64> for Quantity {
    fn from(value: f64) -> Self {
        Quantity::from_f64(value, 2)
    }
}

/// Currency type distinction aligned with Nautilus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CurrencyType {
    Fiat,
    Crypto,
}

/// Currency identifier. Covers both fiat and crypto.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Currency {
    /// Currency code, e.g. "USD", "USDT", "BTC", "ETH"
    pub code: String,
    /// Decimal places (2 for USD, 8 for BTC)
    pub precision: u8,
    /// Fiat or Crypto
    pub currency_type: CurrencyType,
}

impl Currency {
    pub fn new(code: impl Into<String>, precision: u8, currency_type: CurrencyType) -> Self {
        Self {
            code: code.into(),
            precision,
            currency_type,
        }
    }

    /// USD convenience constructor.
    pub fn usd() -> Self {
        Self::new("USD", 2, CurrencyType::Fiat)
    }

    /// USDT convenience constructor.
    pub fn usdt() -> Self {
        Self::new("USDT", 2, CurrencyType::Crypto)
    }

    /// BTC convenience constructor.
    pub fn btc() -> Self {
        Self::new("BTC", 8, CurrencyType::Crypto)
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.code)
    }
}

/// Money = Price + Currency. Represents a monetary amount in a specific currency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Money {
    pub amount: Price,
    pub currency: Currency,
}

impl Money {
    pub fn new(amount: Price, currency: Currency) -> Self {
        Self { amount, currency }
    }

    /// Create Money from f64 with given currency.
    pub fn from_f64(value: f64, currency: Currency) -> Self {
        let precision = currency.precision;
        Self {
            amount: Price::from_f64(value, precision),
            currency,
        }
    }

    /// Zero money in the given currency.
    pub fn zero(currency: Currency) -> Self {
        let precision = currency.precision;
        Self {
            amount: Price::zero(precision),
            currency,
        }
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.amount, self.currency)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_from_f64_roundtrip() {
        let p = Price::from_f64(68432.50, 2);
        let v = p.as_f64();
        assert!((v - 68432.50).abs() < 1e-6, "got {v}");
    }

    #[test]
    fn price_add() {
        let a = Price::from_f64(100.0, 2);
        let b = Price::from_f64(50.25, 2);
        let c = a + b;
        assert!((c.as_f64() - 150.25).abs() < 1e-6);
    }

    #[test]
    fn price_sub() {
        let a = Price::from_f64(100.0, 2);
        let b = Price::from_f64(30.50, 2);
        let c = a - b;
        assert!((c.as_f64() - 69.50).abs() < 1e-6);
    }

    #[test]
    fn price_display() {
        let p = Price::from_f64(68432.5, 2);
        assert_eq!(format!("{p}"), "68432.50");
    }

    #[test]
    fn price_zero() {
        let p = Price::zero(4);
        assert_eq!(p.raw, 0);
        assert_eq!(format!("{p}"), "0.0000");
    }

    #[test]
    fn quantity_from_f64_roundtrip() {
        let q = Quantity::from_f64(1.5, 8);
        let v = q.as_f64();
        assert!((v - 1.5).abs() < 1e-9, "got {v}");
    }

    #[test]
    fn quantity_add() {
        let a = Quantity::from_f64(1.0, 8);
        let b = Quantity::from_f64(0.5, 8);
        let c = a + b;
        assert!((c.as_f64() - 1.5).abs() < 1e-9);
    }

    #[test]
    fn quantity_sub() {
        let a = Quantity::from_f64(2.0, 8);
        let b = Quantity::from_f64(0.3, 8);
        let c = a - b;
        assert!((c.as_f64() - 1.7).abs() < 1e-9);
    }

    #[test]
    fn money_display() {
        let m = Money::from_f64(1234.56, Currency::usd());
        assert_eq!(format!("{m}"), "1234.56 USD");
    }

    #[test]
    fn currency_constructors() {
        let usd = Currency::usd();
        assert_eq!(usd.code, "USD");
        assert_eq!(usd.precision, 2);
        assert_eq!(usd.currency_type, CurrencyType::Fiat);

        let btc = Currency::btc();
        assert_eq!(btc.code, "BTC");
        assert_eq!(btc.precision, 8);
        assert_eq!(btc.currency_type, CurrencyType::Crypto);
    }
}
