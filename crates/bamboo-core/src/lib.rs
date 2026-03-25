pub mod types;
pub mod identifiers;
pub mod enums;
pub mod messages;
pub mod bus;
pub mod error;
pub mod config;

// Re-export key types at crate root for convenience.
pub use types::{Price, Quantity, Money, Currency, CurrencyType, FIXED_SCALAR};
pub use identifiers::{
    InstrumentId, Venue, StrategyId, ClientOrderId, VenueOrderId,
    PositionId, TradeId, AccountId, ComponentId,
};
pub use enums::{
    OrderSide, OrderType, TimeInForce, OrderStatus, PositionSide,
    LiquiditySide, AggressorSide, AssetClass, InstrumentClass,
    AccountType, BookAction, PriceType, BarInterval, TradingMode,
    CycleStage, EmergencyActionType, FeedStatus,
    AgentRunStatus, StrategyType,
};
pub use messages::{
    MarketTick, KlineBar, NewsItem, ResearchFinding, StrategySignal,
    PortfolioIntent, RiskDecision, ExecutionOrderIntent, ExecutionReport,
    PositionUpdate, CycleSummary, EmergencyAction,
    CycleStageChanged, AgentHeartbeat,
};
pub use bus::{Topic, Payload, BusMessage, BusError, BusMetrics, BusReceiver, EventBus};
pub use config::{
    AppConfig, ExchangeConfig, UniverseConfig, CycleConfig,
    RiskLimitsConfig, PortfolioConfig, TuiConfig,
    ResearchConfig, StrategyConfig, MomentumParams, MeanReversionParams,
};
pub use error::{BambooError, BambooResult};
