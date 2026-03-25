use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Sparkline, Table, Tabs},
    Frame,
};

use crate::app::{App, TAB_NAMES};
use crate::widgets::PanelId;

/// Main render dispatch.
pub fn render(frame: &mut Frame, app: &mut App) {
    let size = frame.area();

    match app.active_tab {
        0 => render_market_tab(frame, size, app),
        1 => render_portfolio_tab(frame, size, app),
        2 => render_agents_tab(frame, size, app),
        3 => render_logs_tab(frame, size, app),
        _ => render_market_tab(frame, size, app),
    }
}

/// Market tab: 5-panel layout with tabs bar and cycle status.
fn render_market_tab(frame: &mut Frame, area: Rect, app: &mut App) {
    // Vertical split: tabs | cycle status | middle | lower | event log
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),       // tabs
            Constraint::Length(1),       // cycle status bar
            Constraint::Percentage(35), // middle (watchlist + news)
            Constraint::Percentage(30), // lower (positions + agents)
            Constraint::Min(6),         // event log
        ])
        .split(area);

    // Render tabs bar
    render_tabs(frame, main_chunks[0], app);

    // Render cycle status bar
    render_cycle_status_bar(frame, main_chunks[1], app);

    // Middle row: watchlist (left) | news (right)
    let middle_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_chunks[2]);

    render_watchlist(frame, middle_chunks[0], app);
    render_news(frame, middle_chunks[1], app);

    // Lower row: positions (left) | agent status (right)
    let lower_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_chunks[3]);

    render_positions(frame, lower_chunks[0], app);
    render_agent_status(frame, lower_chunks[1], app);

    // Bottom: event log
    render_event_log(frame, main_chunks[4], app);
}

/// Cycle status bar: shows current cycle stage and focus set.
fn render_cycle_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let focus_str = if app.cycle_focus_set.is_empty() {
        "none".to_string()
    } else {
        // Strip .BINANCE suffix for display
        app.cycle_focus_set
            .iter()
            .map(|s| s.split('.').next().unwrap_or(s).to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };

    let stage_color = match app.cycle_stage.as_str() {
        "Scan" => Color::Yellow,
        "Focus" => Color::Green,
        "Review" => Color::Cyan,
        _ => Color::DarkGray,
    };

    let line = Line::from(vec![
        Span::styled(" Cycle: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            &app.cycle_stage,
            Style::default()
                .fg(stage_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" | Focus Set: ", Style::default().fg(Color::DarkGray)),
        Span::styled(focus_str, Style::default().fg(Color::White)),
    ]);

    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

/// Tabs bar.
fn render_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = TAB_NAMES
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let style = if i == app.active_tab {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            Line::from(Span::styled(format!(" {t} "), style))
        })
        .collect();

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Bamboo Elf "),
        )
        .select(app.active_tab)
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_widget(tabs, area);
}

