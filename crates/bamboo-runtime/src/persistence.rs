//! State persistence via SQLite — saves and restores critical trading state.

use rusqlite::{params, Connection, Result as SqlResult};

use bamboo_core::{
    InstrumentId, OrderSide, OrderStatus, OrderType, PositionId, PositionSide, PositionUpdate,
    Price, Quantity, ClientOrderId, VenueOrderId,
};

use crate::agents::execution::OrderState;

/// SQLite-backed state store for persistence across restarts.
pub struct StateStore {
    conn: Connection,
}

impl StateStore {
    /// Open (or create) a SQLite database at the given path.
    pub fn open(path: &str) -> SqlResult<Self> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.create_tables()?;
        Ok(store)
    }

    /// Open an in-memory database (for testing).
    pub fn open_memory() -> SqlResult<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.create_tables()?;
        Ok(store)
    }

    fn create_tables(&self) -> SqlResult<()> {
        self.conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS positions (
                position_id TEXT PRIMARY KEY,
                instrument_id TEXT NOT NULL,
                side TEXT NOT NULL,
                quantity_raw INTEGER NOT NULL,
                quantity_precision INTEGER NOT NULL,
                avg_entry_price_raw INTEGER NOT NULL,
                avg_entry_price_precision INTEGER NOT NULL,
                unrealized_pnl_raw INTEGER,
                realized_pnl_raw INTEGER,
                timestamp INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS orders (
                client_order_id TEXT PRIMARY KEY,
                venue_order_id TEXT,
                instrument_id TEXT NOT NULL,
                side TEXT NOT NULL,
                order_type TEXT NOT NULL,
                quantity_raw INTEGER NOT NULL,
                quantity_precision INTEGER NOT NULL,
                limit_price_raw INTEGER,
                limit_price_precision INTEGER,
                status TEXT NOT NULL,
                filled_quantity_raw INTEGER NOT NULL,
                filled_quantity_precision INTEGER NOT NULL,
                avg_fill_price_raw INTEGER,
                avg_fill_price_precision INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS portfolio_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                total_equity_raw INTEGER NOT NULL,
                total_equity_precision INTEGER NOT NULL,
                available_capital_raw INTEGER NOT NULL,
                available_capital_precision INTEGER NOT NULL,
                timestamp INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS cycle_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                cycle_id TEXT NOT NULL,
                stage TEXT NOT NULL,
                focus_set TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS audit_trail (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_type TEXT NOT NULL,
                details TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            );
            ",
        )
    }

    // ── Positions ───────────────────────────────────────────────────────────

    pub fn save_position(&self, pos: &PositionUpdate) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO positions
             (position_id, instrument_id, side, quantity_raw, quantity_precision,
              avg_entry_price_raw, avg_entry_price_precision,
              unrealized_pnl_raw, realized_pnl_raw, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                pos.position_id.as_str(),
                pos.instrument_id.as_str(),
                format!("{:?}", pos.side),
                pos.quantity.raw as i64,
                pos.quantity.precision,
                pos.avg_entry_price.raw,
                pos.avg_entry_price.precision,
                pos.unrealized_pnl.as_ref().map(|m| m.amount.raw),
                pos.realized_pnl.as_ref().map(|m| m.amount.raw),
                pos.timestamp as i64,
            ],
        )?;
        Ok(())
    }

    pub fn load_positions(&self) -> SqlResult<Vec<PositionUpdate>> {
        let mut stmt = self.conn.prepare(
            "SELECT position_id, instrument_id, side, quantity_raw, quantity_precision,
                    avg_entry_price_raw, avg_entry_price_precision, timestamp
             FROM positions",
        )?;

        let rows = stmt.query_map([], |row| {
            let side_str: String = row.get(2)?;
            let side = match side_str.as_str() {
                "Long" => PositionSide::Long,
                "Short" => PositionSide::Short,
                _ => PositionSide::Flat,
            };

            Ok(PositionUpdate {
                position_id: PositionId::new(row.get::<_, String>(0)?),
                instrument_id: InstrumentId::new(row.get::<_, String>(1)?),
                side,
                quantity: Quantity::new(row.get::<_, i64>(3)? as u64, row.get::<_, u8>(4)?),
                avg_entry_price: Price::new(row.get(5)?, row.get(6)?),
                unrealized_pnl: None,
                realized_pnl: None,
                timestamp: row.get::<_, i64>(7)? as u64,
            })
        })?;

        rows.collect()
    }

    // ── Orders ──────────────────────────────────────────────────────────────

    pub fn save_order(&self, order: &OrderState) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO orders
             (client_order_id, venue_order_id, instrument_id, side, order_type,
              quantity_raw, quantity_precision, limit_price_raw, limit_price_precision,
              status, filled_quantity_raw, filled_quantity_precision,
              avg_fill_price_raw, avg_fill_price_precision,
              created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                order.client_order_id.as_str(),
                order.venue_order_id.as_ref().map(|v| v.as_str().to_string()),
                order.instrument_id.as_str(),
                format!("{:?}", order.side),
                format!("{:?}", order.order_type),
                order.quantity.raw as i64,
                order.quantity.precision,
                order.limit_price.map(|p| p.raw),
                order.limit_price.map(|p| p.precision),
                format!("{:?}", order.status),
                order.filled_quantity.raw as i64,
                order.filled_quantity.precision,
                order.avg_fill_price.map(|p| p.raw),
                order.avg_fill_price.map(|p| p.precision),
                order.created_at as i64,
                order.updated_at as i64,
            ],
        )?;
        Ok(())
    }

    pub fn load_open_orders(&self) -> SqlResult<Vec<OrderState>> {
        let mut stmt = self.conn.prepare(
            "SELECT client_order_id, venue_order_id, instrument_id, side, order_type,
                    quantity_raw, quantity_precision, limit_price_raw, limit_price_precision,
                    status, filled_quantity_raw, filled_quantity_precision,
                    avg_fill_price_raw, avg_fill_price_precision,
                    created_at, updated_at
             FROM orders
             WHERE status NOT IN ('Filled', 'Rejected', 'Canceled', 'Expired')",
        )?;

        let rows = stmt.query_map([], |row| {
            let side_str: String = row.get(3)?;
            let side = match side_str.as_str() {
                "Buy" => OrderSide::Buy,
                _ => OrderSide::Sell,
            };
            let ot_str: String = row.get(4)?;
            let order_type = match ot_str.as_str() {
                "Limit" => OrderType::Limit,
                "StopMarket" => OrderType::StopMarket,
                "StopLimit" => OrderType::StopLimit,
                _ => OrderType::Market,
            };
            let status_str: String = row.get(9)?;
            let status = match status_str.as_str() {
                "Initialized" => OrderStatus::Initialized,
                "Submitted" => OrderStatus::Submitted,
                "Accepted" => OrderStatus::Accepted,
                "PartiallyFilled" => OrderStatus::PartiallyFilled,
                _ => OrderStatus::Submitted,
            };

            let venue_oid: Option<String> = row.get(1)?;
            let lp_raw: Option<i64> = row.get(7)?;
            let lp_prec: Option<u8> = row.get(8)?;
            let afp_raw: Option<i64> = row.get(12)?;
            let afp_prec: Option<u8> = row.get(13)?;

            Ok(OrderState {
                client_order_id: ClientOrderId::new(row.get::<_, String>(0)?),
                venue_order_id: venue_oid.map(VenueOrderId::new),
                instrument_id: InstrumentId::new(row.get::<_, String>(2)?),
                side,
                order_type,
                quantity: Quantity::new(row.get::<_, i64>(5)? as u64, row.get::<_, u8>(6)?),
                limit_price: lp_raw.zip(lp_prec).map(|(r, p)| Price::new(r, p)),
                status,
                filled_quantity: Quantity::new(
                    row.get::<_, i64>(10)? as u64,
                    row.get::<_, u8>(11)?,
                ),
                avg_fill_price: afp_raw.zip(afp_prec).map(|(r, p)| Price::new(r, p)),
                created_at: row.get::<_, i64>(14)? as u64,
                updated_at: row.get::<_, i64>(15)? as u64,
            })
        })?;

        rows.collect()
    }

    // ── Portfolio State ─────────────────────────────────────────────────────

    pub fn save_portfolio(&self, equity_raw: i64, equity_prec: u8, capital_raw: i64, capital_prec: u8, timestamp: u64) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO portfolio_state
             (id, total_equity_raw, total_equity_precision,
              available_capital_raw, available_capital_precision, timestamp)
             VALUES (1, ?1, ?2, ?3, ?4, ?5)",
            params![equity_raw, equity_prec, capital_raw, capital_prec, timestamp as i64],
        )?;
        Ok(())
    }

    pub fn load_portfolio(&self) -> SqlResult<Option<(i64, u8, i64, u8, u64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT total_equity_raw, total_equity_precision,
                    available_capital_raw, available_capital_precision, timestamp
             FROM portfolio_state WHERE id = 1",
        )?;

        let mut rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, u8>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, u8>(3)?,
                row.get::<_, i64>(4)? as u64,
            ))
        })?;

        match rows.next() {
            Some(Ok(v)) => Ok(Some(v)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    // ── Cycle State ─────────────────────────────────────────────────────────

    pub fn save_cycle(&self, cycle_id: &str, stage: &str, focus_set: &str, timestamp: u64) -> SqlResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO cycle_state (id, cycle_id, stage, focus_set, timestamp)
             VALUES (1, ?1, ?2, ?3, ?4)",
            params![cycle_id, stage, focus_set, timestamp as i64],
        )?;
        Ok(())
    }

    pub fn load_cycle(&self) -> SqlResult<Option<(String, String, String, u64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT cycle_id, stage, focus_set, timestamp FROM cycle_state WHERE id = 1",
        )?;

        let mut rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)? as u64,
            ))
        })?;

        match rows.next() {
            Some(Ok(v)) => Ok(Some(v)),
            Some(Err(e)) => Err(e),
            None => Ok(None),
        }
    }

    // ── Audit Trail ─────────────────────────────────────────────────────────

    pub fn save_audit(&self, event_type: &str, details: &str, timestamp: u64) -> SqlResult<()> {
        self.conn.execute(
            "INSERT INTO audit_trail (event_type, details, timestamp) VALUES (?1, ?2, ?3)",
            params![event_type, details, timestamp as i64],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bamboo_core::{Money, Currency};

    #[test]
    fn persistence_position_roundtrip() {
        let store = StateStore::open_memory().unwrap();

        let pos = PositionUpdate {
            position_id: PositionId::new("POS-BTCUSDT"),
            instrument_id: InstrumentId::from_parts("BTCUSDT", "BINANCE"),
            side: PositionSide::Long,
            quantity: Quantity::from_f64(0.5, 8),
            avg_entry_price: Price::from_f64(50_000.0, 2),
            unrealized_pnl: Some(Money::from_f64(250.0, Currency::usd())),
            realized_pnl: None,
            timestamp: 1234567890,
        };

        store.save_position(&pos).unwrap();
        let loaded = store.load_positions().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].position_id.as_str(), "POS-BTCUSDT");
        assert_eq!(loaded[0].instrument_id.as_str(), "BTCUSDT.BINANCE");
        assert!((loaded[0].quantity.as_f64() - 0.5).abs() < 1e-9);
        assert!((loaded[0].avg_entry_price.as_f64() - 50_000.0).abs() < 1e-6);
    }

    #[test]
    fn persistence_order_roundtrip() {
        let store = StateStore::open_memory().unwrap();

        let order = OrderState {
            client_order_id: ClientOrderId::new("EXE-001"),
            venue_order_id: Some(VenueOrderId::new("PAPER-1")),
            instrument_id: InstrumentId::from_parts("ETHUSDT", "BINANCE"),
            side: OrderSide::Buy,
            order_type: OrderType::Market,
            quantity: Quantity::from_f64(2.0, 8),
            limit_price: None,
            status: OrderStatus::Submitted,
            filled_quantity: Quantity::zero(8),
            avg_fill_price: None,
            created_at: 100,
            updated_at: 200,
        };

        store.save_order(&order).unwrap();
        let loaded = store.load_open_orders().unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].client_order_id.as_str(), "EXE-001");
        assert_eq!(loaded[0].venue_order_id.as_ref().unwrap().as_str(), "PAPER-1");
    }

    #[test]
    fn persistence_filled_orders_not_loaded_as_open() {
        let store = StateStore::open_memory().unwrap();

        let order = OrderState {
            client_order_id: ClientOrderId::new("EXE-002"),
            venue_order_id: None,
            instrument_id: InstrumentId::from_parts("BTCUSDT", "BINANCE"),
            side: OrderSide::Sell,
            order_type: OrderType::Market,
            quantity: Quantity::from_f64(1.0, 8),
            limit_price: None,
            status: OrderStatus::Filled,
            filled_quantity: Quantity::from_f64(1.0, 8),
            avg_fill_price: Some(Price::from_f64(50_000.0, 2)),
            created_at: 100,
            updated_at: 300,
        };

        store.save_order(&order).unwrap();
        let loaded = store.load_open_orders().unwrap();
        assert_eq!(loaded.len(), 0);
    }

    #[test]
    fn persistence_portfolio_roundtrip() {
        let store = StateStore::open_memory().unwrap();

        store.save_portfolio(100_000_000_000_000, 2, 80_000_000_000_000, 2, 999).unwrap();
        let loaded = store.load_portfolio().unwrap();
        assert!(loaded.is_some());
        let (eq_raw, eq_prec, cap_raw, cap_prec, ts) = loaded.unwrap();
        assert_eq!(eq_raw, 100_000_000_000_000);
        assert_eq!(eq_prec, 2);
        assert_eq!(cap_raw, 80_000_000_000_000);
        assert_eq!(cap_prec, 2);
        assert_eq!(ts, 999);
    }

    #[test]
    fn persistence_cycle_roundtrip() {
        let store = StateStore::open_memory().unwrap();

        store.save_cycle("cycle-abc", "Scan", "BTCUSDT,ETHUSDT", 12345).unwrap();
        let loaded = store.load_cycle().unwrap();
        assert!(loaded.is_some());
        let (cid, stage, focus, ts) = loaded.unwrap();
        assert_eq!(cid, "cycle-abc");
        assert_eq!(stage, "Scan");
        assert_eq!(focus, "BTCUSDT,ETHUSDT");
        assert_eq!(ts, 12345);
    }

    #[test]
    fn persistence_audit_trail() {
        let store = StateStore::open_memory().unwrap();
        store.save_audit("paper_fill", "BTCUSDT buy 0.5 @ 50000", 9999).unwrap();
        // Just verify it doesn't error; audit is write-mostly.
    }
}
