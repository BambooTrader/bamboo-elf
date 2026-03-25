//! Research Agent — broad market scanning during Scan, continuous monitoring during Focus.

use std::sync::Arc;
use std::time::Duration;

use bamboo_core::{
    AgentHeartbeat, AgentRunStatus, BusMessage, ComponentId, CycleStage, EventBus, InstrumentId,
    OrderSide, Payload, ResearchConfig, ResearchFinding, Topic,
};
use uuid::Uuid;

use crate::shutdown::ShutdownSignal;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn now_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

fn bus_msg(topic: Topic, payload: Payload, source: &str) -> BusMessage {
    BusMessage {
        id: Uuid::new_v4(),
        topic,
        payload,
        timestamp: now_nanos(),
        source: ComponentId::new(source),
    }
}

async fn publish_heartbeat(bus: &Arc<dyn EventBus>, status: AgentRunStatus, action: Option<String>) {
    let hb = AgentHeartbeat {
        agent_name: "ResearchAgent".to_string(),
        status,
        last_action: action,
        timestamp: now_nanos(),
    };
    let msg = bus_msg(Topic::System, Payload::AgentHeartbeat(hb), "ResearchAgent");
    let _ = bus.publish(msg).await;
}

/// Binance 24hr ticker data (subset of fields we care about).
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
struct BinanceTicker {
    symbol: String,
    price_change_percent: String,
    volume: String,
    #[serde(rename = "quoteVolume")]
    quote_volume: String,
    high_price: String,
    low_price: String,
    last_price: String,
}

/// Scored ticker candidate for ranking.
#[derive(Debug)]
#[allow(dead_code)]
struct ScoredTicker {
    symbol: String,
    volume_usd: f64,
    change_pct: f64,
    volatility: f64,
    last_price: f64,
    score: f64,
}