/// Watchlist panel: table with sparklines in last column.
/// Rows in the focus set are highlighted bold/bright.
fn render_watchlist(frame: &mut Frame, area: Rect, app: &App) {
    let is_focused = app.focused_panel == PanelId::Watchlist;
    let border_style = if is_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    // Split the area: table on left, leave room for sparkline rendering
    let inner_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(area);

    let header = Row::new(vec![
        Cell::from("Symbol"),
        Cell::from("Price"),
        Cell::from("Change"),
    ])
    .style(
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .watchlist
        .entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let symbol = entry
                .instrument_id
                .split('.')
                .next()
                .unwrap_or(&entry.instrument_id);
            let price_str = format!("{:.2}", entry.last_price);
            let change_str = format!("{:+.1}%", entry.change_pct);
            let change_color = if entry.change_pct >= 0.0 {
                Color::Green
            } else {
                Color::Red
            };

            // Check if this symbol is in the focus set
            let in_focus = app
                .cycle_focus_set
                .iter()
                .any(|fs| entry.instrument_id == *fs);

            let (symbol_style, row_style) = if i == app.watchlist.selected {
                if in_focus {
                    (
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                        Style::default().bg(Color::DarkGray),
                    )
                } else {
                    (
                        Style::default().fg(Color::White),
                        Style::default().bg(Color::DarkGray),
                    )
                }
            } else if in_focus {
                (
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                    Style::default(),
                )
            } else {
                (Style::default().fg(Color::White), Style::default())
            };

            Row::new(vec![
                Cell::from(symbol.to_string()).style(symbol_style),
                Cell::from(price_str).style(Style::default().fg(Color::White)),
                Cell::from(change_str).style(Style::default().fg(change_color)),
            ])
            .style(row_style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(40),
            Constraint::Percentage(35),
            Constraint::Percentage(25),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" Watchlist "),
    );

    frame.render_widget(table, inner_chunks[0]);

    // Render sparklines in the right portion
    let sparkline_block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(" Trend ");
    let sparkline_inner = sparkline_block.inner(inner_chunks[1]);
    frame.render_widget(sparkline_block, inner_chunks[1]);

    if !app.watchlist.entries.is_empty() && sparkline_inner.height > 0 {
        let row_height = std::cmp::max(
            1,
            sparkline_inner.height as usize / app.watchlist.entries.len().max(1),
        );

        for (i, entry) in app.watchlist.entries.iter().enumerate() {
            let y_offset = i as u16 * row_height as u16;
            if y_offset >= sparkline_inner.height {
                break;
            }

            let spark_area = Rect {
                x: sparkline_inner.x,
                y: sparkline_inner.y + y_offset,
                width: sparkline_inner.width,
                height: std::cmp::min(row_height as u16, sparkline_inner.height - y_offset),
            };

            if !entry.sparkline_data.is_empty() {
                // Convert f64 to u64 for sparkline (normalize to 0-100 range)
                let data: Vec<u64> = normalize_sparkline(&entry.sparkline_data);
                let color = if entry.change_pct >= 0.0 {
                    Color::Green
                } else {
                    Color::Red
                };

                let sparkline = Sparkline::default()
                    .data(&data)
                    .style(Style::default().fg(color));

                frame.render_widget(sparkline, spark_area);
            }
        }
    }
}

/// Normalize sparkline data from f64 prices to u64 range suitable for display.
fn normalize_sparkline(data: &std::collections::VecDeque<f64>) -> Vec<u64> {
    if data.is_empty() {
        return vec![];
    }
    let min = data.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let range = max - min;

    if range < 1e-10 {
        return data.iter().map(|_| 50u64).collect();
    }

    data.iter()
        .map(|v| (((v - min) / range) * 100.0) as u64)
        .collect()
}

/// News feed panel.
fn render_news(frame: &mut Frame, area: Rect, app: &App) {
    let is_focused = app.focused_panel == PanelId::News;
    let border_style = if is_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let items: Vec<ListItem> = app
        .news_items
        .iter()
        .rev()
        .take(50)
        .map(|item| {
            ListItem::new(Line::from(Span::styled(
                item.clone(),
                Style::default().fg(Color::White),
            )))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" News Feed "),
    );

    frame.render_widget(list, area);
}

