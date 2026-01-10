//! Status column widget for Kanban display
//!
//! Renders a column containing agent cards for a specific status.
//! Columns are: Attention, Working, Compacting, Idle

use crate::config::colors;
use crate::state::Agent;
use crate::ui::card::{render_agent_card, CARD_HEIGHT};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, BorderType, Borders},
    Frame,
};

/// Column definitions with titles and emoji icons
pub const COLUMNS: [(&str, &str); 4] = [
    ("ATTENTION", "🔔"),
    ("WORKING", "🤖"),
    ("COMPACT", "🔄"),
    ("IDLE", "⏸️"),
];

/// Render a status column with agent cards
///
/// # Arguments
/// * `f` - Frame to render into
/// * `area` - Area for the column
/// * `column_index` - Index of this column (0-3)
/// * `agents` - Agents in this column
/// * `selected_card` - Index of selected card if this column is active, None otherwise
/// * `column_active` - Whether this column is the currently selected column
/// * `selected_agents` - Set of pane_ids that are multi-selected for bulk operations
pub fn render_status_column(
    f: &mut Frame,
    area: Rect,
    column_index: usize,
    agents: &[&Agent],
    selected_card: Option<usize>,
    column_active: bool,
    selected_agents: &std::collections::HashSet<String>,
) {
    let (title, icon) = COLUMNS[column_index];

    // Style based on whether column is active
    let (header_style, border_style) = if column_active {
        (
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(colors::HIGHLIGHT),
        )
    } else {
        (
            Style::default().fg(colors::IDLE),
            Style::default().fg(colors::BORDER),
        )
    };

    // Column container
    let block = Block::default()
        .title(format!(" {} {} ({}) ", icon, title, agents.len()))
        .title_style(header_style)
        .borders(Borders::ALL)
        .border_style(border_style)
        .border_type(BorderType::Rounded);

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Render cards vertically within column
    for (i, agent) in agents.iter().enumerate() {
        let y = inner.y + (i as u16 * CARD_HEIGHT);

        // Stop if card won't fit
        if y + CARD_HEIGHT > inner.y + inner.height {
            break;
        }

        let card_area = Rect::new(inner.x, y, inner.width, CARD_HEIGHT);
        let is_selected = column_active && selected_card == Some(i);
        let is_multi_selected = selected_agents.contains(&agent.pane_id);
        render_agent_card(f, card_area, agent, is_selected, is_multi_selected);
    }

    // Show overflow indicator if there are more cards
    let visible_cards = (inner.height / CARD_HEIGHT) as usize;
    if agents.len() > visible_cards {
        let overflow_count = agents.len() - visible_cards;
        let overflow_text = format!("... +{} more", overflow_count);
        let overflow_y = inner.y + inner.height.saturating_sub(1);

        // Render overflow indicator at bottom
        if overflow_y >= inner.y {
            let overflow_area = Rect::new(inner.x, overflow_y, inner.width, 1);
            let overflow = ratatui::widgets::Paragraph::new(overflow_text).style(
                Style::default()
                    .fg(colors::IDLE)
                    .add_modifier(Modifier::DIM),
            );
            f.render_widget(overflow, overflow_area);
        }
    }
}
