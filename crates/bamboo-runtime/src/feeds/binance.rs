use std::sync::Arc;
use std::time::Duration;

use bamboo_core::{
    BusMessage, ComponentId, EventBus, FeedStatus, InstrumentId, MarketTick, Payload, Price,
    Quantity, Topic,
};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use uuid::Uuid;

use crate::shutdown::ShutdownSignal;

/// Binance spot WebSocket market data feed.
///
/// Connects to the Binance miniTicker stream, parses ticks into `MarketTick`
/// messages, and publishes them to the event bus.
pub struct BinanceFeed {
    ws_url: String,
    instruments: Vec<InstrumentId>,
    bus: Arc<dyn EventBus>,
    shutdown: ShutdownSignal,
    status: Arc<std::sync::RwLock<FeedStatus>>,
}

impl BinanceFeed {
    pub fn new(
        ws_url: String,
        bus: Arc<dyn EventBus>,
        shutdown: ShutdownSignal,
    ) -> Self {
        Self {
            ws_url,
            instruments: Vec::new(),
            bus,
            shutdown,
            status: Arc::new(std::sync::RwLock::new(FeedStatus::Disconnected)),
        }
    }

    /// Return current feed status.
    pub fn status(&self) -> FeedStatus {
        *self.status.read().unwrap()
    }

    /// Add instruments to subscribe to. Must be called before `connect`.
    pub fn subscribe_instruments(&mut self, instruments: &[InstrumentId]) {
        self.instruments.extend_from_slice(instruments);
    }

    /// Connect to Binance and spawn a background task that reads from the
    /// WebSocket and publishes `MarketTick` messages to the bus.
    ///
    /// The task auto-reconnects with exponential backoff on disconnection.
    pub async fn connect(&self) -> anyhow::Result<tokio::task::JoinHandle<()>> {
        let bus = Arc::clone(&self.bus);
        let shutdown = self.shutdown.clone();
        let status = Arc::clone(&self.status);
        let instruments = self.instruments.clone();
        let base_url = self.ws_url.clone();

        let handle = tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);
            let max_backoff = Duration::from_secs(60);
            let mut consecutive_failures: u32 = 0;

            loop {
                if shutdown.is_shutdown() {
                    break;
                }

                let url = build_stream_url(&base_url, &instruments);
                tracing::info!(url = %url, "BinanceFeed connecting");

                match tokio_tungstenite::connect_async(&url).await {
                    Ok((ws_stream, _response)) => {
                        {
                            let mut s = status.write().unwrap();
                            *s = FeedStatus::Connected;
                        }
                        backoff = Duration::from_secs(1);
                        consecutive_failures = 0;
                        tracing::info!("BinanceFeed connected");

                        let (mut _write, mut read) = ws_stream.split();

                        loop {
                            if shutdown.is_shutdown() {
                                break;
                            }

                            tokio::select! {
                                msg = read.next() => {
                                    match msg {
                                        Some(Ok(WsMessage::Text(text))) => {
                                            if let Some(bus_msg) = parse_mini_ticker(&text) {
                                                let _ = bus.publish(bus_msg).await;
                                            }
                                        }
                                        Some(Ok(WsMessage::Ping(data))) => {
                                            let _ = _write.send(WsMessage::Pong(data)).await;
                                        }
                                        Some(Ok(WsMessage::Close(_))) => {
                                            tracing::warn!("BinanceFeed received close frame");
                                            break;
                                        }
                                        Some(Err(e)) => {
                                            tracing::error!(error = %e, "BinanceFeed WebSocket error");
                                            break;
                                        }
                                        None => {
                                            tracing::warn!("BinanceFeed stream ended");
                                            break;
                                        }
                                        _ => {} // Binary, Pong — ignore
                                    }
                                }
                                _ = shutdown.wait_for_shutdown() => {
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        consecutive_failures += 1;
                        tracing::error!(
                            error = %e,
                            failures = consecutive_failures,
                            "BinanceFeed connection failed"
                        );

                        if consecutive_failures >= 5 {
                            tracing::warn!(
                                "BinanceFeed: {} consecutive connection failures",
                                consecutive_failures
                            );
                        }
                    }
                }

                if shutdown.is_shutdown() {
                    break;
                }

                // Reconnect with backoff.
                {
                    let mut s = status.write().unwrap();
                    *s = FeedStatus::Reconnecting;
                }
                tracing::info!(
                    backoff_ms = backoff.as_millis(),
                    "BinanceFeed reconnecting after backoff"
                );

                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {}
                    _ = shutdown.wait_for_shutdown() => { break; }
                }

                backoff = (backoff * 2).min(max_backoff);
            }

            {
                let mut s = status.write().unwrap();
                *s = FeedStatus::Disconnected;
            }
            tracing::info!("BinanceFeed task exiting");
        });

