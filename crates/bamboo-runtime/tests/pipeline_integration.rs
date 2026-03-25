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
