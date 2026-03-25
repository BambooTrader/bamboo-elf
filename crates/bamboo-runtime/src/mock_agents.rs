//! Six mock agent tasks for synthetic end-to-end flow validation.
//!
//! Each agent subscribes to its upstream topic and publishes downstream messages
//! with simple logic and realistic delays.

use std::sync::Arc;
use std::time::Duration;

use bamboo_core::{
    BusMessage, ClientOrderId, ComponentId, EventBus, ExecutionReport, InstrumentId, LiquiditySide,
    MarketTick, Money, OrderSide, OrderStatus, OrderType, Payload, PortfolioIntent, PositionId,
    PositionSide, PositionUpdate, Price, Quantity, ResearchFinding, RiskDecision, StrategyId,
    StrategySignal, TimeInForce, Topic,
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

// ── 1. Synthetic Feed ────────────────────────────────────────────────────────

/// Generates fake `MarketTick` every ~1 second for BTC/USDT with a random walk.
pub async fn mock_synthetic_feed(bus: Arc<dyn EventBus>, shutdown: ShutdownSignal) {
    use rand::rngs::SmallRng;
    use rand::{Rng, SeedableRng};

    let mut rng = SmallRng::from_entropy();
    let mut price = 68_000.0_f64;
    let instrument = InstrumentId::from_parts("BTCUSDT", "BINANCE");

    tracing::info!("mock_synthetic_feed started");

    loop {
        if shutdown.is_shutdown() {
            break;
        }

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(1)) => {}
            _ = shutdown.wait_for_shutdown() => { break; }
        }

        // Random walk: +/- up to 0.3% per tick.
        let delta = rng.gen_range(-0.003..0.003) * price;
        price += delta;
        price = price.max(10.0); // floor

        let spread = price * 0.0001;
        let tick = MarketTick {
            instrument_id: instrument.clone(),
            bid: Price::from_f64(price - spread, 2),
            ask: Price::from_f64(price + spread, 2),
            last: Price::from_f64(price, 2),
            volume_24h: Quantity::from_f64(25_000.0, 4),
            timestamp: now_nanos(),
        };

        let msg = bus_msg(Topic::MarketData, Payload::MarketTick(tick), "SyntheticFeed");
        let _ = bus.publish(msg).await;
    }

    tracing::info!("mock_synthetic_feed exiting");
}

// ── 2. Mock Research ─────────────────────────────────────────────────────────

/// Subscribes to `MarketData`, every ~5 seconds publishes a `ResearchFinding`.
pub async fn mock_research(bus: Arc<dyn EventBus>, shutdown: ShutdownSignal) {
    use rand::rngs::SmallRng;
    use rand::{Rng, SeedableRng};

    let mut rx = bus.subscribe(Topic::MarketData);
    let mut last_publish = tokio::time::Instant::now();
    let interval = Duration::from_secs(5);

    tracing::info!("mock_research started");

    loop {
        if shutdown.is_shutdown() {
            break;
        }

        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        if last_publish.elapsed() < interval {
                            continue;
                        }

                        // Extract instrument from the tick.
                        let instrument_id = match &msg.payload {
                            Payload::MarketTick(tick) => tick.instrument_id.clone(),
                            _ => continue,
                        };

                        // Simulate processing delay.
                        tokio::time::sleep(Duration::from_millis(200)).await;

                        let mut rng = SmallRng::from_entropy();
                        let score: f64 = rng.gen_range(0.3..0.95);
                        let side = if score > 0.6 {
                            Some(OrderSide::Buy)
                        } else {
                            Some(OrderSide::Sell)
                        };

                        let finding = ResearchFinding {
                            id: Uuid::new_v4(),
                            instrument_id,
                            thesis: format!("Momentum signal detected (score={score:.2})"),
                            score,
                            recommended_action: side,
                            timestamp: now_nanos(),
                        };

                        let msg = bus_msg(
                            Topic::Signal,
                            Payload::ResearchFinding(finding),
                            "MockResearch",
                        );
                        let _ = bus.publish(msg).await;
                        last_publish = tokio::time::Instant::now();
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lagged = n, "mock_research lagged");
                    }
                    Err(_) => break,
                }
            }
            _ = shutdown.wait_for_shutdown() => { break; }
        }
    }

    tracing::info!("mock_research exiting");
}

// ── 3. Mock Strategy ─────────────────────────────────────────────────────────

