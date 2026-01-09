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
/// * `selected` - Whether this card is currently selected
pub fn render_agent_card(f: &mut Frame, area: Rect, agent: &Agent, selected: bool) {
    let border_color = if selected {
        colors::HIGHLIGHT
    } else {
        colors::BORDER
    };

    let border_type = if selected {
        BorderType::Double
    } else {
        BorderType::Rounded
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .border_type(border_type);

    // Card content: project, tool, elapsed
    let content = vec![
        Line::from(truncate(
            &agent.project,
            area.width.saturating_sub(2) as usize,
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
