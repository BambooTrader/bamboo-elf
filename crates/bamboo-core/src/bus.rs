use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::identifiers::ComponentId;
use crate::messages::{
    AgentHeartbeat, CycleStageChanged, CycleSummary, EmergencyAction, ExecutionOrderIntent,
    ExecutionReport, KlineBar, MarketTick, NewsItem, PortfolioIntent, PositionUpdate,
    ResearchFinding, RiskDecision, StrategySignal,
};

/// Bus topic for message routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Topic {
    MarketData,
    News,
    Signal,
    Intent,
    Risk,
    Execution,
    System,
}

/// Payload wrapping all message types.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Payload {
    MarketTick(MarketTick),
    KlineBar(KlineBar),
    NewsItem(NewsItem),
    ResearchFinding(ResearchFinding),
    StrategySignal(StrategySignal),
    PortfolioIntent(PortfolioIntent),
    RiskDecision(RiskDecision),
    ExecutionOrderIntent(ExecutionOrderIntent),
    ExecutionReport(ExecutionReport),
    PositionUpdate(PositionUpdate),
    CycleSummary(CycleSummary),
    EmergencyAction(EmergencyAction),
    CycleStageChanged(CycleStageChanged),
    AgentHeartbeat(AgentHeartbeat),
}

/// Envelope for all bus messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusMessage {
    pub id: Uuid,
    pub topic: Topic,
    pub payload: Payload,
    /// Unix nanoseconds timestamp.
    pub timestamp: u64,
    pub source: ComponentId,
}

/// Errors that can occur on the event bus.
#[derive(Debug, Clone, thiserror::Error)]
pub enum BusError {
    #[error("no subscribers for topic")]
    NoSubscribers,
    #[error("channel closed")]
    ChannelClosed,
}

/// Metrics snapshot from the event bus.
#[derive(Debug, Clone, Default)]
pub struct BusMetrics {
    pub messages_published: u64,
    pub messages_per_topic: HashMap<Topic, u64>,
    pub queue_depth: HashMap<Topic, usize>,
}

/// Receiver type alias for bus subscriptions.
pub type BusReceiver = tokio::sync::broadcast::Receiver<BusMessage>;

/// The core event bus trait. Implementations live in bamboo-runtime.
#[async_trait::async_trait]
pub trait EventBus: Send + Sync {
    /// Publish a message. Returns the number of active subscribers that received it.
    async fn publish(&self, msg: BusMessage) -> Result<usize, BusError>;

    /// Subscribe to a specific topic.
    fn subscribe(&self, topic: Topic) -> BusReceiver;

    /// Subscribe to all topics.
    fn subscribe_all(&self) -> BusReceiver;

    /// Get current bus metrics.
    fn metrics(&self) -> BusMetrics;
}
