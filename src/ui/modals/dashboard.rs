//! Dashboard modal

use crate::app::App;
use crate::config::colors;
use crate::state::TaskStatus;
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

    // Calculate Claude Code version stats
    let mut version_counts: HashMap<String, u32> = HashMap::new();
    for agent in app.state.agents.values() {
        if let Some(ref version) = agent.claude_code_version {
            *version_counts.entry(version.clone()).or_insert(0) += 1;
        }
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

    // Add Claude Code version breakdown (if any agents have version info)
    if !version_counts.is_empty() {
        lines.push(String::new());
        lines.push("  ┌─ Claude Code Versions ─────────────────┐".to_string());

        let mut versions: Vec<_> = version_counts.into_iter().collect();
        versions.sort_by(|a, b| {
            // Sort by version string descending (newer first)
            b.0.cmp(&a.0)
        });

        for (version, count) in versions.iter().take(5) {
            let display = if version.len() > 20 {
                format!("{}…", &version[..19])
            } else {
                version.clone()
            };
            lines.push(format!("  │ {:20} {:>10} agents │", display, count));
        }
        if versions.len() > 5 {
            lines.push(format!(
                "  │ ... and {} more versions              │",
                versions.len() - 5
            ));
        }
        lines.push("  └──────────────────────────────────────┘".to_string());
    }

    // Collect all tasks from all agents for dependency visualization
    let mut all_tasks: Vec<_> = app
        .state
        .agents
        .values()
        .flat_map(|agent| agent.tasks.values())
        .collect();

    // Show active tasks section if any agents have tasks
    if !all_tasks.is_empty() {
        lines.push(String::new());
        lines.push("  ┌─ Active Tasks ──────────────────────────┐".to_string());

        // Sort tasks: incomplete first, then by ID
        all_tasks.sort_by(|a, b| {
            let a_complete = matches!(a.status, TaskStatus::Completed);
            let b_complete = matches!(b.status, TaskStatus::Completed);
            a_complete.cmp(&b_complete).then_with(|| a.id.cmp(&b.id))
        });

        // Display tasks with dependency indicators
        for task in all_tasks.iter().take(8) {
            let indicator = task.status.indicator();
            let is_blocked = !task.blocked_by.is_empty()
                && task
                    .blocked_by
                    .iter()
                    .any(|id| all_tasks.iter().any(|t| &t.id == id && !matches!(t.status, TaskStatus::Completed)));

            let prefix = if is_blocked {
                "  → " // Indented, blocked
            } else {
                "" // Root or independent task
            };

            let subject = if task.subject.len() > 30 {
                format!("{}…", &task.subject[..29])
            } else if task.subject.is_empty() {
                format!("Task #{}", task.id)
            } else {
                task.subject.clone()
            };

            let blocked_suffix = if is_blocked { " (blocked)" } else { "" };

            lines.push(format!(
                "  │ {} [{}] {}{}{}",
                indicator,
                task.id,
                prefix,
                subject,
                blocked_suffix
            ));
        }

        if all_tasks.len() > 8 {
            lines.push(format!(
                "  │ ... and {} more tasks                    ",
                all_tasks.len() - 8
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
