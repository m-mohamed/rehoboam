//! Dashboard modal

use crate::app::App;
use crate::config::colors;
use crate::state::LoopMode;
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

    // Calculate loop stats
    let mut total_iterations: u32 = 0;
    let mut active_loops: u32 = 0;
    let mut completed_loops: u32 = 0;
    let mut project_counts: HashMap<String, (u32, u32)> = HashMap::new(); // (agents, iters)

    for agent in app.state.agents.values() {
        total_iterations += agent.loop_iteration;

        match agent.loop_mode {
            LoopMode::Active => active_loops += 1,
            LoopMode::Complete => completed_loops += 1,
            _ => {}
        }

        // Group by project
        let entry = project_counts
            .entry(agent.project.clone())
            .or_insert((0, 0));
        entry.0 += 1;
        entry.1 += agent.loop_iteration;
    }

    // Build dashboard text
    let mut lines = vec![
        String::new(),
        format!(
            "  SESSION: {}            AGENTS: {} ({} local, {} sprites)",
            session_str, total, local_count, sprite_count
        ),
        String::new(),
        format!("  ┌─ Status ──────────────┐  ┌─ Loop Progress ─────────────┐"),
        format!(
            "  │ 🔔 Attention:   {:>3}   │  │ Total iterations:   {:>5}   │",
            attention, total_iterations
        ),
        format!(
            "  │ 🤖 Working:     {:>3}   │  │ Completed loops:    {:>5}   │",
            working, completed_loops
        ),
        format!(
            "  │ 🔄 Compacting:  {:>3}   │  │ Active loops:       {:>5}   │",
            compacting, active_loops
        ),
        "  └──────────────────────┘  └─────────────────────────────┘".to_string(),
        String::new(),
    ];

    // Add OTEL Fleet Metrics
    let tool_calls = crate::telemetry::metrics::get_tool_calls();
    let otel_iterations = crate::telemetry::metrics::get_iterations();

    // Calculate average latency from agent data
    let avg_latency: f64 = {
        let latencies: Vec<u64> = app
            .state
            .agents
            .values()
            .filter_map(|a| a.avg_latency_ms)
            .collect();
        if latencies.is_empty() {
            0.0
        } else {
            latencies.iter().sum::<u64>() as f64 / latencies.len() as f64
        }
    };

    lines.push(String::new());
    lines.push("  ┌─ Fleet Metrics (OTEL) ────────────────────────────────┐".to_string());
    lines.push(format!(
        "  │ Tool Calls: {:>8}   Avg Latency: {:>6.1}ms           │",
        tool_calls, avg_latency
    ));
    lines.push(format!(
        "  │ Traced Iterations: {:>5}   Connected Sprites: {:>3}   │",
        otel_iterations,
        app.state.connected_sprite_count()
    ));
    lines.push("  └──────────────────────────────────────────────────────┘".to_string());

    // Add project breakdown
    if !project_counts.is_empty() {
        lines.push(String::new());
        lines.push("  ┌─ By Project ───────────────────────────────────────┐".to_string());
        let mut projects: Vec<_> = project_counts.iter().collect();
        projects.sort_by(|a, b| b.1 .1.cmp(&a.1 .1)); // Sort by iterations desc

        for (project, (agents, iters)) in projects.iter().take(5) {
            let bar_len = (*iters as usize).min(20);
            let bar = "█".repeat(bar_len);
            let project_short = if project.len() > 15 {
                format!("{}...", &project[..12])
            } else {
                project.to_string()
            };
            lines.push(format!(
                "  │ {:15} {:20} {:>2} agents {:>4} iters │",
                project_short, bar, agents, iters
            ));
        }
        lines.push("  └─────────────────────────────────────────────────────┘".to_string());
    }

    let dashboard_text = lines.join("\n");

    let dashboard = Paragraph::new(dashboard_text)
        .style(Style::default().fg(colors::FG))
        .block(
            Block::default()
                .title(" Rehoboam Dashboard ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::HIGHLIGHT))
                .border_type(ratatui::widgets::BorderType::Double)
                .title_bottom(Line::from(" d:close ").centered())
                .style(Style::default().bg(colors::BG)),
        );

    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(dashboard, area);
}
