//! Integration test: full synthetic pipeline end-to-end.
//!
//! Verifies that messages flow through:
//! SyntheticFeed -> Research -> Strategy -> Portfolio -> Risk -> Execution
//! and that the bus delivers messages to all subscribers.

use std::sync::Arc;
use std::time::Duration;

use bamboo_core::{EventBus, Payload, Topic};
use bamboo_runtime::{LocalBus, ShutdownSignal};

/// Test that the synthetic feed produces MarketTick messages on the bus.
#[tokio::test]
async fn synthetic_feed_produces_market_ticks() {
    let bus = Arc::new(LocalBus::new());
    let shutdown = ShutdownSignal::new();
    let mut rx = bus.subscribe(Topic::MarketData);

    bamboo_runtime::mock_agents::spawn_synthetic_feed(
        bus.clone(),
        shutdown.clone(),
        vec!["BTCUSDT".to_string()],
        Duration::from_millis(100),
    );

    // Should receive at least one tick within 500ms
    let msg = tokio::time::timeout(Duration::from_millis(500), rx.recv())
        .await
        .expect("timeout waiting for tick")
        .expect("recv error");

    assert!(matches!(msg.payload, Payload::MarketTick(_)));
    assert_eq!(msg.topic, Topic::MarketData);

    shutdown.trigger();
}

/// Test that the mock news feed produces NewsItem messages.
#[tokio::test]
async fn mock_news_feed_produces_news() {
    let bus = Arc::new(LocalBus::new());
    let shutdown = ShutdownSignal::new();
    let mut rx = bus.subscribe(Topic::News);

    bamboo_runtime::mock_agents::spawn_mock_news_feed(bus.clone(), shutdown.clone());

    // First news arrives after ~2 seconds
    let msg = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for news")
        .expect("recv error");

    assert!(matches!(msg.payload, Payload::NewsItem(_)));
    shutdown.trigger();
}

/// Test that subscribe_all receives messages from multiple topics.
#[tokio::test]
async fn subscribe_all_receives_cross_topic() {
    let bus = Arc::new(LocalBus::new());
    let shutdown = ShutdownSignal::new();
    let mut rx = bus.subscribe_all();

    // Spawn both feeds
    bamboo_runtime::mock_agents::spawn_synthetic_feed(
        bus.clone(),
        shutdown.clone(),
        vec!["BTCUSDT".to_string()],
        Duration::from_millis(100),
    );
    bamboo_runtime::mock_agents::spawn_mock_news_feed(bus.clone(), shutdown.clone());

    // Collect messages for 3 seconds
    let mut market_count = 0u32;
    let mut news_count = 0u32;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(msg) => match msg.topic {
                        Topic::MarketData => market_count += 1,
                        Topic::News => news_count += 1,
                        _ => {}
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
            _ = tokio::time::sleep_until(deadline) => break,
        }
    }

    assert!(market_count > 0, "expected market ticks, got 0");
    assert!(news_count > 0, "expected news items, got 0");
    shutdown.trigger();
}

/// Test bus metrics are updated when messages flow.
#[tokio::test]
async fn bus_metrics_track_messages() {
    let bus = Arc::new(LocalBus::new());
    let shutdown = ShutdownSignal::new();
    let _rx = bus.subscribe(Topic::MarketData); // need a subscriber

    bamboo_runtime::mock_agents::spawn_synthetic_feed(
        bus.clone(),
        shutdown.clone(),
        vec!["BTCUSDT".to_string()],
        Duration::from_millis(50),
    );

    tokio::time::sleep(Duration::from_millis(300)).await;

    let metrics = bus.metrics();
    assert!(
        metrics.messages_published > 0,
        "expected published > 0, got {}",
        metrics.messages_published
    );

    shutdown.trigger();
}

