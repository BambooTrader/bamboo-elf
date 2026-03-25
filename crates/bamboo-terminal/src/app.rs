use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::VecDeque;

use bamboo_core::{BusMessage, Payload};

use crate::widgets::{
    AgentStatus, LogEntry, PanelId, PositionRow, WatchlistState, MAX_LOG_ENTRIES,
};

/// Tab names displayed in the UI.
pub const TAB_NAMES: &[&str] = &["Market", "Portfolio", "Agents", "Logs"];

pub struct App {
    pub watchlist: WatchlistState,
    pub positions: Vec<PositionRow>,
    pub agents: Vec<AgentStatus>,
    pub events: VecDeque<LogEntry>,
    pub news_items: Vec<String>,
    pub active_tab: usize,
    pub focused_panel: PanelId,
    pub should_quit: bool,
    pub log_scroll: usize,
    pub news_scroll: usize,
}

impl App {
    pub fn new(symbols: &[String], sparkline_window: usize) -> Self {
        let agents = vec![
            AgentStatus::new("Research", "idle"),
            AgentStatus::new("Strategy", "idle"),
            AgentStatus::new("Portfolio", "idle"),
            AgentStatus::new("Risk", "ok"),
            AgentStatus::new("Execution", "ready"),
        ];

        Self {
            watchlist: WatchlistState::new(symbols, sparkline_window),
            positions: Vec::new(),
            agents,
            events: VecDeque::with_capacity(MAX_LOG_ENTRIES),
            news_items: Vec::new(),
            active_tab: 0,
            focused_panel: PanelId::Watchlist,
            should_quit: false,
            log_scroll: 0,
            news_scroll: 0,
        }
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
            }
            Payload::ExecutionReport(_) => {
                // Execution reports are logged but no special UI update yet
            }
            Payload::StrategySignal(_) => {
                self.update_agent_status("Strategy", "running");
            }
            Payload::ResearchFinding(_) => {
                self.update_agent_status("Research", "running");
            }
            Payload::RiskDecision(_) => {
                self.update_agent_status("Risk", "ok");
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
            format!("{}", news.title)
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
    }
}
