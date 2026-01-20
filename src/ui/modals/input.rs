//! Input dialog modal

use crate::app::App;
use crate::config::colors;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::super::helpers::centered_rect;

pub fn render_input_dialog(f: &mut Frame, app: &App) {
    let area = centered_rect(60, 20, f.area());

    // Get selected agent info for the title
    let title = if let Some(agent) = app.state.selected_agent() {
        format!(" Send to: {} ({}) ", agent.project, agent.pane_id)
    } else {
        " Send Input ".to_string()
    };

    // Build input display with cursor
    let input_display = format!("{}▏", app.input_buffer);

    let input_widget = Paragraph::new(input_display)
        .style(Style::default().fg(colors::FG))
        .block(
            Block::default()
                .title(title)
                .title_bottom(" [Enter] Send  [Esc] Cancel ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::HIGHLIGHT))
                .border_type(ratatui::widgets::BorderType::Double)
                .style(Style::default().bg(colors::BG)),
        );

    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(input_widget, area);
}
