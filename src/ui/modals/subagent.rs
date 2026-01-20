//! Subagent tree panel
//!
//! Displays hierarchical tree of subagents spawned by the selected agent.
//! Shows role badges (P=Planner, W=Worker, R=Reviewer) and proper tree connectors.

use crate::app::App;
use crate::config::colors;
use crate::state::{AgentRole, Subagent};
use ratatui::{
    prelude::*,
    style::Modifier,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::super::helpers::truncate;

/// Render the subagent tree panel
pub fn render_subagent_tree(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let agent = app.state.selected_agent();

    let content = if let Some(agent) = agent {
        if agent.subagents.is_empty() {
            vec![
                Line::from("No subagents spawned."),
                Line::from(""),
                Line::from(Span::styled(
                    "Subagents appear when Claude",
                    Style::default().fg(colors::IDLE),
                )),
                Line::from(Span::styled(
                    "uses the Task tool.",
                    Style::default().fg(colors::IDLE),
                )),
            ]
        } else {
            let mut lines = vec![Line::from(Span::styled(
                format!("Subagents: {}", agent.subagents.len()),
                Style::default()
                    .fg(colors::HIGHLIGHT)
                    .add_modifier(Modifier::BOLD),
            ))];
            lines.push(Line::from(""));

            // Build tree with proper connectors
            let max_width = area.width.saturating_sub(10) as usize;
            build_tree_lines(&agent.subagents, &mut lines, max_width);

            lines
        }
    } else {
        vec![Line::from("Select an agent to see subagents.")]
    };

    let tree = Paragraph::new(content).block(
        Block::default()
            .title(" Subagents [T] ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER)),
    );

    f.render_widget(tree, area);
}

/// Build tree lines with proper connectors and indentation
fn build_tree_lines(subagents: &[Subagent], lines: &mut Vec<Line<'static>>, max_width: usize) {
    for (i, subagent) in subagents.iter().enumerate() {
        let is_last = i == subagents.len() - 1;

        // Tree connector based on position
        let connector = if is_last { "└── " } else { "├── " };
        let continuation = if is_last { "    " } else { "│   " };

        // Indentation based on depth
        let indent = "    ".repeat(subagent.depth as usize);
        let cont_indent = format!("{}{}", indent, continuation);

        // Role badge with color
        let (role_badge, role_color) = role_badge(&subagent.role);

        // Status indicator with color
        let (status_icon, status_color) = match subagent.status.as_str() {
            "running" => ("⚡", colors::WORKING),
            "completed" => ("✓", Color::Green),
            "failed" => ("✗", Color::Red),
            _ => ("?", colors::IDLE),
        };

        // Duration display
        let duration = subagent
            .duration_ms
            .map(|d| {
                if d > 60000 {
                    format!("{}m", d / 60000)
                } else if d > 1000 {
                    format!("{}s", d / 1000)
                } else {
                    format!("{}ms", d)
                }
            })
            .unwrap_or_else(|| "...".to_string());

        // Main line: connector + role badge + status + id
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}{}", indent, connector),
                Style::default().fg(colors::BORDER),
            ),
            Span::styled(
                format!("[{}]", role_badge),
                Style::default()
                    .fg(role_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(format!("{} ", status_icon), Style::default().fg(status_color)),
            Span::styled(
                truncate(&subagent.id, 12),
                Style::default().fg(colors::FG),
            ),
        ]));

        // Description line (indented)
        let desc = truncate(&subagent.description, max_width.saturating_sub(indent.len() + 4));
        lines.push(Line::from(vec![
            Span::styled(cont_indent.clone(), Style::default().fg(colors::BORDER)),
            Span::styled(desc, Style::default().fg(colors::IDLE)),
        ]));

        // Duration line (indented)
        lines.push(Line::from(vec![
            Span::styled(cont_indent, Style::default().fg(colors::BORDER)),
            Span::styled(duration, Style::default().fg(colors::COMPACTING)),
        ]));
    }
}

/// Get role badge and color for display
fn role_badge(role: &AgentRole) -> (&'static str, Color) {
    match role {
        AgentRole::Planner => ("P", colors::HIGHLIGHT),  // Purple for Planner
        AgentRole::Worker => ("W", colors::WORKING),     // Blue for Worker
        AgentRole::Reviewer => ("R", Color::Green),      // Green for Reviewer
        AgentRole::General => ("G", colors::IDLE),       // Gray for General
    }
}