/// Subscribes to `Signal`, on `ResearchFinding` publishes a `StrategySignal`.
pub async fn mock_strategy(bus: Arc<dyn EventBus>, shutdown: ShutdownSignal) {
    let mut rx = bus.subscribe(Topic::Signal);

    tracing::info!("mock_strategy started");

    loop {
        if shutdown.is_shutdown() {
            break;
        }

        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        let finding = match &msg.payload {
                            Payload::ResearchFinding(f) => f.clone(),
                            _ => continue,
                        };

                        // Simulate processing delay.
                        tokio::time::sleep(Duration::from_millis(150)).await;

                        let side = finding.recommended_action.unwrap_or(OrderSide::Buy);

                        let signal = StrategySignal {
                            id: Uuid::new_v4(),
                            strategy_id: StrategyId::new("momentum-v1"),
                            instrument_id: finding.instrument_id,
                            side,
                            entry_price: None, // market order
                            exit_price: None,
                            stop_loss: None,
                            rationale: format!(
                                "Acting on research finding (score={:.2})",
                                finding.score
                            ),
                            confidence: finding.score * 0.9,
                            horizon_hours: 4,
                            timestamp: now_nanos(),
                        };

                        let msg = bus_msg(
                            Topic::Signal,
                            Payload::StrategySignal(signal),
                            "MockStrategy",
                        );
                        let _ = bus.publish(msg).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lagged = n, "mock_strategy lagged");
                    }
                    Err(_) => break,
                }
            }
            _ = shutdown.wait_for_shutdown() => { break; }
        }
    }

    tracing::info!("mock_strategy exiting");
}

// ── 4. Mock Portfolio ────────────────────────────────────────────────────────

/// Subscribes to `Signal`, on `StrategySignal` publishes a `PortfolioIntent`.
pub async fn mock_portfolio(bus: Arc<dyn EventBus>, shutdown: ShutdownSignal) {
    let mut rx = bus.subscribe(Topic::Signal);

    tracing::info!("mock_portfolio started");

    loop {
        if shutdown.is_shutdown() {
            break;
        }

        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        let signal = match &msg.payload {
                            Payload::StrategySignal(s) => s.clone(),
                            _ => continue, // skip ResearchFinding — that is for strategy
                        };

                        // Simulate portfolio sizing delay.
                        tokio::time::sleep(Duration::from_millis(100)).await;

                        let intent = PortfolioIntent {
                            id: Uuid::new_v4(),
                            signal_id: signal.id,
                            instrument_id: signal.instrument_id,
                            side: signal.side,
                            quantity: Quantity::from_f64(0.01, 8), // small size
                            order_type: OrderType::Market,
                            limit_price: None,
                            stop_price: None,
                            time_in_force: TimeInForce::GTC,
                            timestamp: now_nanos(),
                        };

                        let msg = bus_msg(
                            Topic::Intent,
                            Payload::PortfolioIntent(intent),
                            "MockPortfolio",
                        );
                        let _ = bus.publish(msg).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lagged = n, "mock_portfolio lagged");
                    }
                    Err(_) => break,
                }
            }
            _ = shutdown.wait_for_shutdown() => { break; }
        }
    }

    tracing::info!("mock_portfolio exiting");
}

// ── 5. Mock Risk ─────────────────────────────────────────────────────────────

/// Subscribes to `Intent`, on `PortfolioIntent` publishes a `RiskDecision` (always approve).
pub async fn mock_risk(bus: Arc<dyn EventBus>, shutdown: ShutdownSignal) {
    let mut rx = bus.subscribe(Topic::Intent);

    tracing::info!("mock_risk started");

    loop {
        if shutdown.is_shutdown() {
            break;
        }

        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        let intent = match &msg.payload {
                            Payload::PortfolioIntent(i) => i.clone(),
                            _ => continue,
                        };

                        // Simulate risk check delay.
                        tokio::time::sleep(Duration::from_millis(100)).await;

                        let decision = RiskDecision {
                            id: Uuid::new_v4(),
                            intent_id: intent.id,
                            approved: true,
                            reason: "Within risk limits".to_string(),
                            adjusted_quantity: None,
                            constraints: vec!["max 50% of daily limit".to_string()],
                            timestamp: now_nanos(),
                        };

                        let msg = bus_msg(
                            Topic::Risk,
                            Payload::RiskDecision(decision),
                            "MockRisk",
                        );
                        let _ = bus.publish(msg).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lagged = n, "mock_risk lagged");
                    }
                    Err(_) => break,
                }
            }
            _ = shutdown.wait_for_shutdown() => { break; }
        }
    }

    tracing::info!("mock_risk exiting");
}

// ── 6. Mock Execution ────────────────────────────────────────────────────────

