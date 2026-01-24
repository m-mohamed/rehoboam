//! Dashboard modal

use crate::app::App;
use crate::config::colors;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::collections::HashMap;

use super::super::helpers::centered_rect;

pub fn render_dashboard(f: &mut Frame, app: &App) {
    let area = centered_rect(70, 70, f.area());

    // Calculate session duration
    let session_secs = app.session_start.elapsed().as_secs();
    let session_hours = session_secs / 3600;
    let session_mins = (session_secs % 3600) / 60;
    let session_str = if session_hours > 0 {
        format!("{}h {}m", session_hours, session_mins)
    } else {
        format!("{}m", session_mins)
    };

    // Get counts
    let [attention, working, compacting] = app.state.status_counts;
    let total = app.state.agents.len();
    let sprite_count = app.state.sprite_agent_count();
    let local_count = total - sprite_count;

    // Calculate project stats
    let mut project_counts: HashMap<String, u32> = HashMap::new();
    for agent in app.state.agents.values() {
        *project_counts.entry(agent.project.clone()).or_insert(0) += 1;
    }

    // Build dashboard text
    let mut lines = vec![
        String::new(),
        format!(
            "  SESSION: {}            AGENTS: {} ({} local, {} sprites)",
            session_str, total, local_count, sprite_count
        ),
        String::new(),
        "  ┌─ Status ──────────────┐".to_string(),
        format!("  │ 🔔 Attention:   {:>3}   │", attention),
        format!("  │ 🤖 Working:     {:>3}   │", working),
        format!("  │ 🔄 Compacting:  {:>3}   │", compacting),
        "  └──────────────────────┘".to_string(),
        String::new(),
    ];

    // Add project breakdown (top 5)
    if !project_counts.is_empty() {
        lines.push("  ┌─ Projects ─────────────────────────────┐".to_string());

        let mut projects: Vec<_> = project_counts.into_iter().collect();
        projects.sort_by(|a, b| b.1.cmp(&a.1)); // Sort by count desc

        for (project, count) in projects.iter().take(5) {
            let display = if project.len() > 28 {
                format!("{}…", &project[..27])
            } else {
                project.clone()
            };
            lines.push(format!("  │ {:28} {:>4} │", display, count));
        }
        if projects.len() > 5 {
            lines.push(format!(
                "  │ ... and {} more projects        │",
                projects.len() - 5
            ));
        }
        lines.push("  └──────────────────────────────────────┘".to_string());
    }

    lines.push(String::new());
    lines.push("  Press 'd' to close".to_string());

    let text = lines.join("\n");

    let dashboard = Paragraph::new(text)
        .style(Style::default().fg(colors::FG))
        .block(
            Block::default()
                .title(" Dashboard ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER)),
        );

    f.render_widget(dashboard, area);
}
