//! Agent card widget for Kanban column display
//!
//! Renders a single agent as a card showing:
//! - Project name (bold)
//! - Loop mode indicator (if in loop mode)
//! - Current tool or last latency
//! - Elapsed time (dim)

use crate::config::colors;
use crate::state::{Agent, LoopMode};
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

    // v1.2: Role badge (Cursor-inspired Planner/Worker/Reviewer)
    // v2.1.x: Use explicit agent type badge if available
    let role_badge = agent.agent_type_badge();

    // v2.1.x: Plan mode indicator
    let mode_indicator = if agent.permission_mode.as_deref() == Some("plan") {
        " [PLAN]"
    } else {
        ""
    };

    // Build card content
    let mut content = vec![
        // Line 1: Project name with sprite indicator and role badge
        Line::from(format!(
            "{}{}{} {}{}",
            selection_indicator,
            sprite_indicator,
            truncate(&agent.project, area.width.saturating_sub(14) as usize),
            role_badge,
            mode_indicator
        ))
        .style(Style::default().fg(colors::FG).add_modifier(Modifier::BOLD)),
    ];

    // Line 2: Loop mode indicator OR task info OR subagent count OR tool display
    match &agent.loop_mode {
        LoopMode::Active => {
            content.push(
                Line::from(format!("Loop {}/{}", agent.loop_iteration, agent.loop_max))
                    .style(Style::default().fg(colors::WORKING)),
            );
        }
        LoopMode::Stalled => {
            content.push(
                Line::from("STALLED (X/R)").style(
                    Style::default()
                        .fg(colors::ATTENTION)
                        .add_modifier(Modifier::BOLD),
                ),
            );
        }
        LoopMode::Complete => {
            content.push(
                Line::from(format!("DONE at {}", agent.loop_iteration))
                    .style(Style::default().fg(colors::IDLE)),
            );
        }
        LoopMode::None => {
            // v2.2: Show current task info if working on a Claude Code Task
            if let Some(ref task_subject) = agent.current_task_subject {
                // Show task subject (truncated)
                let task_display = format!(
                    "📋 {}",
                    truncate(task_subject, area.width.saturating_sub(6) as usize)
                );
                content.push(Line::from(task_display).style(Style::default().fg(colors::WORKING)));
            } else if let Some(ref task_id) = agent.current_task_id {
                // Show task ID if no subject available
                let task_display = format!("📋 Task #{}", task_id);
                content.push(Line::from(task_display).style(Style::default().fg(colors::WORKING)));
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
                content.push(Line::from(display).style(Style::default().fg(colors::WORKING)));
            } else {
                content.push(
                    Line::from(agent.tool_display()).style(Style::default().fg(colors::IDLE)),
                );
            }
        }
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
            Span::styled("[", Style::default().fg(colors::IDLE)),
            Span::styled(bar, Style::default().fg(bar_color)),
            Span::styled("]", Style::default().fg(colors::IDLE)),
            Span::raw(format!(" {:.0}%", pct)),
        ];
        if !label.is_empty() {
            spans.push(Span::styled(
                format!(" {}", label),
                Style::default().fg(bar_color).add_modifier(Modifier::BOLD),
            ));
        }

        content.push(Line::from(spans));
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
                .style(
                    Style::default()
                        .fg(colors::IDLE)
                        .add_modifier(Modifier::DIM),
                ),
            );
        } else {
            content.push(
                Line::from(agent.elapsed_display()).style(
                    Style::default()
                        .fg(colors::IDLE)
                        .add_modifier(Modifier::DIM),
                ),
            );
        }
    } else {
        content.push(
            Line::from(agent.elapsed_display()).style(
                Style::default()
                    .fg(colors::IDLE)
                    .add_modifier(Modifier::DIM),
            ),
        );
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