/// Fetch Binance 24hr tickers, score and rank, return top N findings.
async fn scan_binance(
    http: &reqwest::Client,
    config: &ResearchConfig,
) -> Vec<ResearchFinding> {
    let url = "https://api.binance.com/api/v3/ticker/24hr";
    let resp = match http.get(url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Binance ticker fetch failed: {e}");
            return Vec::new();
        }
    };

    let tickers: Vec<BinanceTicker> = match resp.json().await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("Binance ticker parse failed: {e}");
            return Vec::new();
        }
    };

    // Filter USDT pairs only and parse.
    let mut scored: Vec<ScoredTicker> = tickers
        .into_iter()
        .filter(|t| t.symbol.ends_with("USDT"))
        .filter_map(|t| {
            let volume_usd = t.quote_volume.parse::<f64>().ok()?;
            if volume_usd < config.min_volume_usd {
                return None;
            }
            let change_pct = t.price_change_percent.parse::<f64>().ok()?;
            let high = t.high_price.parse::<f64>().ok()?;
            let low = t.low_price.parse::<f64>().ok()?;
            let last_price = t.last_price.parse::<f64>().ok()?;
            let volatility = if last_price > 0.0 {
                (high - low) / last_price * 100.0
            } else {
                0.0
            };
            Some(ScoredTicker {
                symbol: t.symbol,
                volume_usd,
                change_pct,
                volatility,
                last_price,
                score: 0.0,
            })
        })
        .collect();

    if scored.is_empty() {
        return Vec::new();
    }

    // Rank-based scoring: score = volume_rank * 0.4 + change_rank * 0.3 + volatility_rank * 0.3
    let n = scored.len() as f64;

    // Sort by volume and assign ranks.
    scored.sort_by(|a, b| a.volume_usd.partial_cmp(&b.volume_usd).unwrap_or(std::cmp::Ordering::Equal));
    for (i, t) in scored.iter_mut().enumerate() {
        t.score += (i as f64 / n) * 0.4;
    }

    // Sort by abs change and assign ranks.
    scored.sort_by(|a, b| {
        a.change_pct
            .abs()
            .partial_cmp(&b.change_pct.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (i, t) in scored.iter_mut().enumerate() {
        t.score += (i as f64 / n) * 0.3;
    }

    // Sort by volatility and assign ranks.
    scored.sort_by(|a, b| a.volatility.partial_cmp(&b.volatility).unwrap_or(std::cmp::Ordering::Equal));
    for (i, t) in scored.iter_mut().enumerate() {
        t.score += (i as f64 / n) * 0.3;
    }

    // Sort by final score descending, take top N.
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(config.max_candidates);

    scored
        .into_iter()
        .map(|t| {
            let side = if t.change_pct > 0.0 {
                Some(OrderSide::Buy)
            } else {
                Some(OrderSide::Sell)
            };
            ResearchFinding {
                id: Uuid::new_v4(),
                instrument_id: InstrumentId::from_parts(&t.symbol, "BINANCE"),
                thesis: format!(
                    "24h change {:.2}%, vol ${:.0}, volatility {:.2}%",
                    t.change_pct, t.volume_usd, t.volatility,
                ),
                score: t.score,
                recommended_action: side,
                timestamp: now_nanos(),
            }
        })
        .collect()
}

/// Run the research agent as an async task.
pub async fn run_research_agent(
    bus: Arc<dyn EventBus>,
    config: ResearchConfig,
    shutdown: ShutdownSignal,
) {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap_or_default();

    let mut system_rx = bus.subscribe(Topic::System);
    let mut market_rx = bus.subscribe(Topic::MarketData);
    let mut current_stage = CycleStage::Scan;
    let mut focus_set: Vec<InstrumentId> = Vec::new();

    tracing::info!("ResearchAgent started");
    publish_heartbeat(&bus, AgentRunStatus::Running, Some("Started".to_string())).await;

    let scan_interval = Duration::from_secs(config.scan_interval_secs);
    let mut heartbeat_interval = tokio::time::interval(Duration::from_secs(10));
    // scan_tick fires every second; actual scan is gated by scan_interval elapsed check.
    // Using an interval (not sleep) ensures it fires even when other events are frequent.
    let mut scan_tick = tokio::time::interval(Duration::from_secs(1));
    let mut last_scan = tokio::time::Instant::now() - scan_interval; // allow immediate first scan

    loop {
        if shutdown.is_shutdown() {
            break;
        }

        tokio::select! {
            _ = shutdown.wait_for_shutdown() => { break; }
            _ = heartbeat_interval.tick() => {
                publish_heartbeat(
                    &bus,
                    AgentRunStatus::Running,
                    Some(format!("Stage: {:?}", current_stage)),
                ).await;
            }
            result = system_rx.recv() => {
                match result {
                    Ok(msg) => {
                        if let Payload::CycleStageChanged(changed) = msg.payload {
                            current_stage = changed.new_stage;
                            focus_set = changed.focus_set;
                            tracing::info!(stage = ?current_stage, "ResearchAgent stage changed");
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lagged = n, "ResearchAgent system_rx lagged");
                    }
                    Err(_) => break,
                }
            }
            result = market_rx.recv() => {
                // During Focus, monitor for significant moves in focus set.
                if current_stage != CycleStage::Focus {
                    continue;
                }
                match result {
                    Ok(msg) => {
                        if let Payload::MarketTick(tick) = &msg.payload {
                            if focus_set.contains(&tick.instrument_id) {
                                // Check for >3% move (simplified: any last price deviation).
                                // In a real impl we'd track baseline prices.
                                // For now, just pass through — real monitoring deferred.
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
            _ = scan_tick.tick() => {
                // Periodic scan trigger during Scan phase.
                if current_stage == CycleStage::Scan && last_scan.elapsed() >= scan_interval {
                    tracing::info!("ResearchAgent performing scan");
                    let findings = scan_binance(&http, &config).await;
                    for finding in findings {
                        let msg = bus_msg(
                            Topic::Signal,
                            Payload::ResearchFinding(finding),
                            "ResearchAgent",
                        );
                        let _ = bus.publish(msg).await;
                    }
                    last_scan = tokio::time::Instant::now();
                    publish_heartbeat(
                        &bus,
                        AgentRunStatus::Running,
                        Some("Scan published".to_string()),
                    ).await;
                }
            }
        }
    }

    publish_heartbeat(&bus, AgentRunStatus::Stopped, Some("Shutdown".to_string())).await;
    tracing::info!("ResearchAgent exiting");
}
