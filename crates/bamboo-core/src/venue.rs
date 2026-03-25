//! Venue adapter trait — abstracts exchange interface for paper and live trading.

use async_trait::async_trait;

use crate::enums::{OrderStatus, TradingMode, VenueError};
use crate::identifiers::VenueOrderId;
use crate::messages::ExecutionOrderIntent;

/// Abstract venue adapter. Implementations live in bamboo-runtime.
#[async_trait]
pub trait VenueAdapter: Send + Sync {
    /// Submit a new order to the venue.
    async fn submit_order(&self, order: &ExecutionOrderIntent) -> Result<VenueOrderId, VenueError>;

    /// Cancel an existing order.
    async fn cancel_order(&self, venue_order_id: &VenueOrderId) -> Result<(), VenueError>;

    /// Get current order status.
    async fn order_status(&self, venue_order_id: &VenueOrderId) -> Result<OrderStatus, VenueError>;

    /// Get venue name.
    fn venue_name(&self) -> &str;

    /// Get trading mode.
    fn trading_mode(&self) -> TradingMode;
}