/// Test paper venue fills an order with correct slippage.
#[tokio::test]
async fn paper_venue_fills_with_slippage() {
    use bamboo_core::{
        ClientOrderId, ExecutionOrderIntent, InstrumentId, OrderSide, OrderType, PaperConfig,
        Price, Quantity, TimeInForce, TradingMode, Venue, VenueAdapter,
    };
    use uuid::Uuid;

    let config = PaperConfig {
        slippage_bps: 10, // 0.1%
        latency_ms: 0,    // no delay for test
    };

    let venue = bamboo_runtime::venues::PaperVenue::new(config);

    // Set a known price
    let instrument = InstrumentId::from_parts("BTCUSDT", "BINANCE");
    venue
        .set_price(instrument.clone(), Price::from_f64(50_000.0, 2))
        .await;

    let order = ExecutionOrderIntent {
        id: Uuid::new_v4(),
        decision_id: Uuid::new_v4(),
        client_order_id: ClientOrderId::new("test-1"),
        instrument_id: instrument,
        venue: Venue::new("BINANCE"),
        side: OrderSide::Buy,
        order_type: OrderType::Market,
        quantity: Quantity::from_f64(1.0, 8),
        limit_price: None,
        stop_price: None,
        time_in_force: TimeInForce::GTC,
        timestamp: 0,
    };

    let venue_id = venue.submit_order(&order).await.expect("submit failed");
    assert!(!venue_id.to_string().is_empty());
    assert_eq!(venue.trading_mode(), TradingMode::Paper);

    // Check fills
    let fills = venue.fills().await;
    assert_eq!(fills.len(), 1);
    let fill = &fills[0];
    // Buy slippage: price * (1 + 10/10000) = 50000 * 1.001 = 50050
    let fill_price = fill.fill_price.as_f64();
    assert!(
        (fill_price - 50_050.0).abs() < 1.0,
        "expected ~50050, got {fill_price}"
    );
}

/// Test persistence roundtrip for positions and orders.
#[tokio::test]
async fn persistence_full_roundtrip() {
    use bamboo_core::{
        ClientOrderId, Currency, InstrumentId, Money, OrderSide, OrderType, PositionId,
        PositionSide, PositionUpdate, Price, Quantity,
    };
    use bamboo_runtime::persistence::StateStore;

    let store = StateStore::open(":memory:").expect("open failed");

    // Save a position
    let pos = PositionUpdate {
        position_id: PositionId::new("POS-1"),
        instrument_id: InstrumentId::from_parts("BTCUSDT", "BINANCE"),
        side: PositionSide::Long,
        quantity: Quantity::from_f64(0.5, 8),
        avg_entry_price: Price::from_f64(68000.0, 2),
        unrealized_pnl: Some(Money::from_f64(500.0, Currency::usdt())),
        realized_pnl: None,
        timestamp: 1000,
    };
    store.save_position(&pos).expect("save pos failed");

    // Load positions
    let positions = store.load_positions().expect("load pos failed");
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].position_id.to_string(), "POS-1");

    // Save an open order
    let order = bamboo_runtime::agents::execution::OrderState {
        client_order_id: ClientOrderId::new("CLT-1"),
        venue_order_id: None,
        instrument_id: InstrumentId::from_parts("ETHUSDT", "BINANCE"),
        side: OrderSide::Buy,
        order_type: OrderType::Limit,
        quantity: Quantity::from_f64(2.0, 8),
        limit_price: Some(Price::from_f64(3800.0, 2)),
        status: bamboo_core::OrderStatus::Submitted,
        filled_quantity: Quantity::from_f64(0.0, 8),
        avg_fill_price: None,
        created_at: 2000,
        updated_at: 2000,
    };
    store.save_order(&order).expect("save order failed");

    let open_orders = store.load_open_orders().expect("load orders failed");
    assert_eq!(open_orders.len(), 1);
    assert_eq!(open_orders[0].client_order_id.to_string(), "CLT-1");

    // Save and load portfolio
    store
        .save_portfolio(100_000_00, 2, 95_000_00, 2, 3000)
        .expect("save portfolio failed");
    let loaded = store.load_portfolio().expect("load portfolio failed");
    assert!(loaded.is_some());
    let (eq_raw, eq_prec, cap_raw, cap_prec, ts) = loaded.unwrap();
    assert_eq!(eq_raw, 100_000_00);
    assert_eq!(eq_prec, 2);
    assert_eq!(cap_raw, 95_000_00);
    assert_eq!(cap_prec, 2);
    assert_eq!(ts, 3000);
}

/// Test safe mode activation and deactivation.
#[tokio::test]
async fn safe_mode_lifecycle() {
    let bus: Arc<dyn EventBus> = Arc::new(LocalBus::new());
    let mut rx = bus.subscribe(Topic::Risk); // SafeMode publishes to Risk topic

    let safe = bamboo_runtime::safe_mode::SafeMode::new();
    assert!(!safe.is_active());

    // Activate
    safe.activate("drawdown limit breached", &bus).await;
    assert!(safe.is_active());
    assert_eq!(safe.reason(), "drawdown limit breached");

    // Check that EmergencyAction was published — use try_recv since it's already published
    let msg = rx.try_recv().expect("expected EmergencyAction on bus");
    assert!(matches!(msg.payload, Payload::EmergencyAction(_)));

    // Deactivate
    safe.deactivate();
    assert!(!safe.is_active());
}

