//! Agent card widget for Kanban column display
//!
//! Renders a single agent as a card showing:
//! - Project name (bold)
//! - Current tool or last latency
//! - Elapsed time (dim)

use crate::config::colors;
use crate::state::Agent;
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

    // Card content: project, tool, elapsed
    let content = vec![
        Line::from(format!(
            "{}{}",
            selection_indicator,
            truncate(&agent.project, area.width.saturating_sub(4) as usize)
        ))
        .style(Style::default().fg(colors::FG).add_modifier(Modifier::BOLD)),
        Line::from(agent.tool_display()).style(Style::default().fg(colors::IDLE)),
        Line::from(agent.elapsed_display()).style(
            Style::default()
                .fg(colors::IDLE)
                .add_modifier(Modifier::DIM),
        ),
    ];

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