/// Subscribes to `Risk`, on `RiskDecision` publishes `ExecutionReport` + `PositionUpdate`.
pub async fn mock_execution(bus: Arc<dyn EventBus>, shutdown: ShutdownSignal) {
    use rand::rngs::SmallRng;
    use rand::{Rng, SeedableRng};

    let mut rx = bus.subscribe(Topic::Risk);
    let mut position_count: u64 = 0;

    tracing::info!("mock_execution started");

    loop {
        if shutdown.is_shutdown() {
            break;
        }

        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        let _decision = match &msg.payload {
                            Payload::RiskDecision(d) if d.approved => d.clone(),
                            _ => continue,
                        };

                        // Simulate execution latency.
                        tokio::time::sleep(Duration::from_millis(300)).await;

                        let mut rng = SmallRng::from_entropy();
                        let fill_price = 68_000.0 + rng.gen_range(-500.0..500.0);
                        let quantity = Quantity::from_f64(0.01, 8);
                        position_count += 1;

                        // Publish ExecutionReport (Filled).
                        let report = ExecutionReport {
                            client_order_id: ClientOrderId::new(format!("CLT-{}", Uuid::new_v4())),
                            venue_order_id: None,
                            instrument_id: InstrumentId::from_parts("BTCUSDT", "BINANCE"),
                            status: OrderStatus::Filled,
                            side: OrderSide::Buy,
                            filled_quantity: quantity,
                            avg_fill_price: Some(Price::from_f64(fill_price, 2)),
                            commission: Some(Money::from_f64(
                                fill_price * 0.01 * 0.001,
                                bamboo_core::Currency::usdt(),
                            )),
                            liquidity_side: Some(LiquiditySide::Taker),
                            timestamp: now_nanos(),
                        };

                        let exec_msg = bus_msg(
                            Topic::Execution,
                            Payload::ExecutionReport(report),
                            "MockExecution",
                        );
                        let _ = bus.publish(exec_msg).await;

                        // Publish PositionUpdate.
                        let pos = PositionUpdate {
                            position_id: PositionId::new(format!("POS-{position_count}")),
                            instrument_id: InstrumentId::from_parts("BTCUSDT", "BINANCE"),
                            side: PositionSide::Long,
                            quantity,
                            avg_entry_price: Price::from_f64(fill_price, 2),
                            unrealized_pnl: Some(Money::from_f64(0.0, bamboo_core::Currency::usdt())),
                            realized_pnl: None,
                            timestamp: now_nanos(),
                        };

                        let pos_msg = bus_msg(
                            Topic::Execution,
                            Payload::PositionUpdate(pos),
                            "MockExecution",
                        );
                        let _ = bus.publish(pos_msg).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(lagged = n, "mock_execution lagged");
                    }
                    Err(_) => break,
                }
            }
            _ = shutdown.wait_for_shutdown() => { break; }
        }
    }

    tracing::info!("mock_execution exiting");
}

// ── Legacy compatibility wrappers ────────────────────────────────────────────

/// Spawn a synthetic market data feed (compatible with previous API).
pub fn spawn_synthetic_feed(
    bus: Arc<dyn EventBus>,
    shutdown: ShutdownSignal,
    symbols: Vec<String>,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        use rand::rngs::SmallRng;
        use rand::{Rng, SeedableRng};

        let mut rng = SmallRng::from_entropy();
        let mut prices: Vec<f64> = symbols
            .iter()
            .map(|s| match s.as_str() {
                "BTCUSDT" => 68_000.0,
                "ETHUSDT" => 3_800.0,
                "SOLUSDT" => 185.0,
                _ => 100.0,
            })
            .collect();

        loop {
            if shutdown.is_shutdown() {
                break;
            }

            tokio::select! {
                _ = tokio::time::sleep(interval) => {}
                _ = shutdown.wait_for_shutdown() => { break; }
            }

            for (i, sym) in symbols.iter().enumerate() {
                let delta = rng.gen_range(-0.003..0.003) * prices[i];
                prices[i] += delta;
                prices[i] = prices[i].max(1.0);

                let price = prices[i];
                let instrument_id = InstrumentId::from_parts(sym, "BINANCE");
                let precision: u8 = if price > 1000.0 { 2 } else { 4 };
                let spread = price * 0.0001;

                let tick = MarketTick {
                    instrument_id,
                    bid: Price::from_f64(price - spread, precision),
                    ask: Price::from_f64(price + spread, precision),
                    last: Price::from_f64(price, precision),
                    volume_24h: Quantity::from_f64(1_000_000.0, 2),
                    timestamp: now_nanos(),
                };

                let msg = bus_msg(Topic::MarketData, Payload::MarketTick(tick), "SyntheticFeed");
                let _ = bus.publish(msg).await;
            }
        }
    })
}

/// Spawn a mock news feed that publishes sample news items periodically.
pub fn spawn_mock_news_feed(
    bus: Arc<dyn EventBus>,
    shutdown: ShutdownSignal,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let headlines = [
            ("BTC breaks above key resistance level", "CryptoNews"),
            ("Fed holds interest rates steady", "Reuters"),
            ("SOL DEX volume surges 40%", "DeFiPulse"),
            ("ETH staking yields hit new high", "CoinDesk"),
            ("Institutional BTC buying accelerates", "Bloomberg"),
        ];
        let mut idx = 0;

        // Publish first item quickly so TUI has content.
        tokio::time::sleep(Duration::from_secs(2)).await;

        loop {
            if shutdown.is_shutdown() {
                break;
            }

            let (title, source) = headlines[idx % headlines.len()];
            let news = bamboo_core::NewsItem {
                title: title.to_string(),
                source: source.to_string(),
                url: None,
                related_instruments: vec![InstrumentId::from_parts("BTCUSDT", "BINANCE")],
                timestamp: now_nanos(),
            };

            let msg = bus_msg(Topic::News, Payload::NewsItem(news), "MockNewsFeed");
            let _ = bus.publish(msg).await;
            idx += 1;

            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(30)) => {}
                _ = shutdown.wait_for_shutdown() => { break; }
            }
        }
    })
}
