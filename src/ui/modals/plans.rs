//! Plan browser and reader modal
//!
//! Two render modes:
//! 1. **List mode**: Scrollable list of plans (filename, modified date, size)
//! 2. **Reader mode**: Full rendered markdown with syntax-highlighted code blocks

use crate::app::App;
use crate::config::colors;
use crate::plans::{format_relative_time, format_size};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};

/// Render the plan viewer overlay (list or reader mode)
pub fn render_plan_viewer(f: &mut Frame, area: Rect, app: &mut App) {
    f.render_widget(Clear, area);

    if app.plan_viewer.viewing {
        render_reader(f, area, app);
    } else {
        render_list(f, area, app);
    }
}

/// Render the plan list browser
fn render_list(f: &mut Frame, area: Rect, app: &App) {
    let count = app.plan_viewer.plans.len();
    let title = format!(" Plans ({count}) ");

    let items: Vec<ListItem> = app
        .plan_viewer
        .plans
        .iter()
        .enumerate()
        .map(|(i, plan)| {
            let age = format_relative_time(plan.modified);
            let size = format_size(plan.size_bytes);
            let marker = if i == app.plan_viewer.selected_index {
                ">"
            } else {
                " "
            };
            let style = if i == app.plan_viewer.selected_index {
                Style::default()
                    .fg(colors::HIGHLIGHT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors::FG)
            };

            // Truncate name to fit, right-align age and size
            let available = area.width.saturating_sub(4) as usize; // borders + padding
            let suffix = format!(" {size:>4} {age:>4}");
            let name_max = available.saturating_sub(suffix.len() + 2);
            let name = if plan.name.len() > name_max {
                format!("{}…", &plan.name[..name_max.saturating_sub(1)])
            } else {
                plan.name.clone()
            };
            let padding = available.saturating_sub(name.len() + suffix.len() + 2);

            let line = format!("{marker} {name}{}{suffix}", " ".repeat(padding));
            ListItem::new(Line::from(line)).style(style)
        })
        .collect();

    // Slice items to visible window
    let visible_height = area.height.saturating_sub(4) as usize; // borders + footer
    let offset = if app.plan_viewer.selected_index >= visible_height {
        app.plan_viewer.selected_index - visible_height + 1
    } else {
        0
    };
    let visible_items: Vec<ListItem> = items.into_iter().skip(offset).take(visible_height).collect();

    let list = List::new(visible_items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::HIGHLIGHT))
                .border_type(ratatui::widgets::BorderType::Rounded)
                .title_bottom(Line::from(" j/k:nav  Enter:read  Esc:close ").centered())
                .style(Style::default().bg(colors::BG)),
        );

    f.render_widget(list, area);
}

/// Render the markdown reader
fn render_reader(f: &mut Frame, area: Rect, app: &mut App) {
    let plan_name = app
        .plan_viewer
        .plans
        .get(app.plan_viewer.selected_index)
        .map(|p| p.name.as_str())
        .unwrap_or("unknown");

    // Convert markdown to styled ratatui Text
    let rendered = tui_markdown::from_str(&app.plan_viewer.content);
    let total_lines = rendered.height() as u16;

    // Update rendered height for scroll bounds
    let inner_height = area.height.saturating_sub(4); // borders + title + footer
    app.plan_viewer.rendered_height = total_lines.saturating_sub(inner_height);

    // Clamp scroll offset
    if app.plan_viewer.scroll_offset > app.plan_viewer.rendered_height {
        app.plan_viewer.scroll_offset = app.plan_viewer.rendered_height;
    }

    let scroll_pos = app.plan_viewer.scroll_offset;
    let title = format!(
        " {plan_name}.md ({}/{}) ",
        scroll_pos + 1,
        total_lines.max(1)
    );

    let paragraph = Paragraph::new(rendered)
        .scroll((scroll_pos, 0))
        .wrap(ratatui::widgets::Wrap { trim: false })
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::HIGHLIGHT))
                .border_type(ratatui::widgets::BorderType::Rounded)
                .title_bottom(
                    Line::from(" j/k:scroll  d/u:page  g/G:top/bot  n/p:next  Esc:back ")
                        .centered(),
                )
                .style(Style::default().bg(colors::BG)),
        );

    f.render_widget(paragraph, area);

    // Render scrollbar
    if total_lines > inner_height {
        let scrollbar_area = Rect {
            x: area.x + area.width - 1,
            y: area.y + 1,
            width: 1,
            height: area.height.saturating_sub(2),
        };
        let mut scrollbar_state = ScrollbarState::new(app.plan_viewer.rendered_height as usize)
            .position(scroll_pos as usize);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(colors::HIGHLIGHT))
                .track_style(Style::default().fg(colors::BORDER)),
            scrollbar_area,
            &mut scrollbar_state,
        );
    }
}
