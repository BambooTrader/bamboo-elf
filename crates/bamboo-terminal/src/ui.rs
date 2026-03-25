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
        1 => render_portfolio_tab(frame, size),
        2 => render_agents_tab(frame, size),
        3 => render_logs_tab(frame, size, app),
        _ => render_market_tab(frame, size, app),
    }
}

/// Market tab: 5-panel layout with tabs bar.
fn render_market_tab(frame: &mut Frame, area: Rect, app: &mut App) {
    // Vertical split: tabs | middle | lower | event log
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // tabs
            Constraint::Percentage(35), // middle (watchlist + news)
            Constraint::Percentage(30), // lower (positions + agents)
            Constraint::Min(6),         // event log
        ])
        .split(area);

    // Render tabs bar
    render_tabs(frame, main_chunks[0], app);

    // Middle row: watchlist (left) | news (right)
    let middle_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_chunks[1]);

    render_watchlist(frame, middle_chunks[0], app);
    render_news(frame, middle_chunks[1], app);

    // Lower row: positions (left) | agent status (right)
    let lower_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(main_chunks[2]);

    render_positions(frame, lower_chunks[0], app);
    render_agent_status(frame, lower_chunks[1], app);

    // Bottom: event log
    render_event_log(frame, main_chunks[3], app);
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
fn render_watchlist(frame: &mut Frame, area: Rect, app: &App) {
    let is_focused = app.focused_panel == PanelId::Watchlist;
    let border_style = if is_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    // Split the area: table on left, leave room for sparkline rendering
    // We'll render sparklines manually after the table
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
            let symbol = entry.instrument_id.split('.').next().unwrap_or(&entry.instrument_id);
            let price_str = format!("{:.2}", entry.last_price);
            let change_str = format!("{:+.1}%", entry.change_pct);
            let change_color = if entry.change_pct >= 0.0 {
                Color::Green
            } else {
                Color::Red
            };

            let row_style = if i == app.watchlist.selected {
                Style::default().bg(Color::DarkGray)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(symbol.to_string()).style(Style::default().fg(Color::White)),
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

/// Positions panel.
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
            let symbol = pos.instrument_id.split('.').next().unwrap_or(&pos.instrument_id);
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

/// Agent status panel.
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

/// Portfolio tab stub.
fn render_portfolio_tab(frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    render_tab_bar_only(frame, chunks[0], 1);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Portfolio ");

    let text = Paragraph::new("Portfolio details coming in Spec 2")
        .block(block)
        .alignment(ratatui::layout::Alignment::Center)
        .style(Style::default().fg(Color::DarkGray));

    frame.render_widget(text, chunks[1]);
}

/// Agents tab stub.
fn render_agents_tab(frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    render_tab_bar_only(frame, chunks[0], 2);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Agents ");

    let text = Paragraph::new("Agent details coming in Spec 2")
        .block(block)
        .alignment(ratatui::layout::Alignment::Center)
        .style(Style::default().fg(Color::DarkGray));

    frame.render_widget(text, chunks[1]);
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
