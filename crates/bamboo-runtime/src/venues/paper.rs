//! Paper Trading Venue — simulates order execution using real market data.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::Mutex;

use bamboo_core::{
    ClientOrderId, EventBus, ExecutionOrderIntent, InstrumentId, Money, OrderSide, OrderStatus,
    Payload, Price, Quantity, TradingMode, Topic, VenueAdapter, VenueError, VenueOrderId,
};
use bamboo_core::config::PaperConfig;

/// Record of a paper fill for audit trail.
#[derive(Debug, Clone)]
pub struct PaperFill {
    pub client_order_id: ClientOrderId,
    pub venue_order_id: VenueOrderId,
    pub instrument_id: InstrumentId,
    pub side: OrderSide,
    pub quantity: Quantity,
    pub fill_price: Price,
    pub slippage_applied: f64,
    pub simulated_commission: Money,
    pub timestamp: u64,
}

/// Paper trading venue implementation.
pub struct PaperVenue {
    config: PaperConfig,
    fills: Arc<Mutex<Vec<PaperFill>>>,
    prices: Arc<Mutex<HashMap<InstrumentId, Price>>>,
    next_order_id: Arc<Mutex<u64>>,
}

impl PaperVenue {
    pub fn new(config: PaperConfig) -> Self {
        Self {
            config,
            fills: Arc::new(Mutex::new(Vec::new())),
            prices: Arc::new(Mutex::new(HashMap::new())),
            next_order_id: Arc::new(Mutex::new(1)),
        }
    }

    /// Start a background task that listens to MarketData topic and updates prices.
    pub fn start_price_listener(&self, bus: Arc<dyn EventBus>) {
        let prices = self.prices.clone();
        tokio::spawn(async move {
            let mut rx = bus.subscribe(Topic::MarketData);
            loop {
                match rx.recv().await {
                    Ok(msg) => {
                        if let Payload::MarketTick(tick) = &msg.payload {
                            let mut map = prices.lock().await;
                            map.insert(tick.instrument_id.clone(), tick.last);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        });
    }

    /// Get all recorded paper fills.
    pub async fn fills(&self) -> Vec<PaperFill> {
        self.fills.lock().await.clone()
    }

    /// Set a price for testing purposes.
    pub async fn set_price(&self, instrument_id: InstrumentId, price: Price) {
        self.prices.lock().await.insert(instrument_id, price);
    }

    fn now_nanos() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
    }
}

#[async_trait]
impl VenueAdapter for PaperVenue {
    async fn submit_order(&self, order: &ExecutionOrderIntent) -> Result<VenueOrderId, VenueError> {
        // Simulate exchange latency.
        if self.config.latency_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.config.latency_ms)).await;
        }

        // Get current price.
        let last_price = {
            let map = self.prices.lock().await;
            map.get(&order.instrument_id).copied()
        };

        let base_price = match last_price {
            Some(p) => p,
            None => {
                // If no price available, use limit price or reject.
                match order.limit_price {
                    Some(p) => p,
                    None => {
                        return Err(VenueError::InvalidOrder(format!(
                            "No market price available for {} and no limit price set",
                            order.instrument_id
                        )));
                    }
                }
            }
        };

        // Apply slippage.
        let slippage_factor = self.config.slippage_bps as f64 / 10_000.0;
        let base_f64 = base_price.as_f64();
        let fill_price_f64 = match order.side {
            OrderSide::Buy => base_f64 * (1.0 + slippage_factor),
            OrderSide::Sell => base_f64 * (1.0 - slippage_factor),
        };
        let fill_price = Price::from_f64(fill_price_f64, base_price.precision);

        // Generate venue order ID.
        let order_num = {
            let mut id = self.next_order_id.lock().await;
            let n = *id;
            *id += 1;
            n
        };
        let venue_order_id = VenueOrderId::new(format!("PAPER-{order_num}"));

        // Simulate commission (0.1% taker fee).
        let commission_f64 = fill_price_f64 * order.quantity.as_f64() * 0.001;
        let commission = Money::from_f64(commission_f64, bamboo_core::Currency::usd());

        // Record the fill.
        let fill = PaperFill {
            client_order_id: order.client_order_id.clone(),
            venue_order_id: venue_order_id.clone(),
            instrument_id: order.instrument_id.clone(),
            side: order.side,
            quantity: order.quantity,
            fill_price,
            slippage_applied: slippage_factor,
            simulated_commission: commission,
            timestamp: Self::now_nanos(),
        };

        tracing::info!(
            instrument = %order.instrument_id,
            side = ?order.side,
            qty = %order.quantity,
            fill_price = %fill_price,
            venue_order_id = %venue_order_id,
            "Paper fill"
        );

        self.fills.lock().await.push(fill);

        Ok(venue_order_id)
    }

