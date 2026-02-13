//! Task board view - tasks grouped by team in Pending/In Progress/Completed columns

use crate::app::App;
use crate::config::colors;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    prelude::*,
    style::Modifier,
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

/// Render the task board: tasks grouped by team with 3 status columns
pub fn render_task_board(f: &mut Frame, area: Rect, app: &App) {
    let teams = app.state.tasks_by_team();

    if teams.is_empty() {
        let empty = Paragraph::new("No tasks tracked. Press T or Esc to close.")
            .alignment(Alignment::Center)
            .style(Style::default().fg(colors::IDLE))
            .block(
                Block::default()
                    .title(" Task Board ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(colors::BORDER))
                    .border_type(ratatui::widgets::BorderType::Rounded),
            );
        f.render_widget(empty, area);
        return;
    }

    // Split vertically for multiple teams, or use full area for one
    let team_areas = if teams.len() == 1 {
        vec![area]
    } else {
        let constraints: Vec<Constraint> = teams
            .iter()
            .map(|_| Constraint::Ratio(1, teams.len() as u32))
            .collect();
        Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area)
            .to_vec()
    };

    for (i, (team_name, columns)) in teams.iter().enumerate() {
        if i >= team_areas.len() {
            break;
        }
        render_team_section(f, team_areas[i], team_name, columns);
    }
}

/// Render a single team's task section with 3 columns
fn render_team_section(
    f: &mut Frame,
    area: Rect,
    team_name: &str,
    columns: &[Vec<crate::state::TaskWithContext>; 3],
) {
    let total: usize = columns.iter().map(|c| c.len()).sum();
    let completed = columns[2].len();

    // Team header (1 line) + 3-column body
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(4)])
        .split(area);

    // Team header
    let header = format!(
        "\u{1f465} {} ({} tasks, {}/{} done)",
        team_name, total, completed, total
    );
    let header_widget = Paragraph::new(header)
        .style(
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Left);
    f.render_widget(header_widget, chunks[0]);

    // 3-column horizontal split
    let col_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(chunks[1]);

    let col_configs = [
        ("Pending", colors::ATTENTION, &columns[0]),
        ("In Progress", colors::WORKING, &columns[1]),
        ("Completed", colors::IDLE, &columns[2]),
    ];

    for (j, (title, color, tasks)) in col_configs.iter().enumerate() {
        let items: Vec<ListItem> = tasks
            .iter()
            .map(|task| {
                let indicator = task.status.indicator();
                let line1 = format!("{} {}", indicator, task.subject);

                let mut meta_parts: Vec<String> = Vec::new();
                if !task.owner_name.is_empty() {
                    meta_parts.push(format!("@{}", task.owner_name));
                }
                if !task.blocked_by.is_empty() {
                    meta_parts.push(format!("blocked:{}", task.blocked_by.len()));
                }

                let lines = if meta_parts.is_empty() {
                    vec![Line::from(Span::styled(line1, Style::default().fg(*color)))]
                } else {
                    vec![
                        Line::from(Span::styled(line1, Style::default().fg(*color))),
                        Line::from(Span::styled(
                            format!("  {}", meta_parts.join(" ")),
                            Style::default().fg(colors::IDLE),
                        )),
                    ]
                };

                ListItem::new(lines)
            })
            .collect();

        let count_label = format!(" {} ({}) ", title, tasks.len());
        let list = List::new(items).block(
            Block::default()
                .title(count_label)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(*color))
                .border_type(ratatui::widgets::BorderType::Rounded),
        );

        f.render_widget(list, col_chunks[j]);
    }
}
