//! Team view - agents grouped by team with hierarchy

use crate::app::App;
use crate::config::colors;
use crate::state::Status;
use ratatui::{
    prelude::*,
    style::Modifier,
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

/// Render agents grouped by team with tree hierarchy
pub fn render_team_view(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let teams = app.state.agents_by_team();

    // Get selected agent's pane_id for highlighting
    let selected_pane_id = app.state.selected_agent().map(|a| a.pane_id.as_str());

    if teams.is_empty() {
        let placeholder = Block::default()
            .title(" Teams ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER))
            .border_type(ratatui::widgets::BorderType::Rounded);
        f.render_widget(placeholder, area);
        return;
    }

    let mut items: Vec<ListItem> = Vec::new();

    for (team_name, agents) in &teams {
        // Team header
        let team_icon = if team_name == "Independent" {
            "\u{1f464}" // 👤
        } else {
            "\u{1f465}" // 👥
        };
        let header = format!(
            "{} {} ({} agent{})",
            team_icon,
            team_name,
            agents.len(),
            if agents.len() == 1 { "" } else { "s" }
        );
        items.push(ListItem::new(Line::from(vec![Span::styled(
            header,
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )])));

        // Agent entries with tree glyphs
        let agent_count = agents.len();
        for (i, agent) in agents.iter().enumerate() {
            let is_last = i == agent_count - 1;
            let glyph = if is_last {
                "\u{2514}\u{2500}"
            } else {
                "\u{251c}\u{2500}"
            }; // └─ or ├─

            let (icon, color) = match &agent.status {
                Status::Attention(_) => ("\u{1f514}", colors::ATTENTION), // 🔔
                Status::Working => ("\u{1f916}", colors::WORKING),        // 🤖
                Status::Compacting => ("\u{1f504}", colors::COMPACTING),  // 🔄
            };

            let status_str = match &agent.status {
                Status::Attention(_) => "Attention",
                Status::Working => "Working",
                Status::Compacting => "Compacting",
            };

            // Crown prefix for team leads
            let lead_prefix = if agent.team_agent_type.as_deref() == Some("lead") {
                "\u{1f451} " // 👑
            } else {
                ""
            };

            // Prefer team_agent_name, fall back to pane_id
            let display_name = agent.team_agent_name.as_deref().unwrap_or(&agent.pane_id);

            let tool_info = agent.tool_display();
            let elapsed = agent.elapsed_display();

            let is_selected = selected_pane_id == Some(agent.pane_id.as_str());
            let select_prefix = if is_selected { "\u{25b6} " } else { "  " }; // ▶ or spaces

            let line = format!(
                "{}{} {}{} {} ({}) {} {}",
                select_prefix,
                glyph,
                lead_prefix,
                icon,
                display_name,
                status_str,
                tool_info,
                elapsed
            );

            let style = if is_selected {
                Style::default()
                    .fg(color)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(color)
            };

            items.push(ListItem::new(Line::from(vec![Span::styled(line, style)])));

            // Show current_task_subject indented below agent when present
            if let Some(ref task_subject) = agent.current_task_subject {
                let continuation = if is_last { "   " } else { "\u{2502}  " }; // │ or space
                let task_line = format!("  {}  \u{1f4cb} {}", continuation, task_subject); // 📋
                items.push(ListItem::new(Line::from(vec![Span::styled(
                    task_line,
                    Style::default().fg(colors::IDLE),
                )])));
            }
        }

        // Aggregate task progress for the team
        let (team_completed, team_total) = agents.iter().fold((0usize, 0usize), |acc, agent| {
            let (c, t) = agent.task_progress();
            (acc.0 + c, acc.1 + t)
        });

        if team_total > 0 {
            let pct = if team_total > 0 {
                (team_completed as f64 / team_total as f64 * 100.0) as usize
            } else {
                0
            };

            // Build progress bar: 12 chars wide
            let filled = (team_completed * 12) / team_total.max(1);
            let empty = 12 - filled;
            let bar = format!(
                "  [{}{}] {}/{} tasks ({}%)",
                "\u{2588}".repeat(filled), // █
                "\u{2591}".repeat(empty),  // ░
                team_completed,
                team_total,
                pct
            );

            items.push(ListItem::new(Line::from(vec![Span::styled(
                bar,
                Style::default().fg(colors::WORKING),
            )])));
        }

        // Spacing between teams
        items.push(ListItem::new(""));
    }

    let list = List::new(items).block(
        Block::default()
            .title(" Teams ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER))
            .border_type(ratatui::widgets::BorderType::Rounded),
    );

    f.render_widget(list, area);
}