/// Positions panel (compact view for Market tab).
fn render_positions(frame: &mut Frame, area: Rect, app: &App) {
    let is_focused = app.focused_panel == PanelId::Positions;
    let border_style = if is_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let header = Row::new(vec![
        Cell::from("Instrument"),
        Cell::from("Side"),
        Cell::from("Qty"),
        Cell::from("P&L"),
        Cell::from("P&L %"),
    ])
    .style(
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .positions
        .iter()
        .map(|pos| {
            let pnl_color = if pos.pnl_pct >= 0.0 {
                Color::Green
            } else {
                Color::Red
            };
            let symbol = pos
                .instrument_id
                .split('.')
                .next()
                .unwrap_or(&pos.instrument_id);
            Row::new(vec![
                Cell::from(symbol.to_string()),
                Cell::from(pos.side.clone()),
                Cell::from(pos.quantity.clone()),
                Cell::from(pos.pnl.clone()).style(Style::default().fg(pnl_color)),
                Cell::from(format!("{:+.1}%", pos.pnl_pct))
                    .style(Style::default().fg(pnl_color)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(25),
            Constraint::Percentage(15),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" Positions "),
    );

    frame.render_widget(table, area);
}

/// Agent status panel (compact view for Market tab).
fn render_agent_status(frame: &mut Frame, area: Rect, app: &App) {
    let is_focused = app.focused_panel == PanelId::Agents;
    let border_style = if is_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let header = Row::new(vec![Cell::from("Agent"), Cell::from("Status")]).style(
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .agents
        .iter()
        .map(|agent| {
            let status_color = agent.status_color();
            let dot = if agent.is_active { "* " } else { "  " };
            Row::new(vec![
                Cell::from(agent.name.clone()).style(Style::default().fg(Color::White)),
                Cell::from(format!("{dot}{}", agent.status))
                    .style(Style::default().fg(status_color)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [Constraint::Percentage(50), Constraint::Percentage(50)],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" Agent Status "),
    );

    frame.render_widget(table, area);
}

/// Event log panel.
fn render_event_log(frame: &mut Frame, area: Rect, app: &App) {
    let is_focused = app.focused_panel == PanelId::EventLog;
    let border_style = if is_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let items: Vec<ListItem> = app
        .events
        .iter()
        .rev()
        .take(100)
        .map(|entry| {
            let line = Line::from(vec![
                Span::styled(
                    format!("{} ", entry.timestamp),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("[{}] ", entry.topic),
                    Style::default().fg(entry.topic_color),
                ),
                Span::styled(entry.message.clone(), Style::default().fg(Color::White)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" Event Log (subscribe_all) "),
    );

    frame.render_widget(list, area);
}

/// Portfolio tab: summary box + detailed positions table.
fn render_portfolio_tab(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // tabs
            Constraint::Length(5),  // portfolio summary
            Constraint::Min(0),    // positions table
        ])
        .split(area);

    render_tab_bar_only(frame, chunks[0], 1);

    // Portfolio summary box
    render_portfolio_summary(frame, chunks[1], app);

    // Detailed positions table
    render_portfolio_positions(frame, chunks[2], app);
}

/// Render the portfolio summary box.
fn render_portfolio_summary(frame: &mut Frame, area: Rect, app: &App) {
    let ps = &app.portfolio_summary;

    let pnl_color = if ps.total_pnl >= 0.0 {
        Color::Green
    } else {
        Color::Red
    };

    let line1 = Line::from(vec![
        Span::styled(
            format!(" Total Capital: ${:.2}", ps.total_capital),
            Style::default().fg(Color::White),
        ),
        Span::styled("  |  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("Available: ${:.2}", ps.available_capital),
            Style::default().fg(Color::Cyan),
        ),
    ]);

    let line2 = Line::from(vec![
        Span::styled(
            format!(" Exposure: ${:.2}", ps.total_exposure),
            Style::default().fg(Color::Yellow),
        ),
        Span::styled("  |  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("P&L: ${:+.2} ({:+.2}%)", ps.total_pnl, ps.total_pnl_pct),
            Style::default().fg(pnl_color),
        ),
    ]);

    let text = vec![line1, line2];
    let paragraph = Paragraph::new(text).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Portfolio Summary "),
    );

    frame.render_widget(paragraph, area);
}

/// Render the full positions table for the Portfolio tab (with more columns).
fn render_portfolio_positions(frame: &mut Frame, area: Rect, app: &App) {
    let header = Row::new(vec![
        Cell::from("Instrument"),
        Cell::from("Side"),
        Cell::from("Qty"),
        Cell::from("Entry Price"),
        Cell::from("Current"),
        Cell::from("P&L"),
        Cell::from("P&L %"),
    ])
    .style(
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .positions
        .iter()
        .map(|pos| {
            let pnl_color = if pos.pnl_pct >= 0.0 {
                Color::Green
            } else {
                Color::Red
            };
            let symbol = pos
                .instrument_id
                .split('.')
                .next()
                .unwrap_or(&pos.instrument_id);

            // Try to get current price from watchlist
            let current_price = app
                .watchlist
                .entries
                .iter()
                .find(|e| e.instrument_id == pos.instrument_id)
                .map(|e| format!("{:.2}", e.last_price))
                .unwrap_or_else(|| pos.current_price.clone());

            Row::new(vec![
                Cell::from(symbol.to_string()),
                Cell::from(pos.side.clone()),
                Cell::from(pos.quantity.clone()),
                Cell::from(pos.entry_price.clone()),
                Cell::from(current_price),
                Cell::from(pos.pnl.clone()).style(Style::default().fg(pnl_color)),
                Cell::from(format!("{:+.2}%", pos.pnl_pct))
                    .style(Style::default().fg(pnl_color)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(16),
            Constraint::Percentage(10),
            Constraint::Percentage(12),
            Constraint::Percentage(16),
            Constraint::Percentage(16),
            Constraint::Percentage(15),
            Constraint::Percentage(15),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Positions Detail "),
    );

    frame.render_widget(table, area);
}

/// Agents tab: detailed agent table with status, last action, message count.
fn render_agents_tab(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // tabs
            Constraint::Length(1),  // cycle status
            Constraint::Min(0),    // agents table
        ])
        .split(area);

    render_tab_bar_only(frame, chunks[0], 2);

    // Cycle status bar
    render_cycle_status_bar(frame, chunks[1], app);

    // Agents detail table
    let header = Row::new(vec![
        Cell::from("Agent Name"),
        Cell::from("Status"),
        Cell::from("Last Action"),
        Cell::from("Messages"),
    ])
    .style(
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );

    let rows: Vec<Row> = app
        .agent_details
        .iter()
        .map(|agent| {
            let status_color = agent.status_color();
            let active_marker = if agent.is_active { "*" } else { " " };

            Row::new(vec![
                Cell::from(format!("{active_marker} {}", agent.name))
                    .style(Style::default().fg(Color::White)),
                Cell::from(agent.status.clone()).style(Style::default().fg(status_color)),
                Cell::from(truncate_str(&agent.last_action, 40))
                    .style(Style::default().fg(Color::DarkGray)),
                Cell::from(format!("{}", agent.message_count))
                    .style(Style::default().fg(Color::Cyan)),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(20),
            Constraint::Percentage(15),
            Constraint::Percentage(50),
            Constraint::Percentage(15),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Agent Details "),
    );

    frame.render_widget(table, chunks[2]);
}

/// Logs tab: full-screen log view with color coding.
fn render_logs_tab(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    render_tab_bar_only(frame, chunks[0], 3);

    let items: Vec<ListItem> = app
        .events
        .iter()
        .rev()
        .map(|entry| {
            let line = Line::from(vec![
                Span::styled(
                    format!("{} ", entry.timestamp),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!("[{:<10}] ", entry.topic),
                    Style::default()
                        .fg(entry.topic_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(entry.message.clone(), Style::default().fg(Color::White)),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Full Log View "),
    );

    frame.render_widget(list, chunks[1]);
}

/// Render a standalone tabs bar for non-Market tabs.
fn render_tab_bar_only(frame: &mut Frame, area: Rect, selected: usize) {
    let titles: Vec<Line> = TAB_NAMES
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let style = if i == selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            Line::from(Span::styled(format!(" {t} "), style))
        })
        .collect();

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Bamboo Elf "),
        )
        .select(selected)
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_widget(tabs, area);
}

/// Truncate a string to a maximum display length, adding "..." if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