    async fn cancel_order(&self, _venue_order_id: &VenueOrderId) -> Result<(), VenueError> {
        // Paper venue fills instantly, so cancel always succeeds (nothing to cancel).
        Ok(())
    }

    async fn order_status(&self, _venue_order_id: &VenueOrderId) -> Result<OrderStatus, VenueError> {
        // Paper venue fills instantly for market orders.
        Ok(OrderStatus::Filled)
    }

    fn venue_name(&self) -> &str {
        "PaperVenue"
    }

    fn trading_mode(&self) -> TradingMode {
        TradingMode::Paper
    }

    async fn last_fill_price(&self, venue_order_id: &VenueOrderId) -> Option<Price> {
        let fills = self.fills.lock().await;
        fills
            .iter()
            .rev()
            .find(|f| &f.venue_order_id == venue_order_id)
            .map(|f| f.fill_price)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bamboo_core::{OrderType, TimeInForce, Venue};
    use uuid::Uuid;

    fn make_order(instrument: &str, side: OrderSide, qty: f64) -> ExecutionOrderIntent {
        ExecutionOrderIntent {
            id: Uuid::new_v4(),
            decision_id: Uuid::new_v4(),
            client_order_id: ClientOrderId::new("TEST-001"),
            instrument_id: InstrumentId::from_parts(instrument, "BINANCE"),
            venue: Venue::new("BINANCE"),
            side,
            order_type: OrderType::Market,
            quantity: Quantity::from_f64(qty, 8),
            limit_price: None,
            stop_price: None,
            time_in_force: TimeInForce::GTC,
            timestamp: 0,
        }
    }

    #[tokio::test]
    async fn paper_fill_buy_with_slippage() {
        let config = PaperConfig {
            slippage_bps: 10, // 0.1%
            latency_ms: 0,    // No delay in tests.
        };
        let venue = PaperVenue::new(config);

        // Set a price.
        let instrument = InstrumentId::from_parts("BTCUSDT", "BINANCE");
        venue.set_price(instrument, Price::from_f64(50_000.0, 2)).await;

        let order = make_order("BTCUSDT", OrderSide::Buy, 0.5);
        let result = venue.submit_order(&order).await;
        assert!(result.is_ok());

        let fills = venue.fills().await;
        assert_eq!(fills.len(), 1);

        let fill = &fills[0];
        // Buy slippage: 50000 * (1 + 0.001) = 50050.0
        let expected = 50_050.0;
        assert!(
            (fill.fill_price.as_f64() - expected).abs() < 0.01,
            "got {}, expected {}",
            fill.fill_price.as_f64(),
            expected
        );
    }

    #[tokio::test]
    async fn paper_fill_sell_with_slippage() {
        let config = PaperConfig {
            slippage_bps: 10,
            latency_ms: 0,
        };
        let venue = PaperVenue::new(config);

        let instrument = InstrumentId::from_parts("ETHUSDT", "BINANCE");
        venue.set_price(instrument, Price::from_f64(3_000.0, 2)).await;

        let order = make_order("ETHUSDT", OrderSide::Sell, 2.0);
        let result = venue.submit_order(&order).await;
        assert!(result.is_ok());

        let fills = venue.fills().await;
        assert_eq!(fills.len(), 1);

        let fill = &fills[0];
        // Sell slippage: 3000 * (1 - 0.001) = 2997.0
        let expected = 2_997.0;
        assert!(
            (fill.fill_price.as_f64() - expected).abs() < 0.01,
            "got {}, expected {}",
            fill.fill_price.as_f64(),
            expected
        );
    }

    #[tokio::test]
    async fn paper_rejects_no_price() {
        let config = PaperConfig {
            slippage_bps: 5,
            latency_ms: 0,
        };
        let venue = PaperVenue::new(config);

        // No price set for this instrument.
        let order = make_order("XYZUSDT", OrderSide::Buy, 1.0);
        let result = venue.submit_order(&order).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn paper_cancel_always_succeeds() {
        let config = PaperConfig::default();
        let venue = PaperVenue::new(config);
        let vid = VenueOrderId::new("PAPER-1");
        assert!(venue.cancel_order(&vid).await.is_ok());
    }

    #[tokio::test]
    async fn paper_order_status_is_filled() {
        let config = PaperConfig::default();
        let venue = PaperVenue::new(config);
        let vid = VenueOrderId::new("PAPER-1");
        let status = venue.order_status(&vid).await.unwrap();
        assert_eq!(status, OrderStatus::Filled);
    }
}
