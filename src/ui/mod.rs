//! UI rendering module for Rehoboam TUI
//!
//! Provides a Kanban-style column layout where agents are grouped by status:
//! - Attention (needs user input)
//! - Working (actively processing)
//! - Compacting (context compaction)
//! - Idle (waiting)

mod card;
mod column;

use crate::app::App;
use crate::config::colors;
use crate::state::{Status, NUM_COLUMNS};
use column::render_status_column;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    prelude::*,
    style::{Modifier, Style},
    symbols,
    widgets::{
        Block, Borders, List, ListItem, Paragraph, RenderDirection, Sparkline, SparklineBar,
    },
    Frame,
};

/// Main render function
pub fn render(f: &mut Frame, app: &App) {
    // Create layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(12),   // Agent columns (Kanban)
            Constraint::Length(8), // Activity sparklines
            Constraint::Length(1), // Footer
        ])
        .split(f.area());

    render_header(f, chunks[0], app);
    render_agent_columns(f, chunks[1], app);
    render_activity(f, chunks[2], app);
    render_footer(f, chunks[3], app);

    // Render event log if in debug mode
    if app.debug_mode && !app.state.events.is_empty() {
        render_event_log(f, app);
    }

    // Render help popup if active
    if app.show_help {
        render_help(f);
    }
}

fn render_header(f: &mut Frame, area: Rect, app: &App) {
    // Use cached status counts (O(1) instead of O(4n))
    let [attention, working, compacting, idle] = app.state.status_counts;
    let total = app.state.agents.len();

    // Build status summary
    let status_parts: Vec<String> = [
        (attention, "attention"),
        (working, "working"),
        (compacting, "compacting"),
        (idle, "idle"),
    ]
    .iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, label)| format!("{} {}", count, label))
    .collect();

    let frozen_indicator = if app.frozen { " [FROZEN]" } else { "" };
    let title = if total == 0 {
        format!("Rehoboam{}", frozen_indicator)
    } else {
        format!(
            "Rehoboam ({} agents: {}){}",
            total,
            status_parts.join(", "),
            frozen_indicator
        )
    };

    let header = Paragraph::new(title)
        .style(Style::default().fg(colors::FG).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER))
                .border_type(ratatui::widgets::BorderType::Rounded),
        );
    f.render_widget(header, area);
}

/// Render agents in Kanban-style columns by status
fn render_agent_columns(f: &mut Frame, area: Rect, app: &App) {
    let columns = app.state.agents_by_column();

    // 4 equal-width columns
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, NUM_COLUMNS as u32),
            Constraint::Ratio(1, NUM_COLUMNS as u32),
            Constraint::Ratio(1, NUM_COLUMNS as u32),
            Constraint::Ratio(1, NUM_COLUMNS as u32),
        ])
        .split(area);

    for (i, agents) in columns.iter().enumerate() {
        let selected_card = if app.state.selected_column == i {
            Some(app.state.selected_card)
        } else {
            None
        };
        let column_active = app.state.selected_column == i;
        render_status_column(f, chunks[i], i, agents, selected_card, column_active);
    }
}

