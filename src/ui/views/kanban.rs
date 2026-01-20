//! Kanban view - agents in columns by status

use crate::app::App;
use crate::state::NUM_COLUMNS;
use crate::ui::column::render_status_column;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    Frame,
};

/// Render agents in Kanban-style columns by status
pub fn render_agent_columns(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let columns = app.state.agents_by_column();

    // 3 equal-width columns (Attention, Working, Compacting)
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
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
        render_status_column(
            f,
            chunks[i],
            i,
            agents,
            selected_card,
            column_active,
            &app.state.selected_agents,
        );
    }
}
