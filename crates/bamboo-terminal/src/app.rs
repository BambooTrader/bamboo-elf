use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::VecDeque;

use bamboo_core::{BusMessage, Payload};

use crate::widgets::{
    AgentDetail, AgentStatus, LogEntry, OrderRow, PanelId, PortfolioSummary, PositionRow,
    WatchlistState, MAX_LOG_ENTRIES, MAX_ORDER_HISTORY,
};

/// Tab names displayed in the UI.
pub const TAB_NAMES: &[&str] = &["Market", "Portfolio", "Agents", "Logs"];

pub struct App {
    pub watchlist: WatchlistState,
    pub positions: Vec<PositionRow>,
    pub agents: Vec<AgentStatus>,
    pub agent_details: Vec<AgentDetail>,
    pub events: VecDeque<LogEntry>,
    pub news_items: Vec<String>,
    pub active_tab: usize,
    pub focused_panel: PanelId,
    pub should_quit: bool,
    pub log_scroll: usize,
    pub news_scroll: usize,
    // Spec 2: Cycle state
    pub cycle_stage: String,
    pub cycle_focus_set: Vec<String>,
    // Spec 2: Portfolio summary
    pub portfolio_summary: PortfolioSummary,
    // Spec 3: Execution + Live Trading
    pub trading_mode: String,
    pub safe_mode_active: bool,
    pub safe_mode_reason: String,
    pub order_history: VecDeque<OrderRow>,
}

impl App {
    pub fn new(
        symbols: &[String],
        sparkline_window: usize,
        trading_mode: String,
    ) -> Self {
        let agents = vec![
            AgentStatus::new("CycleManager", "Idle"),
            AgentStatus::new("Research", "Idle"),
            AgentStatus::new("Strategy", "Idle"),
            AgentStatus::new("Portfolio", "Idle"),
            AgentStatus::new("Risk", "Idle"),
            AgentStatus::new("Execution", "Idle"),
        ];

        let agent_details = vec![
            AgentDetail::new("CycleManager"),
            AgentDetail::new("Research"),
            AgentDetail::new("Strategy"),
            AgentDetail::new("Portfolio"),
            AgentDetail::new("Risk"),
            AgentDetail::new("Execution"),
        ];

        Self {
            watchlist: WatchlistState::new(symbols, sparkline_window),
            positions: Vec::new(),
            agents,
            agent_details,
            events: VecDeque::with_capacity(MAX_LOG_ENTRIES),
            news_items: Vec::new(),
            active_tab: 0,
            focused_panel: PanelId::Watchlist,
            should_quit: false,
            log_scroll: 0,
            news_scroll: 0,
            cycle_stage: "Idle".to_string(),
            cycle_focus_set: Vec::new(),
            portfolio_summary: PortfolioSummary::default(),
            trading_mode,
            safe_mode_active: false,
            safe_mode_reason: String::new(),
            order_history: VecDeque::with_capacity(MAX_ORDER_HISTORY),
        }
    }

    /// Initialize portfolio summary from config values.
    pub fn init_portfolio(&mut self, initial_capital: f64) {
        self.portfolio_summary.total_capital = initial_capital;
        self.portfolio_summary.available_capital = initial_capital;
    }