        Ok(handle)
    }
}

/// Build a combined stream URL for Binance miniTicker streams.
/// E.g. `wss://stream.binance.com:9443/ws/btcusdt@miniTicker/ethusdt@miniTicker`
fn build_stream_url(base: &str, instruments: &[InstrumentId]) -> String {
    if instruments.is_empty() {
        return format!("{base}/btcusdt@miniTicker");
    }

    let streams: Vec<String> = instruments
        .iter()
        .map(|id| {
            let symbol = id.symbol().to_lowercase();
            format!("{symbol}@miniTicker")
        })
        .collect();

    format!("{base}/{}", streams.join("/"))
}

/// Parse a Binance miniTicker JSON message into a `BusMessage`.
///
/// Example payload:
/// ```json
/// {
///   "e": "24hrMiniTicker",
///   "s": "BTCUSDT",
///   "c": "68432.50",
///   "o": "67900.00",
///   "h": "68800.00",
///   "l": "67500.00",
///   "v": "12345.678",
///   "E": 1711234567890
/// }
/// ```
fn parse_mini_ticker(text: &str) -> Option<BusMessage> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;

    // Could be an array of tickers or a single ticker.
    let items = if v.is_array() {
        v.as_array()?.clone()
    } else {
        vec![v]
    };

    // For simplicity, process the first item. In production, iterate all.
    let item = items.first()?;

    let event_type = item.get("e")?.as_str()?;
    if event_type != "24hrMiniTicker" {
        return None;
    }

    let symbol = item.get("s")?.as_str()?;
    let close: f64 = item.get("c")?.as_str()?.parse().ok()?;
    let _open: f64 = item.get("o")?.as_str()?.parse().ok()?;
    let _high: f64 = item.get("h")?.as_str()?.parse().ok()?;
    let volume: f64 = item.get("v")?.as_str()?.parse().ok()?;
    let event_time = item.get("E")?.as_u64()?;

    let precision: u8 = if close > 1000.0 { 2 } else { 4 };

    // Use close as last, approximate bid/ask from close.
    let spread = close * 0.0001; // 1 basis point spread
    let tick = MarketTick {
        instrument_id: InstrumentId::from_parts(symbol, "BINANCE"),
        bid: Price::from_f64(close - spread, precision),
        ask: Price::from_f64(close + spread, precision),
        last: Price::from_f64(close, precision),
        volume_24h: Quantity::from_f64(volume, 4),
        timestamp: event_time * 1_000_000, // millis -> nanos
    };

    Some(BusMessage {
        id: Uuid::new_v4(),
        topic: Topic::MarketData,
        payload: Payload::MarketTick(tick),
        timestamp: event_time * 1_000_000,
        source: ComponentId::new("BinanceFeed"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_stream_url_with_instruments() {
        let instruments = vec![
            InstrumentId::from_parts("BTCUSDT", "BINANCE"),
            InstrumentId::from_parts("ETHUSDT", "BINANCE"),
        ];
        let url = build_stream_url("wss://stream.binance.com:9443/ws", &instruments);
        assert_eq!(
            url,
            "wss://stream.binance.com:9443/ws/btcusdt@miniTicker/ethusdt@miniTicker"
        );
    }

    #[test]
    fn parse_valid_mini_ticker() {
        let json = r#"{
            "e": "24hrMiniTicker",
            "E": 1711234567890,
            "s": "BTCUSDT",
            "c": "68432.50",
            "o": "67900.00",
            "h": "68800.00",
            "l": "67500.00",
            "v": "12345.678"
        }"#;
        let msg = parse_mini_ticker(json).expect("should parse");
        assert_eq!(msg.topic, Topic::MarketData);
        if let Payload::MarketTick(tick) = &msg.payload {
            assert_eq!(tick.instrument_id.symbol(), "BTCUSDT");
            assert!((tick.last.as_f64() - 68432.50).abs() < 1.0);
        } else {
            panic!("expected MarketTick payload");
        }
    }

    #[test]
    fn parse_invalid_json_returns_none() {
        assert!(parse_mini_ticker("not json").is_none());
    }

    #[test]
    fn parse_wrong_event_type_returns_none() {
        let json = r#"{"e": "trade", "s": "BTCUSDT"}"#;
        assert!(parse_mini_ticker(json).is_none());
    }
}
