//! Checkpoint timeline modal

use crate::app::App;
use crate::config::colors;
use ratatui::{
    layout::Alignment,
    prelude::*,
    style::Modifier,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::super::helpers::centered_rect;

pub fn render_checkpoint_timeline(f: &mut Frame, app: &App) {
    let area = centered_rect(70, 60, f.area());

    // Get project name for title
    let title = if let Some(agent) = app.state.selected_agent() {
        format!(" Checkpoint Timeline: {} ", agent.project)
    } else {
        " Checkpoint Timeline ".to_string()
    };

    let content = if app.checkpoint_timeline.is_empty() {
        // Empty state
        let empty_msg = vec![
            Line::from(""),
            Line::styled(
                "No checkpoints yet",
                Style::default().fg(colors::FG).add_modifier(Modifier::DIM),
            ),
            Line::from(""),
            Line::styled(
                "Checkpoints are created automatically during sprite execution",
                Style::default().fg(colors::FG).add_modifier(Modifier::DIM),
            ),
            Line::styled(
                "or manually via the Sprites API.",
                Style::default().fg(colors::FG).add_modifier(Modifier::DIM),
            ),
        ];
        Paragraph::new(empty_msg).alignment(Alignment::Center)
    } else {
        // Build timeline list
        let items: Vec<Line> = app
            .checkpoint_timeline
            .iter()
            .enumerate()
            .map(|(i, cp)| {
                let prefix = if i == app.selected_checkpoint {
                    "▶ "
                } else {
                    "  "
                };
                let style = if i == app.selected_checkpoint {
                    Style::default()
                        .fg(colors::HIGHLIGHT)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(colors::FG)
                };

                // Format elapsed time from Unix timestamp
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let elapsed_secs = (now - cp.created_at).max(0) as u64;
                let elapsed = std::time::Duration::from_secs(elapsed_secs);
                let elapsed_str = format_checkpoint_elapsed(elapsed);

                Line::styled(
                    format!(
                        "{}{} │ {} ago │ {}",
                        prefix,
                        &cp.id[..cp.id.len().min(8)],
                        elapsed_str,
                        cp.comment
                    ),
                    style,
                )
            })
            .collect();

        Paragraph::new(items)
    };

    let timeline_widget = content.block(
        Block::default()
            .title(title)
            .title_bottom(" [↑/k] Up  [↓/j] Down  [Enter] Restore  [t/Esc] Close ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::HIGHLIGHT))
            .border_type(ratatui::widgets::BorderType::Double)
            .style(Style::default().bg(colors::BG)),
    );

    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(timeline_widget, area);
}

/// Format elapsed time for checkpoint display
fn format_checkpoint_elapsed(elapsed: std::time::Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}
