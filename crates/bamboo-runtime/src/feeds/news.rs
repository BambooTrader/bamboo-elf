use std::sync::Arc;
use std::time::Duration;

use bamboo_core::{BusMessage, ComponentId, EventBus, InstrumentId, NewsItem, Payload, Topic};
use uuid::Uuid;

use crate::shutdown::ShutdownSignal;

/// CryptoCompare news API URL.
const CRYPTOCOMPARE_NEWS_URL: &str = "https://min-api.cryptocompare.com/data/v2/news/?lang=EN";

/// Polling interval for news (5 minutes).
const POLL_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// HTTP-polling news feed that fetches headlines from CryptoCompare
/// and publishes `NewsItem` messages to the event bus.
pub struct NewsFeed {
    bus: Arc<dyn EventBus>,
    shutdown: ShutdownSignal,
    api_url: String,
}

impl NewsFeed {
    pub fn new(bus: Arc<dyn EventBus>, shutdown: ShutdownSignal) -> Self {
        Self {
            bus,
            shutdown,
            api_url: CRYPTOCOMPARE_NEWS_URL.to_string(),
        }
    }

    /// Override the API URL (useful for testing).
    pub fn with_url(mut self, url: String) -> Self {
        self.api_url = url;
        self
    }

    /// Start the polling loop as a background tokio task.
    pub fn start(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default();

            // Initial short delay so the system can start up.
            tokio::time::sleep(Duration::from_secs(5)).await;

            loop {
                if self.shutdown.is_shutdown() {
                    break;
                }

                match self.fetch_news(&client).await {
                    Ok(items) => {
                        tracing::info!(count = items.len(), "NewsFeed fetched headlines");
                        for item in items {
                            let msg = BusMessage {
                                id: Uuid::new_v4(),
                                topic: Topic::News,
                                payload: Payload::NewsItem(item),
                                timestamp: now_nanos(),
                                source: ComponentId::new("NewsFeed"),
                            };
                            let _ = self.bus.publish(msg).await;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "NewsFeed fetch failed");
                    }
                }

                // Sleep for the poll interval, but check shutdown periodically.
                tokio::select! {
                    _ = tokio::time::sleep(POLL_INTERVAL) => {}
                    _ = self.shutdown.wait_for_shutdown() => { break; }
                }
            }

            tracing::info!("NewsFeed task exiting");
        })
    }

    async fn fetch_news(&self, client: &reqwest::Client) -> anyhow::Result<Vec<NewsItem>> {
        let resp = client.get(&self.api_url).send().await?;
        let body: serde_json::Value = resp.json().await?;

        let mut items = Vec::new();
        if let Some(data) = body.get("Data").and_then(|d| d.as_array()) {
            for entry in data.iter().take(10) {
                let title = entry
                    .get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string();
                let source = entry
                    .get("source")
                    .and_then(|s| s.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let url = entry
                    .get("url")
                    .and_then(|u| u.as_str())
                    .map(|s| s.to_string());
                let published_on = entry
                    .get("published_on")
                    .and_then(|p| p.as_u64())
                    .unwrap_or(0);

                // Try to extract related categories/tags for instrument association.
                let categories = entry
                    .get("categories")
                    .and_then(|c| c.as_str())
                    .unwrap_or("");
                let related = extract_related_instruments(categories);

                if !title.is_empty() {
                    items.push(NewsItem {
                        title,
                        source,
                        url,
                        related_instruments: related,
                        timestamp: published_on * 1_000_000_000, // seconds -> nanos
                    });
                }
            }
        }

        Ok(items)
    }
}

/// Best-effort extraction of instrument IDs from CryptoCompare category tags.
fn extract_related_instruments(categories: &str) -> Vec<InstrumentId> {
    let mut instruments = Vec::new();
    let lower = categories.to_uppercase();

    for token in ["BTC", "ETH", "SOL", "BNB", "XRP", "ADA", "DOGE", "AVAX"] {
        if lower.contains(token) {
            instruments.push(InstrumentId::from_parts(
                &format!("{token}USDT"),
                "BINANCE",
            ));
        }
    }

    instruments
}

fn now_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}
