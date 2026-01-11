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
    style::{Modifier, Style},
    text::Line,
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

    // Build card content
    let mut content = vec![
        // Line 1: Project name with sprite indicator
        Line::from(format!(
            "{}{}{}",
            selection_indicator,
            sprite_indicator,
            truncate(&agent.project, area.width.saturating_sub(6) as usize)
        ))
        .style(Style::default().fg(colors::FG).add_modifier(Modifier::BOLD)),
    ];

    // Line 2: Loop mode indicator OR subagent count OR tool display
    match &agent.loop_mode {
        LoopMode::Active => {
            content.push(
                Line::from(format!(
                    "Loop {}/{}",
                    agent.loop_iteration, agent.loop_max
                ))
                .style(Style::default().fg(colors::WORKING)),
            );
        }
        LoopMode::Stalled => {
            content.push(
                Line::from("STALLED (X/R)")
                    .style(Style::default().fg(colors::ATTENTION).add_modifier(Modifier::BOLD)),
            );
        }
        LoopMode::Complete => {
            content.push(
                Line::from(format!("DONE at {}", agent.loop_iteration))
                    .style(Style::default().fg(colors::IDLE)),
            );
        }
        LoopMode::None => {
            // Show subagent info if any, otherwise tool display
            if !agent.subagents.is_empty() {
                let running = agent.subagents.iter().filter(|s| s.status == "running").count();
                let total = agent.subagents.len();
                let display = if running > 0 {
                    format!("{} subagent{}", running, if running == 1 { "" } else { "s" })
                } else {
                    format!("{} done", total)
                };
                content.push(
                    Line::from(display).style(Style::default().fg(colors::WORKING)),
                );
            } else {
                content.push(
                    Line::from(agent.tool_display()).style(Style::default().fg(colors::IDLE)),
                );
            }
        }
    }

    // Line 3: Elapsed time (or most recent subagent if any running)
    if !agent.subagents.is_empty() {
        // Show most recent subagent description
        if let Some(subagent) = agent.subagents.iter().rev().find(|s| s.status == "running") {
            content.push(
                Line::from(truncate(&subagent.description, area.width.saturating_sub(4) as usize))
                    .style(Style::default().fg(colors::IDLE).add_modifier(Modifier::DIM)),
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