    /// Dispatch a bus message to the appropriate state update handler.
    pub fn handle_bus_message(&mut self, msg: BusMessage) {
        // Always add to event log
        let log_msg = format_bus_message(&msg);
        self.push_log(LogEntry::new(msg.topic, log_msg));

        match msg.payload {
            Payload::MarketTick(tick) => {
                self.watchlist.handle_tick(&tick);
            }
            Payload::NewsItem(news) => {
                let formatted = format!("{} [{}]", news.title, news.source);
                self.news_items.push(formatted);
                // Keep max 100 news items
                if self.news_items.len() > 100 {
                    self.news_items.remove(0);
                }
            }
            Payload::PositionUpdate(pos) => {
                let pnl_val = pos
                    .unrealized_pnl
                    .as_ref()
                    .map(|m| m.amount.as_f64())
                    .unwrap_or(0.0);
                let entry_price = pos.avg_entry_price.as_f64();
                let pnl_pct = if entry_price > 0.0 {
                    (pnl_val / entry_price) * 100.0
                } else {
                    0.0
                };

                let row = PositionRow {
                    instrument_id: pos.instrument_id.to_string(),
                    side: format!("{:?}", pos.side),
                    quantity: pos.quantity.to_string(),
                    entry_price: format!("{:.2}", entry_price),
                    current_price: String::new(), // updated on next tick
                    pnl: format!("{:+.2}", pnl_val),
                    pnl_pct,
                };

                // Update existing or add new
                if let Some(existing) = self
                    .positions
                    .iter_mut()
                    .find(|p| p.instrument_id == row.instrument_id)
                {
                    *existing = row;
                } else {
                    self.positions.push(row);
                }

                // Recalculate portfolio summary from positions
                self.recalculate_portfolio_summary();
            }
            Payload::ExecutionReport(er) => {
                // Update agent status on execution reports
                let status_str = format!("{:?}", er.status);
                self.update_agent_status("Execution", "Running");
                self.update_agent_detail(
                    "Execution",
                    &format!(
                        "{} {:?} {:?} qty={}",
                        er.instrument_id, er.side, er.status, er.filled_quantity
                    ),
                );

                // Add to order history (ring buffer)
                let fill_price = er
                    .avg_fill_price
                    .map(|p| format!("{:.2}", p.as_f64()))
                    .unwrap_or_else(|| "-".to_string());
                let row = OrderRow {
                    client_order_id: er.client_order_id.to_string(),
                    instrument: er.instrument_id.to_string(),
                    side: format!("{:?}", er.side),
                    order_type: "-".to_string(), // not in ExecutionReport
                    quantity: er.filled_quantity.to_string(),
                    status: status_str,
                    fill_price,
                    time: chrono::Local::now().format("%H:%M:%S").to_string(),
                };
                if self.order_history.len() >= MAX_ORDER_HISTORY {
                    self.order_history.pop_front();
                }
                self.order_history.push_back(row);
            }
            Payload::StrategySignal(sig) => {
                self.update_agent_status("Strategy", "Running");
                self.update_agent_detail(
                    "Strategy",
                    &format!("{} {:?} conf={:.2}", sig.instrument_id, sig.side, sig.confidence),
                );
            }
            Payload::ResearchFinding(rf) => {
                self.update_agent_status("Research", "Running");
                self.update_agent_detail(
                    "Research",
                    &format!("{} score={:.2}", rf.instrument_id, rf.score),
                );
            }
            Payload::RiskDecision(rd) => {
                let verdict = if rd.approved { "APPROVED" } else { "REJECTED" };
                self.update_agent_status("Risk", "Running");
                self.update_agent_detail("Risk", &format!("{verdict}: {}", rd.reason));
            }
            Payload::PortfolioIntent(pi) => {
                self.update_agent_status("Portfolio", "Running");
                self.update_agent_detail(
                    "Portfolio",
                    &format!("{} {:?} qty={}", pi.instrument_id, pi.side, pi.quantity),
                );
            }
            Payload::CycleStageChanged(c) => {
                self.cycle_stage = format!("{:?}", c.new_stage);
                self.cycle_focus_set = c
                    .focus_set
                    .iter()
                    .map(|id| id.to_string())
                    .collect();
                self.update_agent_status("CycleManager", "Running");
                self.update_agent_detail(
                    "CycleManager",
                    &format!("Stage -> {:?}", c.new_stage),
                );
            }
            Payload::SignalOutcome(so) => {
                // Spec 3: Log signal outcome and update strategy agent status
                let pnl_str = so
                    .pnl
                    .as_ref()
                    .map(|m| format!("pnl={:+.2}", m.amount.as_f64()))
                    .unwrap_or_else(|| "open".to_string());
                self.update_agent_detail(
                    "Strategy",
                    &format!("{} {:?} {}", so.instrument_id, so.status, pnl_str),
                );
            }
            Payload::EmergencyAction(ea) => {
                // Spec 3: Enter safe mode on emergency actions
                self.safe_mode_active = true;
                self.safe_mode_reason = ea.reason.clone();
                self.update_agent_status("Risk", "Error");
                self.update_agent_detail(
                    "Risk",
                    &format!("EMERGENCY {:?}: {}", ea.action_type, ea.reason),
                );
            }
            Payload::AgentHeartbeat(h) => {
                let status_str = format!("{:?}", h.status);
                // Update both agent views
                self.update_agent_status(&h.agent_name, &status_str);
                if let Some(detail) = self
                    .agent_details
                    .iter_mut()
                    .find(|d| d.name == h.agent_name)
                {
                    detail.status = status_str;
                    detail.message_count += 1;
                    detail.is_active = true;
                    if let Some(action) = &h.last_action {
                        detail.last_action = action.clone();
                    }
                }
            }
            _ => {}
        }
    }