/// V3 + V4: Full pipeline end-to-end test with all real agents (Strategy → Execution).
///
/// Uses all real agents: CycleManager, StrategyAgent, PortfolioAgent, RiskAgent,
/// ExecutionAgent + PaperVenue. Injects a synthetic ResearchFinding to seed the
/// pipeline (the ResearchAgent would normally produce these from Binance, but the
/// Binance public API may be geo-restricted in the test environment).
///
/// Verifies all 8 message types flow through the real agent pipeline within 10s.
#[tokio::test(flavor = "multi_thread")]
async fn v3_full_pipeline_collects_all_message_types() {
    use std::collections::HashSet;

    use bamboo_core::{
        BusMessage, ComponentId, Currency, CycleConfig, EventBus, ExecutionConfig,
        InstrumentId, MeanReversionParams, MomentumParams, Money, OrderSide, PaperConfig,
        Payload, PortfolioConfig, Price, ResearchFinding, ResearchConfig, RiskLimitsConfig,
        StrategyConfig, Topic, TradingMode, VenueAdapter,
    };

    let bus: Arc<dyn EventBus> = Arc::new(LocalBus::new());
    let shutdown = ShutdownSignal::new();

    // PaperVenue with minimal latency for testing
    let venue = Arc::new(bamboo_runtime::PaperVenue::new(PaperConfig {
        slippage_bps: 5,
        latency_ms: 10,
    }));
    venue.start_price_listener(bus.clone());

    // Subscribe BEFORE spawning so we don't miss early messages
    let mut rx_all = bus.subscribe_all();

    // Synthetic feed — emits MarketTick for BTCUSDT so PaperVenue has prices
    bamboo_runtime::mock_agents::spawn_synthetic_feed(
        bus.clone(),
        shutdown.clone(),
        vec!["BTCUSDT".to_string(), "ETHUSDT".to_string(), "SOLUSDT".to_string()],
        Duration::from_millis(100),
    );

    // CycleManager
    {
        let b = bus.clone();
        let s = shutdown.clone();
        tokio::spawn(async move {
            bamboo_runtime::run_cycle_manager(
                b,
                CycleConfig { default_duration_hours: 24, auto_advance: false },
                10,
                s,
            )
            .await;
        });
    }

    // ResearchAgent (runs but won't produce findings if Binance is unavailable)
    {
        let b = bus.clone();
        let s = shutdown.clone();
        tokio::spawn(async move {
            bamboo_runtime::run_research_agent(
                b,
                ResearchConfig {
                    min_volume_usd: 1_000_000.0,
                    max_candidates: 10,
                    scan_interval_secs: 300,
                },
                s,
            )
            .await;
        });
    }

    // StrategyAgent — low threshold to ensure signals fire on any market movement
    {
        let b = bus.clone();
        let s = shutdown.clone();
        tokio::spawn(async move {
            bamboo_runtime::run_strategy_agent(
                b,
                StrategyConfig {
                    enabled_strategies: vec!["momentum".to_string()],
                    max_concurrent_signals: 10,
                    momentum: MomentumParams {
                        min_change_pct: 0.01,
                        hold_hours: 4,
                        stop_loss_pct: 1.0,
                    },
                    mean_reversion: MeanReversionParams::default(),
                },
                s,
            )
            .await;
        });
    }

    // PortfolioAgent
    {
        let b = bus.clone();
        let s = shutdown.clone();
        tokio::spawn(async move {
            bamboo_runtime::run_portfolio_agent(
                b,
                PortfolioConfig {
                    initial_capital_usd: Money::new(
                        Price::from_f64(100_000.0, 2),
                        Currency::usd(),
                    ),
                    max_positions: 10,
                    risk_pct_per_trade: 1.0,
                },
                s,
            )
            .await;
        });
    }

    // RiskAgent — generous limits so trades are approved
    {
        let b = bus.clone();
        let s = shutdown.clone();
        tokio::spawn(async move {
            bamboo_runtime::run_risk_agent(
                b,
                RiskLimitsConfig {
                    max_position_size_usd: Money::new(
                        Price::from_f64(50_000.0, 2),
                        Currency::usd(),
                    ),
                    max_portfolio_exposure_usd: Money::new(
                        Price::from_f64(500_000.0, 2),
                        Currency::usd(),
                    ),
                    max_concentration_pct: 50.0,
                    max_drawdown_pct: 30.0,
                    order_rate_limit_per_min: 100,
                    kill_switch_enabled: false,
                },
                100_000.0,
                s,
            )
            .await;
        });
    }

    // ExecutionAgent with real PaperVenue
    {
        let b = bus.clone();
        let s = shutdown.clone();
        let v = venue.clone() as Arc<dyn VenueAdapter>;
        tokio::spawn(async move {
            bamboo_runtime::run_execution_agent(
                b,
                v,
                ExecutionConfig {
                    mode: TradingMode::Paper,
                    max_open_orders: 10,
                    order_timeout_secs: 300,
                    retry_failed_orders: false,
                },
                s,
            )
            .await;
        });
    }

    // Wait for agents to subscribe to their topics and the synthetic feed to
    // emit its first tick (so PaperVenue has a price for BTCUSDT.BINANCE).
    tokio::time::sleep(Duration::from_millis(400)).await;

    // Inject a synthetic ResearchFinding as if produced by the ResearchAgent.
    // Thesis format matches what parse_change_pct() expects: "24h change X.XX%, ..."
    let finding = ResearchFinding {
        id: uuid::Uuid::new_v4(),
        instrument_id: InstrumentId::from_parts("BTCUSDT", "BINANCE"),
        thesis: "24h change 5.20%, vol $1500000000, volatility 3.50%".to_string(),
        score: 0.85,
        recommended_action: Some(OrderSide::Buy),
        timestamp: 0,
    };
    let finding_msg = BusMessage {
        id: uuid::Uuid::new_v4(),
        topic: Topic::Signal,
        payload: Payload::ResearchFinding(finding),
        timestamp: 0,
        source: ComponentId::new("TestHarness"),
    };
    bus.publish(finding_msg).await.unwrap();

    // Collect messages up to 10s (all pipeline stages are local, no network needed)
    let required_types = [
        "MarketTick",
        "CycleStageChanged",
        "ResearchFinding",
        "StrategySignal",
        "PortfolioIntent",
        "RiskDecision",
        "ExecutionReport",
        "PositionUpdate",
    ];
    let mut seen: HashSet<&'static str> = HashSet::new();
    let mut saw_filled_report = false;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }

        match tokio::time::timeout(remaining, rx_all.recv()).await {
            Ok(Ok(msg)) => {
                match &msg.payload {
                    bamboo_core::Payload::MarketTick(_) => {
                        seen.insert("MarketTick");
                    }
                    bamboo_core::Payload::CycleStageChanged(_) => {
                        seen.insert("CycleStageChanged");
                    }
                    bamboo_core::Payload::ResearchFinding(_) => {
                        seen.insert("ResearchFinding");
                    }
                    bamboo_core::Payload::StrategySignal(_) => {
                        seen.insert("StrategySignal");
                    }
                    bamboo_core::Payload::PortfolioIntent(_) => {
                        seen.insert("PortfolioIntent");
                    }
                    bamboo_core::Payload::RiskDecision(_) => {
                        seen.insert("RiskDecision");
                    }
                    bamboo_core::Payload::ExecutionReport(r) => {
                        seen.insert("ExecutionReport");
                        if r.status == bamboo_core::OrderStatus::Filled {
                            saw_filled_report = true;
                            // V4: avg_fill_price must be Some for a Filled order
                            assert!(
                                r.avg_fill_price.is_some(),
                                "Filled ExecutionReport must have avg_fill_price"
                            );
                        }
                    }
                    bamboo_core::Payload::PositionUpdate(_) => {
                        seen.insert("PositionUpdate");
                    }
                    _ => {}
                }
                if seen.len() == required_types.len() {
                    break;
                }
            }
            Ok(Err(_)) | Err(_) => break,
        }
    }

    shutdown.shutdown();

    // Assert all 8 message types were observed
    for msg_type in &required_types {
        assert!(
            seen.contains(msg_type),
            "missing message type: {msg_type}\nSeen so far: {:?}",
            seen
        );
    }

    // V4: at least one Filled ExecutionReport with non-None avg_fill_price
    assert!(saw_filled_report, "expected at least one Filled ExecutionReport");
}

