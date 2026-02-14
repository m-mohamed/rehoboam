//! UI rendering module for Rehoboam TUI
//!
//! Provides a team-grouped view where agents are organized by team with hierarchy.
//! When no teams exist, agents appear under "Independent" as a flat list.

pub mod helpers;
mod modals;
mod views;

use crate::app::{App, InputMode};
use crate::config::colors;
use modals::{
    render_debug_viewer, render_event_log, render_help, render_history_viewer,
    render_insights_viewer, render_plan_viewer, render_spawn_dialog, render_stats_viewer,
};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    prelude::*,
    style::Modifier,
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use views::render_team_view;

/// Main render function
pub fn render(f: &mut Frame, app: &mut App) {
    // Create layout: 3 zones (header, team view, footer)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(12),   // Team view (always rendered)
            Constraint::Length(1), // Footer
        ])
        .split(f.area());

    render_header(f, chunks[0], app);
    render_team_view(f, chunks[1], app);
    render_footer(f, chunks[2], app);

    // Render event log if in debug mode
    if app.debug_mode && !app.state.events.is_empty() {
        render_event_log(f, app);
    }

    // Render task board overlay if active
    if app.show_task_board {
        let area = helpers::centered_rect(80, 80, f.area());
        f.render_widget(ratatui::widgets::Clear, area);
        views::render_task_board(f, area, app);
    }

    // Render plan viewer overlay if active
    if app.show_plan_viewer {
        let area = helpers::centered_rect(85, 85, f.area());
        render_plan_viewer(f, area, app);
    }

    // Render stats dashboard overlay if active
    if app.show_stats_viewer {
        let area = helpers::centered_rect(90, 90, f.area());
        render_stats_viewer(f, area, app);
    }

    // Render history timeline overlay if active
    if app.show_history_viewer {
        let area = helpers::centered_rect(85, 85, f.area());
        render_history_viewer(f, area, app);
    }

    // Render debug log viewer overlay if active
    if app.show_debug_viewer {
        let area = helpers::centered_rect(85, 85, f.area());
        render_debug_viewer(f, area, app);
    }

    // Render insights report overlay if active
    if app.show_insights_viewer {
        let area = helpers::centered_rect(90, 90, f.area());
        render_insights_viewer(f, area, app);
    }

    // Render help popup if active (always on top)
    if app.show_help {
        render_help(f);
    }

    // Render spawn dialog if in spawn mode (always on top)
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
        "Rehoboam".to_string()
    } else {
        format!(
            "Rehoboam ({} agents: {}){}{}",
            total,
            status_parts.join(", "),
            cc_version,
            sprite_indicator,
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

fn render_footer(f: &mut Frame, area: Rect, app: &App) {
    // Health warning takes highest priority (persistent red text)
    if let Some(ref warning) = app.state.health_warning {
        let style = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
        let msg = Paragraph::new(warning.as_str())
            .style(style)
            .alignment(Alignment::Center);
        f.render_widget(msg, area);
        return;
    }

    // Search mode: show search input
    if app.input_mode == InputMode::Search {
        let search_text = format!("Search: {}|", app.search_query);
        let footer = Paragraph::new(search_text)
            .style(Style::default().fg(Color::Yellow))
            .alignment(Alignment::Center);
        f.render_widget(footer, area);
        return;
    }

    // Context-aware help based on selection state
    let help = if let Some(_agent) = app.state.selected_agent() {
        // Single agent selected - show relevant commands
        let debug = if app.debug_mode { "[debug] " } else { "" };
        format!("{debug}Enter:jump  T:tasks  P:plans  S:stats  L:log  D:debug  I:insights  ?:help")
    } else {
        // No selection - show general commands
        let debug = if app.debug_mode { "[debug] " } else { "" };
        format!("{debug}j/k:nav  s:spawn  T:tasks  P:plans  S:stats  L:log  D:debug  I:insights  ?:help  q:quit")
    };

    let footer = Paragraph::new(help)
        .style(Style::default().fg(colors::IDLE))
        .alignment(Alignment::Center);

    f.render_widget(footer, area);
}
