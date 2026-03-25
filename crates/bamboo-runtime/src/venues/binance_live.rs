//! Binance Live Trading Venue — real exchange integration via REST API.

use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use bamboo_core::{
    ExecutionOrderIntent, OrderSide, OrderStatus, OrderType, TradingMode, TimeInForce,
    VenueAdapter, VenueError, VenueOrderId,
};

type HmacSha256 = Hmac<Sha256>;

/// Binance live trading venue.
pub struct BinanceLiveVenue {
    rest_client: reqwest::Client,
    base_url: String,
    api_key: String,
    api_secret: String,
}

impl BinanceLiveVenue {
    /// Create a new BinanceLiveVenue reading credentials from environment variables.
    pub fn new(base_url: &str, api_key_env: &str, api_secret_env: &str) -> Result<Self, VenueError> {
        let api_key = std::env::var(api_key_env).map_err(|_| {
            VenueError::ConnectionError(format!("Missing env var: {api_key_env}"))
        })?;
        let api_secret = std::env::var(api_secret_env).map_err(|_| {
            VenueError::ConnectionError(format!("Missing env var: {api_secret_env}"))
        })?;

        Ok(Self {
            rest_client: reqwest::Client::new(),
            base_url: base_url.to_string(),
            api_key,
            api_secret,
        })
    }

    /// Create from explicit credentials (for testing).
    pub fn with_credentials(base_url: &str, api_key: String, api_secret: String) -> Self {
        Self {
            rest_client: reqwest::Client::new(),
            base_url: base_url.to_string(),
            api_key,
            api_secret,
        }
    }

    fn timestamp_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn sign(&self, query_string: &str) -> String {
        let mut mac = HmacSha256::new_from_slice(self.api_secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(query_string.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    fn order_side_str(side: OrderSide) -> &'static str {
        match side {
            OrderSide::Buy => "BUY",
            OrderSide::Sell => "SELL",
        }
    }

    fn order_type_str(ot: OrderType) -> &'static str {
        match ot {
            OrderType::Market => "MARKET",
            OrderType::Limit => "LIMIT",
            OrderType::StopMarket => "STOP_LOSS",
            OrderType::StopLimit => "STOP_LOSS_LIMIT",
        }
    }

    fn time_in_force_str(tif: TimeInForce) -> &'static str {
        match tif {
            TimeInForce::GTC => "GTC",
            TimeInForce::IOC => "IOC",
            TimeInForce::FOK => "FOK",
            TimeInForce::GTD => "GTC", // Binance doesn't support GTD natively.
            TimeInForce::DAY => "GTC",
        }
    }
}

#[async_trait]
impl VenueAdapter for BinanceLiveVenue {
    async fn submit_order(&self, order: &ExecutionOrderIntent) -> Result<VenueOrderId, VenueError> {
        let symbol = order.instrument_id.symbol();
        let side = Self::order_side_str(order.side);
        let order_type = Self::order_type_str(order.order_type);
        let quantity = format!("{:.8}", order.quantity.as_f64());
        let timestamp = Self::timestamp_ms();

        let mut params = format!(
            "symbol={symbol}&side={side}&type={order_type}&quantity={quantity}&timestamp={timestamp}&newClientOrderId={}",
            order.client_order_id
        );

        // Add price for limit orders.
        if let Some(price) = order.limit_price {
            params.push_str(&format!("&price={:.8}&timeInForce={}", price.as_f64(), Self::time_in_force_str(order.time_in_force)));
        }

        // Add stop price if present.
        if let Some(stop) = order.stop_price {
            params.push_str(&format!("&stopPrice={:.8}", stop.as_f64()));
        }

        let signature = self.sign(&params);
        params.push_str(&format!("&signature={signature}"));

        let url = format!("{}/api/v3/order?{params}", self.base_url);

        tracing::info!(symbol, side, order_type, "Submitting live order to Binance");

        let resp = self
            .rest_client
            .post(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .map_err(|e| VenueError::ConnectionError(e.to_string()))?;

        if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
            || resp.status() == reqwest::StatusCode::from_u16(418).unwrap_or(reqwest::StatusCode::TOO_MANY_REQUESTS)
        {
            return Err(VenueError::RateLimit);
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| VenueError::Unknown(format!("Failed to parse response: {e}")))?;

        if let Some(code) = body.get("code") {
            let code = code.as_i64().unwrap_or(0);
            let msg = body
                .get("msg")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown")
                .to_string();

            return match code {
                -2010 => Err(VenueError::InsufficientFunds),
                -1013 | -1111 => Err(VenueError::InvalidOrder(msg)),
                _ => Err(VenueError::OrderRejected(msg)),
            };
        }

        let order_id = body
            .get("orderId")
            .and_then(|v| v.as_u64())
            .map(|id| VenueOrderId::new(id.to_string()))
            .ok_or_else(|| VenueError::Unknown("No orderId in response".to_string()))?;

        tracing::info!(venue_order_id = %order_id, "Binance order submitted");

        Ok(order_id)
    }

    async fn cancel_order(&self, venue_order_id: &VenueOrderId) -> Result<(), VenueError> {
        // NOTE: Binance cancel requires the symbol. For a full implementation,
        // we'd track the symbol per order. For now, this is a basic implementation.
        let timestamp = Self::timestamp_ms();
        let params = format!("orderId={}&timestamp={timestamp}", venue_order_id);
        let signature = self.sign(&params);
        let url = format!(
            "{}/api/v3/order?{params}&signature={signature}",
            self.base_url
        );

        let resp = self
            .rest_client
            .delete(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .map_err(|e| VenueError::ConnectionError(e.to_string()))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(VenueError::Unknown(format!("Cancel failed: {body}")));
        }

        Ok(())
    }

    async fn order_status(&self, venue_order_id: &VenueOrderId) -> Result<OrderStatus, VenueError> {
        let timestamp = Self::timestamp_ms();
        let params = format!("orderId={}&timestamp={timestamp}", venue_order_id);
        let signature = self.sign(&params);
        let url = format!(
            "{}/api/v3/order?{params}&signature={signature}",
            self.base_url
        );

        let resp = self
            .rest_client
            .get(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await
            .map_err(|e| VenueError::ConnectionError(e.to_string()))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| VenueError::Unknown(format!("Failed to parse response: {e}")))?;

        let status_str = body
            .get("status")
            .and_then(|s| s.as_str())
            .unwrap_or("UNKNOWN");

        let status = match status_str {
            "NEW" => OrderStatus::Accepted,
            "PARTIALLY_FILLED" => OrderStatus::PartiallyFilled,
            "FILLED" => OrderStatus::Filled,
            "CANCELED" => OrderStatus::Canceled,
            "REJECTED" => OrderStatus::Rejected,
            "EXPIRED" => OrderStatus::Expired,
            _ => OrderStatus::Submitted,
        };

        Ok(status)
    }

    fn venue_name(&self) -> &str {
        "BinanceLive"
    }

    fn trading_mode(&self) -> TradingMode {
        TradingMode::LiveConstrained
    }
}
