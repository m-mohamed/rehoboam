//! Project view - agents grouped by project

use crate::app::App;
use crate::config::colors;
use crate::state::Status;
use ratatui::{
    prelude::*,
    style::Modifier,
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

/// Render agents grouped by project
pub fn render_project_view(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let projects = app.state.agents_by_project();

    if projects.is_empty() {
        let placeholder = Block::default()
            .title(" Projects ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER))
            .border_type(ratatui::widgets::BorderType::Rounded);
        f.render_widget(placeholder, area);
        return;
    }

    // Create scrollable list of projects with their agents
    let mut items: Vec<ListItem> = Vec::new();

    for (project_name, agents) in &projects {
        // Project header
        let header = format!(
            "📁 {} ({} agent{})",
            project_name,
            agents.len(),
            if agents.len() == 1 { "" } else { "s" }
        );
        items.push(ListItem::new(Line::from(vec![Span::styled(
            header,
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )])));

        // Agent entries under this project
        for agent in agents {
            let (icon, color) = match &agent.status {
                Status::Attention(_) => ("🔔", colors::ATTENTION),
                Status::Working => ("🤖", colors::WORKING),
                Status::Compacting => ("🔄", colors::COMPACTING),
            };

            let status_str = match &agent.status {
                Status::Attention(_) => "Attention",
                Status::Working => "Working",
                Status::Compacting => "Compacting",
            };

            // Sprite indicator for remote agents
            let sprite_prefix = if agent.is_sprite { "☁ " } else { "" };

            let tool_info = agent.tool_display();
            let elapsed = agent.elapsed_display();

            let line = format!(
                "  {}{} {} ({}) {} {}",
                sprite_prefix, icon, agent.pane_id, status_str, tool_info, elapsed
            );

            items.push(ListItem::new(Line::from(vec![Span::styled(
                line,
                Style::default().fg(color),
            )])));
        }

        // Add spacing between projects
        items.push(ListItem::new(""));
    }

    let list = List::new(items).block(
        Block::default()
            .title(" Projects [v:view] ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER))
            .border_type(ratatui::widgets::BorderType::Rounded),
    );

    f.render_widget(list, area);
}
