use serde::{Deserialize, Serialize};

// ── Order domain ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderType {
    Market,
    Limit,
    StopMarket,
    StopLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimeInForce {
    GTC,
    IOC,
    FOK,
    GTD,
    DAY,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OrderStatus {
    Initialized,
    Submitted,
    Accepted,
    Rejected,
    Canceled,
    Expired,
    Triggered,
    PendingUpdate,
    PendingCancel,
    PartiallyFilled,
    Filled,
}

impl OrderStatus {
    /// Returns true if the order is in a terminal state (no further transitions expected).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            OrderStatus::Rejected
                | OrderStatus::Canceled
                | OrderStatus::Expired
                | OrderStatus::Filled
        )
    }

    /// Returns true if the order is in an active (non-terminal) state.
    pub fn is_active(&self) -> bool {
        !self.is_terminal()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PositionSide {
    Flat,
    Long,
    Short,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LiquiditySide {
    Maker,
    Taker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AggressorSide {
    Buyer,
    Seller,
}

// ── Instrument domain ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AssetClass {
    Cryptocurrency,
    Equity,
    Commodity,
    Fx,
    Index,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InstrumentClass {
    Spot,
    Future,
    Perpetual,
    Option,
    Swap,
}

// ── Account domain ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AccountType {
    Cash,
    Margin,
}

// ── Market data ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BookAction {
    Add,
    Update,
    Delete,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PriceType {
    Bid,
    Ask,
    Mid,
    Last,
    Mark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BarInterval {
    Min1,
    Min5,
    Min15,
    Hour1,
    Hour4,
    Day1,
}

// ── System ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TradingMode {
    Backtest,
    Paper,
    LiveConstrained,
    LiveFull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CycleStage {
    Scan,
    Focus,
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EmergencyActionType {
    KillSwitch,
    ForceLiquidate,
    ReduceExposure,
    HaltNewOrders,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FeedStatus {
    Connected,
    Reconnecting,
    Disconnected,
}

// ── Agent intelligence ──

/// Runtime status of an agent. Contains String in Error variant so it is not Copy.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AgentRunStatus {
    Starting,
    Running,
    Idle,
    Error(String),
    Stopped,
}

/// Strategy type identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StrategyType {
    Momentum,
    MeanReversion,
}

/// Venue adapter errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum VenueError {
    #[error("order rejected: {0}")]
    OrderRejected(String),
    #[error("connection error: {0}")]
    ConnectionError(String),
    #[error("rate limit exceeded")]
    RateLimit,
    #[error("insufficient funds")]
    InsufficientFunds,
    #[error("invalid order: {0}")]
    InvalidOrder(String),
    #[error("unknown error: {0}")]
    Unknown(String),
}

/// Signal outcome status for tracking signal performance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SignalOutcomeStatus {
    Open,
    ProfitTarget,
    StopLoss,
    TimedOut,
    ForceClosed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_status_terminal() {
        assert!(OrderStatus::Filled.is_terminal());
        assert!(OrderStatus::Rejected.is_terminal());
        assert!(OrderStatus::Canceled.is_terminal());
        assert!(OrderStatus::Expired.is_terminal());
        assert!(!OrderStatus::Submitted.is_terminal());
        assert!(!OrderStatus::PartiallyFilled.is_terminal());
    }

    #[test]
    fn order_status_active() {
        assert!(OrderStatus::Initialized.is_active());
        assert!(OrderStatus::Submitted.is_active());
        assert!(OrderStatus::Accepted.is_active());
        assert!(!OrderStatus::Filled.is_active());
    }
}
