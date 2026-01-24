//! Split view - agent list on left, live output on right

use crate::app::App;
use crate::config::colors;
use crate::state::Status;
use crate::ui::helpers::truncate;
use crate::ui::modals::render_subagent_tree;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    prelude::*,
    style::Modifier,
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

/// Render split view: agent list on left, live output on right
pub fn render_split_view(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    // Calculate split ratio based on whether subagent panel is shown
    let constraints = if app.show_subagents {
        vec![
            Constraint::Percentage(25), // Agent list
            Constraint::Percentage(50), // Live output
            Constraint::Percentage(25), // Subagent tree
        ]
    } else {
        vec![
            Constraint::Percentage(30), // Agent list
            Constraint::Percentage(70), // Live output
        ]
    };

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    // Left panel: Agent list (compact)
    render_agent_list_compact(f, chunks[0], app);

    // Right panel: Live output
    render_live_output(f, chunks[1], app);

    // Subagent panel (if shown)
    if app.show_subagents && chunks.len() > 2 {
        render_subagent_tree(f, chunks[2], app);
    }
}

/// Render compact agent list for split view
fn render_agent_list_compact(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let columns = app.state.agents_by_column();
    let agents: Vec<&_> = columns.iter().flatten().copied().collect();

    if agents.is_empty() {
        let placeholder = Paragraph::new("No agents running.\n\nPress 's' to spawn.")
            .style(Style::default().fg(colors::IDLE))
            .block(
                Block::default()
                    .title(" Agents ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(colors::BORDER)),
            );
        f.render_widget(placeholder, area);
        return;
    }

    let selected_pane = app.state.selected_agent().map(|a| a.pane_id.as_str());

    let items: Vec<ListItem> = agents
        .iter()
        .map(|agent| {
            let (icon, color) = match &agent.status {
                Status::Attention(_) => ("🔔", colors::ATTENTION),
                Status::Working => ("🤖", colors::WORKING),
                Status::Compacting => ("🔄", colors::COMPACTING),
            };

            let sprite_prefix = if agent.is_sprite { "☁" } else { "" };
            let selected = if Some(agent.pane_id.as_str()) == selected_pane {
                "▶"
            } else {
                " "
            };

            let line = format!(
                "{} {}{} {}",
                selected,
                sprite_prefix,
                icon,
                truncate(&agent.project, 15)
            );

            let style = if Some(agent.pane_id.as_str()) == selected_pane {
                Style::default().fg(color).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(color)
            };

            ListItem::new(Line::from(vec![Span::styled(line, style)]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" Agents [j/k] ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::HIGHLIGHT)),
    );

    f.render_widget(list, area);
}

/// Render live output panel
fn render_live_output(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let output_lines: Vec<Line> = app
        .live_output
        .lines()
        .skip(app.output_scroll as usize)
        .take(area.height.saturating_sub(2) as usize)
        .map(|line| {
            // Color code based on content
            let style = if line.starts_with("Error") || line.contains("error") {
                Style::default().fg(Color::Red)
            } else if line.starts_with('>') || line.starts_with("claude") {
                Style::default().fg(colors::WORKING)
            } else if line.contains("✓") || line.contains("passed") {
                Style::default().fg(Color::Green)
            } else if line.contains("warning") {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(colors::FG)
            };
            Line::from(vec![Span::styled(line, style)])
        })
        .collect();

    let title = if let Some(agent) = app.state.selected_agent() {
        format!(
            " {} | {} | {:?} ",
            agent.project, agent.pane_id, agent.status
        )
    } else {
        " Live Output ".to_string()
    };

    let output = Paragraph::new(output_lines)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::HIGHLIGHT))
                .border_type(ratatui::widgets::BorderType::Rounded),
        )
        .wrap(ratatui::widgets::Wrap { trim: false });

    f.render_widget(output, area);
}
