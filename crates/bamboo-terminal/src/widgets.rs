use bamboo_core::{FeedStatus, MarketTick, Topic};
use ratatui::style::Color;
use std::collections::VecDeque;

/// Maximum number of log entries kept in the ring buffer.
pub const MAX_LOG_ENTRIES: usize = 500;

/// Maximum number of sparkline data points (default for WatchlistEntry).
#[allow(dead_code)]
pub const MAX_SPARKLINE_POINTS: usize = 120;

// ── Panel focus ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelId {
    Watchlist,
    News,
    Positions,
    Agents,
    EventLog,
}

impl PanelId {
    /// Cycle to the next panel (left-to-right, top-to-bottom).
    pub fn next(self) -> Self {
        match self {
            PanelId::Watchlist => PanelId::News,
            PanelId::News => PanelId::Positions,
            PanelId::Positions => PanelId::Agents,
            PanelId::Agents => PanelId::EventLog,
            PanelId::EventLog => PanelId::Watchlist,
        }
    }

    /// Cycle to the previous panel.
    pub fn prev(self) -> Self {
        match self {
            PanelId::Watchlist => PanelId::EventLog,
            PanelId::News => PanelId::Watchlist,
            PanelId::Positions => PanelId::News,
            PanelId::Agents => PanelId::Positions,
            PanelId::EventLog => PanelId::Agents,
        }
    }
}

// ── Watchlist ──

#[derive(Debug, Clone)]
pub struct WatchlistEntry {
    pub instrument_id: String,
    pub last_price: f64,
    pub change_pct: f64,
    pub sparkline_data: VecDeque<f64>,
    pub feed_status: FeedStatus,
    first_price: Option<f64>,
}

impl WatchlistEntry {
    pub fn new(instrument_id: String, sparkline_capacity: usize) -> Self {
        Self {
            instrument_id,
            last_price: 0.0,
            change_pct: 0.0,
            sparkline_data: VecDeque::with_capacity(sparkline_capacity),
            feed_status: FeedStatus::Disconnected,
            first_price: None,
        }
    }

    /// Update from a MarketTick.
    pub fn update_from_tick(&mut self, tick: &MarketTick, max_sparkline: usize) {
        let price = tick.last.as_f64();
        self.last_price = price;
        self.feed_status = FeedStatus::Connected;

        if self.first_price.is_none() {
            self.first_price = Some(price);
        }

        // Calculate change percentage from first observed price
        if let Some(first) = self.first_price {
            if first > 0.0 {
                self.change_pct = ((price - first) / first) * 100.0;
            }
        }

        // Push to sparkline ring buffer
        if self.sparkline_data.len() >= max_sparkline {
            self.sparkline_data.pop_front();
        }
        self.sparkline_data.push_back(price);
    }
}

#[derive(Debug, Clone)]
pub struct WatchlistState {
    pub entries: Vec<WatchlistEntry>,
    pub selected: usize,
    pub sparkline_window: usize,
}

impl WatchlistState {
    pub fn new(symbols: &[String], sparkline_window: usize) -> Self {
        let entries = symbols
            .iter()
            .map(|s| {
                let instrument_id = format!("{s}.BINANCE");
                WatchlistEntry::new(instrument_id, sparkline_window)
            })
            .collect();
        Self {
            entries,
            selected: 0,
            sparkline_window,
        }
    }

    /// Find and update the matching entry from a tick.
    pub fn handle_tick(&mut self, tick: &MarketTick) {
        let id_str = tick.instrument_id.to_string();
        if let Some(entry) = self.entries.iter_mut().find(|e| e.instrument_id == id_str) {
            entry.update_from_tick(tick, self.sparkline_window);
        }
    }

    pub fn scroll_down(&mut self) {
        if !self.entries.is_empty() {
            self.selected = (self.selected + 1) % self.entries.len();
        }
    }

    pub fn scroll_up(&mut self) {
        if !self.entries.is_empty() {
            self.selected = self.selected.checked_sub(1).unwrap_or(self.entries.len() - 1);
        }
    }
}

// ── Positions ──

#[derive(Debug, Clone)]
pub struct PositionRow {
    pub instrument_id: String,
    pub side: String,
    pub quantity: String,
    pub entry_price: String,
    pub current_price: String,
    pub pnl: String,
    pub pnl_pct: f64,
}

// ── Portfolio summary ──

#[derive(Debug, Clone)]
pub struct PortfolioSummary {
    pub total_capital: f64,
    pub available_capital: f64,
    pub total_exposure: f64,
    pub total_pnl: f64,
    pub total_pnl_pct: f64,
}

impl Default for PortfolioSummary {
    fn default() -> Self {
        Self {
            total_capital: 0.0,
            available_capital: 0.0,
            total_exposure: 0.0,
            total_pnl: 0.0,
            total_pnl_pct: 0.0,
        }
    }
}

// ── Agent status ──

#[derive(Debug, Clone)]
pub struct AgentStatus {
    pub name: String,
    pub status: String,
    pub is_active: bool,
    pub last_action: String,
    pub message_count: u32,
}

impl AgentStatus {
    pub fn new(name: &str, status: &str) -> Self {
        Self {
            name: name.to_string(),
            status: status.to_string(),
            is_active: false,
            last_action: String::new(),
            message_count: 0,
        }
    }

    pub fn status_color(&self) -> Color {
        match self.status.as_str() {
            "Running" | "running" => Color::Green,
            "Idle" | "idle" | "ok" | "ready" => Color::Green,
            "Starting" => Color::Yellow,
            "Error" | "error" => Color::Red,
            "Stopped" => Color::DarkGray,
            _ => Color::White,
        }
    }
}

// ── Agent detail (rich view for Agents tab) ──

#[derive(Debug, Clone)]
pub struct AgentDetail {
    pub name: String,
    pub status: String,
    pub last_action: String,
    pub message_count: u32,
    pub is_active: bool,
}

impl AgentDetail {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            status: "Idle".to_string(),
            last_action: String::new(),
            message_count: 0,
            is_active: false,
        }
    }

    pub fn status_color(&self) -> Color {
        match self.status.as_str() {
            "Running" => Color::Green,
            "Idle" => Color::Green,
            "Starting" => Color::Yellow,
            "Error" => Color::Red,
            "Stopped" => Color::DarkGray,
            _ => Color::White,
        }
    }
}

// ── Order history ──

/// A single row in the order history table (Spec 3: Execution).
#[derive(Debug, Clone)]
pub struct OrderRow {
    pub client_order_id: String,
    pub instrument: String,
    pub side: String,
    pub order_type: String,
    pub quantity: String,
    pub status: String,
    pub fill_price: String,
    pub time: String,
}

/// Maximum number of order history entries kept (ring buffer).
pub const MAX_ORDER_HISTORY: usize = 100;

// ── Log entries ──

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub topic: String,
    pub message: String,
    pub topic_color: Color,
}

impl LogEntry {
    pub fn new(topic: Topic, message: String) -> Self {
        let now = chrono::Local::now();
        Self {
            timestamp: now.format("%H:%M:%S").to_string(),
            topic: format!("{topic:?}"),
            message,
            topic_color: topic_to_color(topic),
        }
    }
}

/// Map a bus Topic to a display color.
pub fn topic_to_color(topic: Topic) -> Color {
    match topic {
        Topic::MarketData => Color::Cyan,
        Topic::News => Color::Yellow,
        Topic::Signal => Color::Magenta,
        Topic::Intent => Color::Blue,
        Topic::Risk => Color::Red,
        Topic::Execution => Color::Green,
        Topic::System => Color::White,
    }
}
