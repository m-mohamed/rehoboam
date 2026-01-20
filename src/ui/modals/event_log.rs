//! Event log modal

use crate::app::App;
use crate::config::colors;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

use super::super::helpers::{centered_rect, format_timestamp, truncate};

pub fn render_event_log(f: &mut Frame, app: &App) {
    let area = centered_rect(60, 50, f.area());

    let items: Vec<ListItem> = app
        .state
        .events
        .iter()
        .take(15)
        .map(|event| {
            let line = format!(
                "{} │ {:12} │ {:15} │ {}",
                format_timestamp(event.timestamp),
                event.event,
                truncate(&event.project, 15),
                event.status
            );
            ListItem::new(line).style(Style::default().fg(colors::FG))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" Event Log ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER))
            .border_type(ratatui::widgets::BorderType::Rounded)
            .style(Style::default().bg(colors::BG)),
    );

    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(list, area);
}