    /// Handle a keyboard event.
    pub fn handle_key_event(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            // Tab navigation
            KeyCode::Tab => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.active_tab = if self.active_tab == 0 {
                        TAB_NAMES.len() - 1
                    } else {
                        self.active_tab - 1
                    };
                } else {
                    self.active_tab = (self.active_tab + 1) % TAB_NAMES.len();
                }
            }
            KeyCode::Char('1') => self.active_tab = 0,
            KeyCode::Char('2') => self.active_tab = 1,
            KeyCode::Char('3') => self.active_tab = 2,
            KeyCode::Char('4') => self.active_tab = 3,
            // Scrolling within focused panel
            KeyCode::Char('j') | KeyCode::Down => self.scroll_focused_down(),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_focused_up(),
            // Panel focus with Ctrl+arrows
            KeyCode::Right if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.focused_panel = self.focused_panel.next();
            }
            KeyCode::Left if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.focused_panel = self.focused_panel.prev();
            }
            _ => {}
        }
    }

    fn scroll_focused_down(&mut self) {
        match self.focused_panel {
            PanelId::Watchlist => self.watchlist.scroll_down(),
            PanelId::EventLog => {
                if self.log_scroll < self.events.len().saturating_sub(1) {
                    self.log_scroll += 1;
                }
            }
            PanelId::News => {
                if self.news_scroll < self.news_items.len().saturating_sub(1) {
                    self.news_scroll += 1;
                }
            }
            _ => {}
        }
    }

    fn scroll_focused_up(&mut self) {
        match self.focused_panel {
            PanelId::Watchlist => self.watchlist.scroll_up(),
            PanelId::EventLog => {
                self.log_scroll = self.log_scroll.saturating_sub(1);
            }
            PanelId::News => {
                self.news_scroll = self.news_scroll.saturating_sub(1);
            }
            _ => {}
        }
    }

    fn push_log(&mut self, entry: LogEntry) {
        if self.events.len() >= MAX_LOG_ENTRIES {
            self.events.pop_front();
        }
        self.events.push_back(entry);
    }

    fn update_agent_status(&mut self, name: &str, status: &str) {
        if let Some(agent) = self.agents.iter_mut().find(|a| a.name == name) {
            agent.status = status.to_string();
            agent.is_active = true;
            agent.message_count += 1;
        }
    }

    fn update_agent_detail(&mut self, name: &str, action: &str) {
        if let Some(detail) = self.agent_details.iter_mut().find(|d| d.name == name) {
            detail.last_action = action.to_string();
            detail.message_count += 1;
            detail.is_active = true;
        }
        // Also update the simple agent status last_action
        if let Some(agent) = self.agents.iter_mut().find(|a| a.name == name) {
            agent.last_action = action.to_string();
        }
    }

    /// Recalculate portfolio summary from current positions.
    fn recalculate_portfolio_summary(&mut self) {
        let mut total_exposure = 0.0_f64;
        let mut total_pnl = 0.0_f64;

        for pos in &self.positions {
            // Parse pnl from the formatted string
            if let Ok(pnl) = pos.pnl.parse::<f64>() {
                total_pnl += pnl;
            }
            // Estimate exposure from quantity * entry_price
            let qty: f64 = pos.quantity.parse().unwrap_or(0.0);
            let entry: f64 = pos.entry_price.parse().unwrap_or(0.0);
            total_exposure += qty * entry;
        }

        self.portfolio_summary.total_exposure = total_exposure;
        self.portfolio_summary.total_pnl = total_pnl;
        self.portfolio_summary.available_capital =
            self.portfolio_summary.total_capital - total_exposure;

        if self.portfolio_summary.total_capital > 0.0 {
            self.portfolio_summary.total_pnl_pct =
                (total_pnl / self.portfolio_summary.total_capital) * 100.0;
        }
    }
}

