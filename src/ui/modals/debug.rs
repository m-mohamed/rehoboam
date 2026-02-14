//! Debug log viewer modal
//!
//! Two render modes:
//! 1. **List mode**: Scrollable index of debug log files (session ID, size, age)
//! 2. **Reader mode**: Full log content with level-based coloring

use crate::app::App;
use crate::config::colors;
use crate::plans::{format_relative_time, format_size};
use ratatui::{
    prelude::*,
    widgets::{
        Block, Borders, Clear, List, ListItem, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState,
    },
    Frame,
};

/// Render the debug log viewer overlay (list or reader mode)
pub fn render_debug_viewer(f: &mut Frame, area: Rect, app: &mut App) {
    f.render_widget(Clear, area);

    if app.debug_viewer.viewing {
        render_reader(f, area, app);
    } else {
        render_list(f, area, app);
    }
}

/// Render the debug log list browser
fn render_list(f: &mut Frame, area: Rect, app: &App) {
    let entries = &app.state.debug_log_entries;
    let count = entries.len();
    let title = format!(" Debug Logs ({count}) ");
    let selected = app.debug_viewer.selected_index;

    if entries.is_empty() {
        let msg = Paragraph::new("No debug logs found in ~/.claude/debug/")
            .style(Style::default().fg(colors::IDLE))
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(colors::HIGHLIGHT))
                    .border_type(ratatui::widgets::BorderType::Rounded)
                    .title_bottom(Line::from(" Esc:close ").centered())
                    .style(Style::default().bg(colors::BG)),
            );
        f.render_widget(msg, area);
        return;
    }

    let visible_height = area.height.saturating_sub(4) as usize;
    let offset = if selected >= visible_height {
        selected - visible_height + 1
    } else {
        0
    };

    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible_height)
        .map(|(i, entry)| {
            let is_selected = i == selected;
            let marker = if is_selected { "> " } else { "  " };

            // Truncate session ID for display
            let id_display = if entry.session_id.len() > 10 {
                format!("{}…", &entry.session_id[..9])
            } else {
                entry.session_id.clone()
            };

            let latest_tag = if entry.is_latest { " (latest)" } else { "" };
            let size = format_size(entry.size_bytes);
            let age = format_relative_time(entry.modified);

            let available = area.width.saturating_sub(4) as usize;
            let suffix = format!("{latest_tag}  {size:>6}  {age:>6}");
            let padding_len = available.saturating_sub(id_display.len() + suffix.len() + 2);

            let line = format!(
                "{marker}{id_display}{}{}",
                " ".repeat(padding_len),
                suffix
            );

            let style = if is_selected {
                Style::default()
                    .fg(colors::HIGHLIGHT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors::FG)
            };

            ListItem::new(Line::from(line)).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::HIGHLIGHT))
            .border_type(ratatui::widgets::BorderType::Rounded)
            .title_bottom(Line::from(" j/k:nav  Enter:read  Esc:close ").centered())
            .style(Style::default().bg(colors::BG)),
    );

    f.render_widget(list, area);

    // Scrollbar
    if count > visible_height {
        let scrollbar_area = Rect {
            x: area.x + area.width - 1,
            y: area.y + 1,
            width: 1,
            height: area.height.saturating_sub(2),
        };
        let max_scroll = count.saturating_sub(visible_height);
        let mut state = ScrollbarState::new(max_scroll).position(offset);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(colors::HIGHLIGHT))
                .track_style(Style::default().fg(colors::BORDER)),
            scrollbar_area,
            &mut state,
        );
    }
}

/// Render the debug log reader with level-based coloring
fn render_reader(f: &mut Frame, area: Rect, app: &mut App) {
    let entry_idx = app.debug_viewer.selected_index;
    let total = app.state.debug_log_entries.len();
    let session_id = app
        .state
        .debug_log_entries
        .get(entry_idx)
        .map(|e| {
            if e.session_id.len() > 10 {
                format!("{}…", &e.session_id[..9])
            } else {
                e.session_id.clone()
            }
        })
        .unwrap_or_else(|| "unknown".to_string());

    // Parse content into styled lines with log level coloring
    let lines: Vec<Line> = app
        .debug_viewer
        .content
        .lines()
        .map(|line| {
            let style = if line.contains("[ERROR]") {
                Style::default().fg(Color::Red)
            } else if line.contains("[WARN]") {
                Style::default().fg(Color::Yellow)
            } else if line.contains("[INFO]") {
                Style::default().fg(Color::Cyan)
            } else if line.contains("[DEBUG]") {
                Style::default().fg(colors::IDLE)
            } else {
                Style::default().fg(colors::FG)
            };
            Line::from(Span::styled(line, style))
        })
        .collect();

    let total_lines = lines.len() as u16;
    let inner_height = area.height.saturating_sub(4);
    app.debug_viewer.rendered_height = total_lines.saturating_sub(inner_height);

    // Clamp scroll offset
    if app.debug_viewer.scroll_offset > app.debug_viewer.rendered_height {
        app.debug_viewer.scroll_offset = app.debug_viewer.rendered_height;
    }

    let scroll_pos = app.debug_viewer.scroll_offset;
    let title = format!(" {session_id} ({}/{total}) ", entry_idx + 1);

    let paragraph = Paragraph::new(lines)
        .scroll((scroll_pos, 0))
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::HIGHLIGHT))
                .border_type(ratatui::widgets::BorderType::Rounded)
                .title_bottom(
                    Line::from(" j/k:scroll  d/u:page  g/G:top/bot  Esc:back ").centered(),
                )
                .style(Style::default().bg(colors::BG)),
        );

    f.render_widget(paragraph, area);

    // Scrollbar
    if app.debug_viewer.rendered_height > 0 {
        let scrollbar_area = Rect {
            x: area.x + area.width - 1,
            y: area.y + 1,
            width: 1,
            height: area.height.saturating_sub(2),
        };
        let mut state = ScrollbarState::new(app.debug_viewer.rendered_height as usize)
            .position(scroll_pos as usize);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(colors::HIGHLIGHT))
                .track_style(Style::default().fg(colors::BORDER)),
            scrollbar_area,
            &mut state,
        );
    }
}
