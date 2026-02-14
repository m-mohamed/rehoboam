//! Insights report viewer modal
//!
//! Displays the parsed HTML report from Claude Code's `/insights` command
//! with tabbed sections, prose text, and bar charts.

use crate::app::App;
use crate::config::colors;
use ratatui::{
    prelude::*,
    widgets::{
        Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Tabs,
    },
    Frame,
};

/// Render the insights report viewer overlay
pub fn render_insights_viewer(f: &mut Frame, area: Rect, app: &mut App) {
    f.render_widget(Clear, area);

    let report = match &app.state.insights_report {
        Some(r) if !r.sections.is_empty() => r,
        _ => {
            let msg = Paragraph::new(
                "No insights report found.\nRun /insights in Claude Code to generate one.",
            )
            .style(Style::default().fg(colors::IDLE))
            .block(
                Block::default()
                    .title(" Claude Code Insights ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(colors::HIGHLIGHT))
                    .border_type(ratatui::widgets::BorderType::Rounded)
                    .title_bottom(Line::from(" Esc:close ").centered())
                    .style(Style::default().bg(colors::BG)),
            );
            f.render_widget(msg, area);
            return;
        }
    };

    let section_count = report.sections.len();
    let active = app.insights_viewer.active_section.min(section_count.saturating_sub(1));

    // Build tab titles (truncated to fit)
    let tab_titles: Vec<String> = report
        .sections
        .iter()
        .map(|s| {
            if s.title.len() > 12 {
                format!("{}…", &s.title[..11])
            } else {
                s.title.clone()
            }
        })
        .collect();

    // Split into tab bar + content
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    // Tab bar
    let tabs = Tabs::new(tab_titles)
        .select(active)
        .style(Style::default().fg(colors::FG))
        .highlight_style(
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )
        .divider(" | ")
        .block(
            Block::default()
                .title(" Claude Code Insights ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::HIGHLIGHT))
                .border_type(ratatui::widgets::BorderType::Rounded)
                .style(Style::default().bg(colors::BG)),
        );
    f.render_widget(tabs, chunks[0]);

    // Render active section content
    // Clone the data we need to avoid borrow conflicts
    let section_title = report.sections[active].title.clone();
    let section_content = report.sections[active].content.clone();
    let section_bars = report.sections[active].bars.clone();
    render_section(f, chunks[1], app, &section_title, &section_content, &section_bars);
}

fn render_section(
    f: &mut Frame,
    area: Rect,
    app: &mut App,
    title: &str,
    content: &str,
    bars: &[crate::state::InsightsBar],
) {
    let mut lines: Vec<Line> = Vec::new();

    // Section title
    lines.push(Line::from(Span::styled(
        title,
        Style::default()
            .fg(colors::HIGHLIGHT)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    // Prose content
    for paragraph in content.split('\n') {
        let trimmed = paragraph.trim();
        if trimmed.is_empty() {
            lines.push(Line::from(""));
        } else {
            // Wrap long lines manually for the available width
            let width = area.width.saturating_sub(4) as usize;
            let mut remaining = trimmed;
            while !remaining.is_empty() {
                if remaining.len() <= width {
                    lines.push(Line::from(format!("  {remaining}")));
                    break;
                }
                // Find a good break point
                let break_at = remaining[..width]
                    .rfind(' ')
                    .unwrap_or(width);
                lines.push(Line::from(format!("  {}", &remaining[..break_at])));
                remaining = remaining[break_at..].trim_start();
            }
        }
    }

    // Bar charts
    if !bars.is_empty() {
        lines.push(Line::from(""));

        let max_label = bars.iter().map(|b| b.label.len()).max().unwrap_or(10);
        let bar_width = 20usize;

        for bar in bars {
            let filled = (bar.percentage * bar_width as f32) as usize;
            let empty = bar_width.saturating_sub(filled);
            let label = format!("{:>width$}", bar.label, width = max_label);

            lines.push(Line::from(vec![
                Span::styled(format!("  {label} "), Style::default().fg(colors::FG)),
                Span::styled("█".repeat(filled.max(1)), Style::default().fg(colors::WORKING)),
                Span::raw(" ".repeat(empty)),
                Span::styled(
                    format!(" {}", bar.value),
                    Style::default().fg(colors::IDLE),
                ),
            ]));
        }
    }

    let total_lines = lines.len() as u16;
    let inner_height = area.height.saturating_sub(4);
    let max_scroll = total_lines.saturating_sub(inner_height);
    if app.insights_viewer.scroll_offset > max_scroll {
        app.insights_viewer.scroll_offset = max_scroll;
    }

    let scroll_pos = app.insights_viewer.scroll_offset;

    let paragraph = Paragraph::new(lines)
        .scroll((scroll_pos, 0))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER))
                .border_type(ratatui::widgets::BorderType::Rounded)
                .title_bottom(
                    Line::from(" Tab:sections  j/k:scroll  Esc:close ").centered(),
                )
                .style(Style::default().bg(colors::BG)),
        );

    f.render_widget(paragraph, area);

    // Scrollbar
    if max_scroll > 0 {
        let scrollbar_area = Rect {
            x: area.x + area.width - 1,
            y: area.y + 1,
            width: 1,
            height: area.height.saturating_sub(2),
        };
        let mut state = ScrollbarState::new(max_scroll as usize).position(scroll_pos as usize);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(colors::HIGHLIGHT))
                .track_style(Style::default().fg(colors::BORDER)),
            scrollbar_area,
            &mut state,
        );
    }
}