/// Format a BusMessage into a human-readable log string.
fn format_bus_message(msg: &BusMessage) -> String {
    match &msg.payload {
        Payload::MarketTick(tick) => {
            format!("{} tick {:.2}", tick.instrument_id, tick.last.as_f64())
        }
        Payload::NewsItem(news) => {
            news.title.clone()
        }
        Payload::StrategySignal(sig) => {
            format!(
                "{} {:?} signal conf={:.2}",
                sig.instrument_id, sig.side, sig.confidence
            )
        }
        Payload::ResearchFinding(rf) => {
            format!("{} score={:.2}", rf.instrument_id, rf.score)
        }
        Payload::RiskDecision(rd) => {
            let verdict = if rd.approved { "APPROVED" } else { "REJECTED" };
            format!("{verdict}: {}", rd.reason)
        }
        Payload::ExecutionReport(er) => {
            format!(
                "{} {:?} {:?}",
                er.instrument_id, er.side, er.status
            )
        }
        Payload::PositionUpdate(pu) => {
            format!(
                "{} {:?} qty={}",
                pu.instrument_id, pu.side, pu.quantity
            )
        }
        Payload::KlineBar(bar) => {
            format!(
                "{} {:?} O={} C={}",
                bar.instrument_id, bar.interval, bar.open, bar.close
            )
        }
        Payload::PortfolioIntent(pi) => {
            format!("{} {:?} qty={}", pi.instrument_id, pi.side, pi.quantity)
        }
        Payload::ExecutionOrderIntent(eoi) => {
            format!(
                "{} {:?} {:?} qty={}",
                eoi.instrument_id, eoi.side, eoi.order_type, eoi.quantity
            )
        }
        Payload::CycleSummary(cs) => {
            format!(
                "Cycle {:?} signals={} trades={}",
                cs.stage_completed, cs.signals_generated, cs.trades_executed
            )
        }
        Payload::EmergencyAction(ea) => {
            format!("{:?}: {}", ea.action_type, ea.reason)
        }
        Payload::CycleStageChanged(c) => {
            let focus_str: Vec<String> = c.focus_set.iter().map(|id| id.to_string()).collect();
            format!(
                "Stage -> {:?} focus=[{}]",
                c.new_stage,
                focus_str.join(", ")
            )
        }
        Payload::AgentHeartbeat(h) => {
            format!(
                "{} {:?} action={:?}",
                h.agent_name, h.status, h.last_action
            )
        }
        Payload::SignalOutcome(so) => {
            let pnl_str = so
                .pnl
                .as_ref()
                .map(|m| format!("pnl={:.2}", m.amount.as_f64()))
                .unwrap_or_else(|| "pnl=n/a".to_string());
            format!(
                "{} {:?} {}",
                so.instrument_id, so.status, pnl_str
            )
        }
    }
}
