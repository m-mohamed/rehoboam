//! Agent card widget for Kanban column display
//!
//! Renders a single agent as a card showing:
//! - Project name (bold)
//! - Current tool or last latency
//! - Elapsed time (dim)

use crate::config::{colors, styles};
use crate::state::Agent;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

/// Height of each agent card in rows
pub const CARD_HEIGHT: u16 = 5;

/// Render an agent card
///
/// # Arguments
/// * `f` - Frame to render into
/// * `area` - Area for the card
/// * `agent` - Agent data to display
/// * `selected` - Whether this card is currently focused (cursor)
/// * `multi_selected` - Whether this card is part of bulk selection
pub fn render_agent_card(
    f: &mut Frame,
    area: Rect,
    agent: &Agent,
    selected: bool,
    multi_selected: bool,
) {
    let border_color = if selected {
        colors::HIGHLIGHT
    } else if multi_selected {
        colors::WORKING // Use working color for multi-selected
    } else {
        colors::BORDER
    };

    let border_type = if selected {
        BorderType::Double
    } else if multi_selected {
        BorderType::Thick
    } else {
        BorderType::Rounded
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .border_type(border_type);

    // Selection indicator prefix
    let selection_indicator = if multi_selected { "● " } else { "" };

    // Sprite indicator (cloud icon for remote agents)
    let sprite_indicator = if agent.is_sprite { "☁ " } else { "" };

    // v2.1.x: Background task indicator (hourglass for agents with background tasks)
    let bg_task_indicator = if agent.has_background_tasks { "⏳" } else { "" };

    // v2.1.x: Failed tool indicator (shows when last tool call failed)
    let failed_tool_indicator = if agent.last_tool_failed { "❌" } else { "" };

    // v1.2: Role badge (Cursor-inspired Planner/Worker/Reviewer)
    // v2.1.x: Use explicit agent type badge if available
    let role_badge = agent.agent_type_badge();

    // v2.1.x: Plan mode indicator
    let mode_indicator = if agent.permission_mode.as_deref() == Some("plan") {
        " [PLAN]"
    } else {
        ""
    };

    // v2.1.x: Model name indicator
    let model_indicator = agent.model_display().map_or(String::new(), |m| format!(" ({})", m));

    // Build card content
    let mut content = vec![
        // Line 1: Project name with sprite indicator, background task indicator, failed tool indicator, role badge, and model
        Line::from(format!(
            "{}{}{}{}{} {}{}{}",
            selection_indicator,
            sprite_indicator,
            bg_task_indicator,
            failed_tool_indicator,
            truncate(&agent.project, area.width.saturating_sub(24) as usize),
            role_badge,
            mode_indicator,
            model_indicator
        ))
        .style(styles::HEADER),
    ];

    // Line 2: Task info OR subagent count OR tool display
    // v2.2: Show current task info if working on a Claude Code Task
    if let Some(ref task_subject) = agent.current_task_subject {
        // Show task subject (truncated)
        let task_display = format!(
            "📋 {}",
            truncate(task_subject, area.width.saturating_sub(6) as usize)
        );
        content.push(Line::from(task_display).style(styles::WORKING));
    } else if let Some(ref task_id) = agent.current_task_id {
        // Show task ID if no subject available
        let task_display = format!("📋 Task #{}", task_id);
        content.push(Line::from(task_display).style(styles::WORKING));
    } else if !agent.subagents.is_empty() {
        // Show subagent info if any
        let running: Vec<_> = agent
            .subagents
            .iter()
            .filter(|s| s.status == "running")
            .collect();

        let display = if !running.is_empty() {
            // v1.3: Show role breakdown for running subagents
            use crate::state::AgentRole;
            let planners = running
                .iter()
                .filter(|s| s.role == AgentRole::Planner)
                .count();
            let workers = running
                .iter()
                .filter(|s| s.role == AgentRole::Worker)
                .count();
            let reviewers = running
                .iter()
                .filter(|s| s.role == AgentRole::Reviewer)
                .count();

            let mut parts = Vec::new();
            if planners > 0 {
                parts.push(format!("{planners}P"));
            }
            if workers > 0 {
                parts.push(format!("{workers}W"));
            }
            if reviewers > 0 {
                parts.push(format!("{reviewers}R"));
            }
            let others = running.len() - planners - workers - reviewers;
            if others > 0 {
                parts.push(format!("{others}"));
            }

            if parts.is_empty() {
                format!(
                    "{} subagent{}",
                    running.len(),
                    if running.len() == 1 { "" } else { "s" }
                )
            } else {
                format!("⤵ {}", parts.join("/"))
            }
        } else {
            format!("{} done", agent.subagents.len())
        };
        content.push(Line::from(display).style(styles::WORKING));
    } else {
        content.push(Line::from(agent.tool_display()).style(styles::IDLE));
    }

    // Line 3: Elapsed time OR context usage warning (when context is high)
    // v2.1.x: Show context usage bar when context is getting full (>= 80%)
    if let Some(pct) = agent.context_usage_percent.filter(|&p| p >= 80.0) {
        // Show context usage bar instead of elapsed time when context is high
        let bar_width = 10_usize;
        let filled = ((pct / 100.0) * bar_width as f64) as usize;
        let empty = bar_width.saturating_sub(filled);

        let (bar_color, label) = match agent.context_level() {
            Some("critical") => (Color::Red, "FULL"),
            Some("high") => (Color::Yellow, "HIGH"),
            _ => (Color::Blue, ""),
        };

        let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));

        // Build context line with spans
        let mut spans = vec![
            Span::styled("[", styles::IDLE),
            Span::styled(bar, Style::default().fg(bar_color)),
            Span::styled("]", styles::IDLE),
            Span::raw(format!(" {:.0}%", pct)),
        ];
        if !label.is_empty() {
            spans.push(Span::styled(
                format!(" {}", label),
                Style::default().fg(bar_color).add_modifier(Modifier::BOLD),
            ));
        }

        content.push(Line::from(spans));
    } else if let Some(ref cwd) = agent.cwd {
        // v2.1.x: Show working directory with elapsed time
        let cwd_display = truncate_path(cwd, area.width.saturating_sub(12) as usize);
        let elapsed = agent.elapsed_display();
        content.push(
            Line::from(vec![
                Span::styled(format!("📁 {}", cwd_display), styles::DIM),
                Span::raw(" "),
                Span::styled(elapsed, styles::DIM),
            ])
        );
    } else if !agent.subagents.is_empty() {
        // Show most recent subagent description with role badge
        if let Some(subagent) = agent.subagents.iter().rev().find(|s| s.status == "running") {
            // v1.3: Include role badge in subagent description
            use crate::state::AgentRole;
            let role_badge = match subagent.role {
                AgentRole::Planner => "[P] ",
                AgentRole::Worker => "[W] ",
                AgentRole::Reviewer => "[R] ",
                AgentRole::General => "",
            };
            content.push(
                Line::from(format!(
                    "{}{}",
                    role_badge,
                    truncate(&subagent.description, area.width.saturating_sub(8) as usize)
                ))
                .style(styles::DIM),
            );
        } else {
            content.push(Line::from(agent.elapsed_display()).style(styles::DIM));
        }
    } else {
        content.push(Line::from(agent.elapsed_display()).style(styles::DIM));
    }

    let paragraph = Paragraph::new(content).block(block);
    f.render_widget(paragraph, area);
}

/// Truncate string to max length with ellipsis
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len > 1 {
        format!("{}…", &s[..max_len - 1])
    } else {
        "…".to_string()
    }
}

/// Truncate a file path intelligently for display
///
/// Replaces home directory with ~ and shows only the last components if too long.
fn truncate_path(path: &str, max_len: usize) -> String {
    // Replace home directory with ~
    let home = std::env::var("HOME").unwrap_or_default();
    let display_path = if !home.is_empty() && path.starts_with(&home) {
        format!("~{}", &path[home.len()..])
    } else {
        path.to_string()
    };

    if display_path.len() <= max_len {
        display_path
    } else if max_len > 4 {
        // Show "…/last/components" style
        let parts: Vec<&str> = display_path.split('/').collect();
        let mut result = String::new();
        for part in parts.iter().rev() {
            if result.is_empty() {
                result = part.to_string();
            } else {
                let candidate = format!("{}/{}", part, result);
                if candidate.len() < max_len {
                    result = candidate;
                } else {
                    break;
                }
            }
        }
        format!("…/{}", result)
    } else {
        "…".to_string()
    }
}