fn render_activity(f: &mut Frame, area: Rect, app: &App) {
    // Flatten already-sorted columns instead of O(n log n) sorted_agents()
    let columns = app.state.agents_by_column();
    let agents: Vec<&_> = columns.iter().flatten().copied().collect();

    // Create horizontal layout for sparklines
    let num_agents = agents.len().max(1);
    let constraints: Vec<Constraint> = (0..num_agents)
        .map(|_| Constraint::Ratio(1, num_agents as u32))
        .collect();

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    // Get current time for pulse animation
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    for (i, agent) in agents.iter().enumerate() {
        if i >= chunks.len() {
            break;
        }

        let base_color = status_base_color(&agent.status);
        let is_working = matches!(agent.status, Status::Working);

        // Create SparklineBars with pulse effect for working status
        let data: Vec<SparklineBar> = agent
            .activity
            .iter()
            .enumerate()
            .map(|(idx, v)| {
                let value = (v * 8.0) as u64;
                let bar = SparklineBar::from(value);

                // Pulse effect: recent bars glow brighter when working
                if is_working && idx >= agent.activity.len().saturating_sub(5) {
                    // Pulse based on time - creates a breathing effect
                    let pulse = ((now / 100) % 10) as usize;
                    let intensity = if (idx + pulse).is_multiple_of(3) {
                        colors::WORKING_BRIGHT // Brighter pulse
                    } else {
                        base_color
                    };
                    bar.style(Style::default().fg(intensity))
                } else {
                    bar.style(Style::default().fg(base_color))
                }
            })
            .collect();

        let sparkline = Sparkline::default()
            .data(data)
            .direction(RenderDirection::LeftToRight)
            .bar_set(symbols::bar::NINE_LEVELS)
            .absent_value_style(Style::default().fg(colors::BG_LIGHT))
            .absent_value_symbol(symbols::bar::NINE_LEVELS.empty)
            .block(
                Block::default()
                    .title(format!(" {} ", truncate(&agent.project, 12)))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(colors::BORDER))
                    .border_type(ratatui::widgets::BorderType::Rounded),
            );

        f.render_widget(sparkline, chunks[i]);
    }

    // Show placeholder if no agents
    if agents.is_empty() {
        let placeholder = Block::default()
            .title(" Activity ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER))
            .border_type(ratatui::widgets::BorderType::Rounded);
        f.render_widget(placeholder, area);
    }
}

fn render_footer(f: &mut Frame, area: Rect, app: &App) {
    let mode_indicators: Vec<&str> = [
        app.debug_mode.then_some("[debug]"),
        app.frozen.then_some("[frozen]"),
    ]
    .into_iter()
    .flatten()
    .collect();

    let mode = if mode_indicators.is_empty() {
        String::new()
    } else {
        format!("{} ", mode_indicators.join(" "))
    };

    let help = format!(
        "{}q:quit  h/l:column  j/k:card  Enter:jump  f:freeze  d:debug  ?:help",
        mode
    );

    let footer = Paragraph::new(help)
        .style(Style::default().fg(colors::IDLE))
        .alignment(Alignment::Center);

    f.render_widget(footer, area);
}

fn render_event_log(f: &mut Frame, app: &App) {
    let area = centered_rect(60, 50, f.area());

    let items: Vec<ListItem> = app
        .state
        .events
        .iter()
        .take(15)
        .map(|event| {
            let line = format!(
                "{} │ {:12} │ {:15} │ {}",
                format_timestamp(event.timestamp),
                event.event,
                truncate(&event.project, 15),
                event.status
            );
            ListItem::new(line).style(Style::default().fg(colors::FG))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" Event Log ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER))
            .border_type(ratatui::widgets::BorderType::Rounded)
            .style(Style::default().bg(colors::BG)),
    );

    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(list, area);
}

fn render_help(f: &mut Frame) {
    let area = centered_rect(45, 50, f.area());

    let help_text = r#"
  Keybindings

  q, Esc      Quit
  h, Left     Previous column
  l, Right    Next column
  j, Down     Next card in column
  k, Up       Previous card in column
  Enter       Jump to agent pane
  f           Freeze display
  d           Toggle debug mode
  ?, H        Toggle this help
"#;

    let help = Paragraph::new(help_text)
        .style(Style::default().fg(colors::FG))
        .block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::HIGHLIGHT))
                .border_type(ratatui::widgets::BorderType::Rounded)
                .style(Style::default().bg(colors::BG)),
        );

    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(help, area);
}

// Helper functions

fn status_base_color(status: &Status) -> Color {
    match status {
        Status::Working => colors::WORKING,
        Status::Attention(_) => colors::ATTENTION,
        Status::Compacting => colors::COMPACTING,
        Status::Idle => colors::IDLE,
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len - 1])
    }
}

fn format_timestamp(ts: i64) -> String {
    use std::time::{Duration, UNIX_EPOCH};
    let datetime = UNIX_EPOCH + Duration::from_secs(ts as u64);
    let now = std::time::SystemTime::now();

    // Simple HH:MM:SS format
    if let Ok(duration) = now.duration_since(datetime) {
        let secs = duration.as_secs();
        let hours = (secs / 3600) % 24;
        let mins = (secs / 60) % 60;
        let secs = secs % 60;
        format!("{:02}:{:02}:{:02}", hours, mins, secs)
    } else {
        "??:??:??".to_string()
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
