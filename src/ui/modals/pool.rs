//! Sprite pool management modal
//!
//! Displays and manages the distributed sprite worker pool.

use crate::app::App;
use crate::config::colors;
use crate::sprite::SpriteWorkerStatus;
use ratatui::{
    layout::Alignment,
    prelude::*,
    style::Modifier,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::super::helpers::centered_rect;

pub fn render_pool_management(f: &mut Frame, app: &App) {
    let area = centered_rect(75, 70, f.area());

    let content = if let Some(ref pool) = app.sprite_pool {
        // Build worker table
        let mut lines: Vec<Line> = Vec::new();

        // Header
        lines.push(Line::styled(
            format!(
                "{:^12} │ {:^16} │ {:^12} │ {:^30}",
                "Worker", "Sprite", "Status", "Task"
            ),
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::styled(
            "─".repeat(75),
            Style::default().fg(colors::BORDER),
        ));

        if pool.workers.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::styled(
                "No workers in pool",
                Style::default().fg(colors::FG).add_modifier(Modifier::DIM),
            ));
            lines.push(Line::from(""));
            lines.push(Line::styled(
                "Spawn agents with 's' -> enable Sprite -> spawn",
                Style::default().fg(colors::FG).add_modifier(Modifier::DIM),
            ));
        } else {
            // Sort workers by ID for consistent display
            let mut workers: Vec<_> = pool.workers.values().collect();
            workers.sort_by(|a, b| a.id.cmp(&b.id));

            for worker in workers {
                let status_style = match worker.status {
                    SpriteWorkerStatus::Idle => Style::default().fg(colors::IDLE),
                    SpriteWorkerStatus::Working => Style::default().fg(colors::WORKING),
                    SpriteWorkerStatus::Provisioning => Style::default().fg(colors::COMPACTING),
                    SpriteWorkerStatus::Checkpointing => Style::default().fg(colors::COMPACTING),
                    SpriteWorkerStatus::Completed => Style::default().fg(Color::Green),
                    SpriteWorkerStatus::Failed => Style::default().fg(Color::Red),
                    SpriteWorkerStatus::Terminating => Style::default().fg(colors::ATTENTION),
                };

                let status_str = match worker.status {
                    SpriteWorkerStatus::Idle => "Idle",
                    SpriteWorkerStatus::Working => "Working",
                    SpriteWorkerStatus::Provisioning => "Starting...",
                    SpriteWorkerStatus::Checkpointing => "Checkpoint",
                    SpriteWorkerStatus::Completed => "Done",
                    SpriteWorkerStatus::Failed => "Failed",
                    SpriteWorkerStatus::Terminating => "Stopping...",
                };

                let task = worker
                    .task_description
                    .as_ref()
                    .map(|t| t.chars().take(30).collect::<String>())
                    .unwrap_or_else(|| "-".to_string());

                let worker_id = if worker.id.len() > 12 {
                    format!("{}...", &worker.id[..9])
                } else {
                    worker.id.clone()
                };

                let sprite_name = if worker.sprite_name.len() > 16 {
                    format!("{}...", &worker.sprite_name[..13])
                } else {
                    worker.sprite_name.clone()
                };

                lines.push(Line::from(vec![
                    Span::styled(format!("{:12}", worker_id), Style::default().fg(colors::FG)),
                    Span::raw(" │ "),
                    Span::styled(
                        format!("{:16}", sprite_name),
                        Style::default().fg(colors::FG),
                    ),
                    Span::raw(" │ "),
                    Span::styled(format!("{:12}", status_str), status_style),
                    Span::raw(" │ "),
                    Span::styled(format!("{:30}", task), Style::default().fg(colors::FG)),
                ]));
            }
        }

        // Pool summary
        lines.push(Line::styled(
            "─".repeat(75),
            Style::default().fg(colors::BORDER),
        ));

        let idle_count = pool.count_by_status(SpriteWorkerStatus::Idle);
        let working_count = pool.count_by_status(SpriteWorkerStatus::Working);
        let total = pool.workers.len();

        let hybrid_str = if pool.hybrid_mode {
            format!(
                " │ Hybrid Mode (Planner: {})",
                pool.local_planner_pane.as_deref().unwrap_or("unknown")
            )
        } else {
            String::new()
        };

        lines.push(Line::styled(
            format!(
                "Workers: {}/{} │ Idle: {} │ Working: {}{}",
                total, pool.config.max_workers, idle_count, working_count, hybrid_str
            ),
            Style::default().fg(colors::FG).add_modifier(Modifier::DIM),
        ));

        Paragraph::new(lines)
    } else {
        // No pool configured
        let empty_msg = vec![
            Line::from(""),
            Line::styled(
                "No sprite pool configured",
                Style::default().fg(colors::FG).add_modifier(Modifier::DIM),
            ),
            Line::from(""),
            Line::styled("To create a sprite pool:", Style::default().fg(colors::FG)),
            Line::from(""),
            Line::styled(
                "1. Press 's' to open spawn dialog",
                Style::default().fg(colors::FG).add_modifier(Modifier::DIM),
            ),
            Line::styled(
                "2. Enable Sprite Mode",
                Style::default().fg(colors::FG).add_modifier(Modifier::DIM),
            ),
            Line::styled(
                "3. Spawn workers for distributed execution",
                Style::default().fg(colors::FG).add_modifier(Modifier::DIM),
            ),
        ];
        Paragraph::new(empty_msg).alignment(Alignment::Center)
    };

    let pool_widget = content.block(
        Block::default()
            .title(" Sprite Pool Management ")
            .title_bottom(" [P/Esc] Close  [+] Add Worker  [k] Kill  [r] Restart ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::HIGHLIGHT))
            .border_type(ratatui::widgets::BorderType::Double)
            .style(Style::default().bg(colors::BG)),
    );

    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(pool_widget, area);
}
