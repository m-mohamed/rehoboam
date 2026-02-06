//! UI rendering module for Rehoboam TUI
//!
//! Provides a team-grouped view where agents are organized by team with hierarchy.
//! When no teams exist, agents appear under "Independent" as a flat list.

pub mod helpers;
mod modals;
mod views;

use crate::app::{App, InputMode};
use crate::config::colors;
use crate::state::Status;
use helpers::{status_base_color, truncate};
use modals::{
    render_checkpoint_timeline, render_dashboard, render_diff_modal, render_event_log, render_help,
    render_input_dialog, render_spawn_dialog,
};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    prelude::*,
    style::Modifier,
    symbols,
    widgets::{Block, Borders, Paragraph, RenderDirection, Sparkline, SparklineBar},
    Frame,
};
use views::render_team_view;

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

    render_team_view(f, chunks[1], app);

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

    // Render dashboard overlay if active
    if app.show_dashboard {
        render_dashboard(f, app);
    }

    // Render diff modal if active
    if app.show_diff {
        render_diff_modal(f, app);
    }

    // Render checkpoint timeline if active
    if app.show_checkpoint_timeline {
        render_checkpoint_timeline(f, app);
    }

    // Render input dialog if in input mode
    if app.input_mode == InputMode::Input {
        render_input_dialog(f, app);
    }

    // Render spawn dialog if in spawn mode
    if app.input_mode == InputMode::Spawn {
        render_spawn_dialog(f, &app.spawn_state);
    }
}

fn render_header(f: &mut Frame, area: Rect, app: &App) {
    // Use cached status counts (O(1) instead of O(3n))
    let [attention, working, compacting] = app.state.status_counts;
    let total = app.state.agents.len();
    let sprite_count = app.state.sprite_agent_count();

    // Build status summary
    let status_parts: Vec<String> = [
        (attention, "attention"),
        (working, "working"),
        (compacting, "compacting"),
    ]
    .iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, label)| format!("{count} {label}"))
    .collect();

    // Show Claude Code version from any active agent (they should all be the same)
    let cc_version = app
        .state
        .agents
        .values()
        .find_map(|a| a.claude_code_version.as_deref())
        .map(|v| format!(" [CC {v}]"))
        .unwrap_or_default();

    let frozen_indicator = if app.frozen { " [FROZEN]" } else { "" };
    // Show sprite count with connection status if any remote agents
    let connected_count = app.state.connected_sprite_count();
    let sprite_indicator = if sprite_count > 0 {
        if connected_count > 0 {
            // Show connected/total (e.g., "☁ 2/3 sprites")
            format!(
                " [☁ {}/{} sprite{}]",
                connected_count,
                sprite_count,
                if sprite_count == 1 { "" } else { "s" }
            )
        } else {
            // No sprites connected yet
            format!(
                " [☁ {} sprite{} (offline)]",
                sprite_count,
                if sprite_count == 1 { "" } else { "s" }
            )
        }
    } else {
        String::new()
    };
    let title = if total == 0 {
        format!("Rehoboam{frozen_indicator}")
    } else {
        format!(
            "Rehoboam ({} agents: {}){}{}{}",
            total,
            status_parts.join(", "),
            cc_version,
            sprite_indicator,
            frozen_indicator,
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
    // Check if we have a status message to display
    if let Some((ref msg, timestamp)) = app.status_message {
        // Only show if less than 5 seconds old
        if timestamp.elapsed() < std::time::Duration::from_secs(5) {
            let style = if msg.starts_with("Error") || msg.starts_with("⚠") {
                Style::default().fg(Color::Red)
            } else if msg.starts_with("✓") {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Yellow)
            };
            let status = Paragraph::new(msg.as_str())
                .style(style)
                .alignment(Alignment::Center);
            f.render_widget(status, area);
            return;
        }
    }

    // Search mode: show search input
    if app.input_mode == InputMode::Search {
        let search_text = format!("🔍 Search: {}█", app.search_query);
        let footer = Paragraph::new(search_text)
            .style(Style::default().fg(Color::Yellow))
            .alignment(Alignment::Center);
        f.render_widget(footer, area);
        return;
    }

    let selection_count = app.state.selected_agents.len();

    // Context-aware help based on selection state
    let help = if selection_count > 0 {
        // Multi-select mode
        format!("[{selection_count} selected]  Y/N:bulk approve  K:kill all  x:clear  Space:toggle")
    } else if let Some(_agent) = app.state.selected_agent() {
        // Single agent selected - show relevant commands
        let mode_indicators: Vec<&str> = [
            app.debug_mode.then_some("[debug]"),
            app.frozen.then_some("[frozen]"),
        ]
        .into_iter()
        .flatten()
        .collect();

        let prefix = if mode_indicators.is_empty() {
            String::new()
        } else {
            format!("{} ", mode_indicators.join(" "))
        };

        format!("{prefix}Enter:jump  y/n:approve  c:input  d:dash  ?:help")
    } else {
        // No selection - show general commands
        let mode_indicators: Vec<&str> = [
            app.debug_mode.then_some("[debug]"),
            app.frozen.then_some("[frozen]"),
        ]
        .into_iter()
        .flatten()
        .collect();

        let prefix = if mode_indicators.is_empty() {
            String::new()
        } else {
            format!("{} ", mode_indicators.join(" "))
        };

        format!("{prefix}j/k:nav  s:spawn  d:dashboard  /:search  ?:help  q:quit")
    };

    let footer = Paragraph::new(help)
        .style(Style::default().fg(colors::IDLE))
        .alignment(Alignment::Center);

    f.render_widget(footer, area);
}