/// V4: Risk agent rejects oversized position and PortfolioAgent returns capital.
#[tokio::test(flavor = "multi_thread")]
async fn v4_risk_rejects_oversized_position() {
    use bamboo_core::{
        Currency, EventBus, InstrumentId, Money, OrderSide,
        Payload, PortfolioConfig, Price, RiskLimitsConfig,
    };

    let bus: Arc<dyn EventBus> = Arc::new(LocalBus::new());
    let shutdown = ShutdownSignal::new();
    let mut rx = bus.subscribe_all();

    // PortfolioAgent: 1000 USD capital, 1% risk per trade, 3% stop = 333 USD per position
    {
        let b = bus.clone();
        let s = shutdown.clone();
        tokio::spawn(async move {
            bamboo_runtime::run_portfolio_agent(
                b,
                PortfolioConfig {
                    initial_capital_usd: Money::new(Price::from_f64(1_000.0, 2), Currency::usd()),
                    max_positions: 10,
                    risk_pct_per_trade: 1.0,
                },
                s,
            )
            .await;
        });
    }

    // RiskAgent: max_position_size = 100 USD — will reject 333 USD position
    {
        let b = bus.clone();
        let s = shutdown.clone();
        tokio::spawn(async move {
            bamboo_runtime::run_risk_agent(
                b,
                RiskLimitsConfig {
                    max_position_size_usd: Money::new(Price::from_f64(100.0, 2), Currency::usd()),
                    max_portfolio_exposure_usd: Money::new(
                        Price::from_f64(10_000.0, 2),
                        Currency::usd(),
                    ),
                    max_concentration_pct: 50.0,
                    max_drawdown_pct: 30.0,
                    order_rate_limit_per_min: 100,
                    kill_switch_enabled: false,
                },
                1_000.0,
                s,
            )
            .await;
        });
    }

    // Publish a StrategySignal that will trigger a PortfolioIntent > 100 USD
    let signal = bamboo_core::StrategySignal {
        id: uuid::Uuid::new_v4(),
        strategy_id: bamboo_core::StrategyId::new("test"),
        instrument_id: InstrumentId::from_parts("BTCUSDT", "BINANCE"),
        side: OrderSide::Buy,
        entry_price: None,
        exit_price: None,
        stop_loss: None,
        rationale: "test signal".to_string(),
        confidence: 0.9,
        horizon_hours: 4,
        timestamp: 0,
    };

    // Give agents time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    let msg = bamboo_core::BusMessage {
        id: uuid::Uuid::new_v4(),
        topic: bamboo_core::Topic::Signal,
        payload: bamboo_core::Payload::StrategySignal(signal),
        timestamp: 0,
        source: bamboo_core::ComponentId::new("test"),
    };
    bus.publish(msg).await.unwrap();

    // Collect messages, expect a rejected RiskDecision (no ExecutionReport)
    let mut got_intent = false;
    let mut rejection: Option<bool> = None;
    let mut got_execution_report = false;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(msg)) => match &msg.payload {
                Payload::PortfolioIntent(_) => got_intent = true,
                Payload::RiskDecision(d) => rejection = Some(d.approved),
                Payload::ExecutionReport(_) => got_execution_report = true,
                _ => {}
            },
            Ok(Err(_)) | Err(_) => break,
        }
        // Stop once we have intent + decision
        if got_intent && rejection.is_some() {
            break;
        }
    }

    shutdown.shutdown();

    assert!(got_intent, "expected PortfolioIntent to be published");
    assert_eq!(
        rejection,
        Some(false),
        "expected RiskDecision.approved == false for oversized position"
    );
    assert!(
        !got_execution_report,
        "ExecutionReport should not appear after risk rejection"
    );
}

/// Test shutdown signal coordination.
#[tokio::test]
async fn shutdown_signal_stops_feeds() {
    let bus = Arc::new(LocalBus::new());
    let shutdown = ShutdownSignal::new();
    let mut rx = bus.subscribe(Topic::MarketData);

    let handle = bamboo_runtime::mock_agents::spawn_synthetic_feed(
        bus.clone(),
        shutdown.clone(),
        vec!["BTCUSDT".to_string()],
        Duration::from_millis(50),
    );

    // Wait for a tick
    let _ = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await;

    // Trigger shutdown
    shutdown.trigger();

    // Feed task should terminate
    let result = tokio::time::timeout(Duration::from_secs(2), handle).await;
    assert!(result.is_ok(), "feed task did not terminate after shutdown");
}
