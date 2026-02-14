//! History timeline modal
//!
//! Displays a scrollable list of user inputs from ~/.claude/history.jsonl
//! with timestamps, project names, and paste indicators.

use crate::app::App;
use crate::config::colors;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};

/// Render the history timeline overlay
pub fn render_history_viewer(f: &mut Frame, area: Rect, app: &mut App) {
    f.render_widget(Clear, area);

    let entries = &app.state.history_entries;
    let count = entries.len();
    let title = format!(" History ({count}) ");
    let selected = app.history_viewer.selected_index;

    if entries.is_empty() {
        let msg = ratatui::widgets::Paragraph::new(
            "No history found. Use Claude Code to generate history.",
        )
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

    // Calculate scroll offset to keep selection visible
    let scroll_offset = if selected >= app.history_viewer.scroll_offset + visible_height {
        selected - visible_height + 1
    } else if selected < app.history_viewer.scroll_offset {
        selected
    } else {
        app.history_viewer.scroll_offset
    };
    app.history_viewer.scroll_offset = scroll_offset;

    let items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_height)
        .map(|(i, entry)| {
            let is_selected = i == selected;

            // Format timestamp
            let ts = format_timestamp(entry.timestamp);

            // Extract project short name (last path component)
            let project = entry
                .project
                .rsplit('/')
                .next()
                .unwrap_or(&entry.project);
            let project = if project.len() > 14 {
                format!("{}…", &project[..13])
            } else {
                format!("{:<14}", project)
            };

            // Paste indicator
            let paste_indicator = if entry.has_pasted { " + " } else { "   " };

            // Truncate display text
            let available = area.width.saturating_sub(4) as usize; // borders
            let prefix_len = 17 + 15 + 3; // timestamp + project + paste
            let text_max = available.saturating_sub(prefix_len);
            let display = if entry.display.len() > text_max {
                format!("{}…", &entry.display[..text_max.saturating_sub(1)])
            } else {
                entry.display.clone()
            };

            let marker = if is_selected { "> " } else { "  " };

            let style = if is_selected {
                Style::default()
                    .fg(colors::HIGHLIGHT)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors::FG)
            };

            let line = format!("{marker}{ts}  {project}{paste_indicator}{display}");
            ListItem::new(Line::from(line)).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::HIGHLIGHT))
            .border_type(ratatui::widgets::BorderType::Rounded)
            .title_bottom(Line::from(" j/k:scroll  Esc:close ").centered())
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
        let mut state = ScrollbarState::new(max_scroll).position(scroll_offset);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(colors::HIGHLIGHT))
                .track_style(Style::default().fg(colors::BORDER)),
            scrollbar_area,
            &mut state,
        );
    }
}

/// Format a Unix timestamp in milliseconds to "Mon DD HH:MM" format
fn format_timestamp(ts_millis: i64) -> String {
    let secs = ts_millis / 1000;
    // Simple formatting: compute date components from unix timestamp
    // Using chrono-free approach: just show relative time for simplicity
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let elapsed = now - secs;
    if elapsed < 60 {
        "just now        ".to_string()
    } else if elapsed < 3600 {
        format!("{:>3}m ago        ", elapsed / 60)
    } else if elapsed < 86400 {
        format!("{:>3}h ago        ", elapsed / 3600)
    } else if elapsed < 604800 {
        format!("{:>3}d ago        ", elapsed / 86400)
    } else {
        format!("{:>3}w ago        ", elapsed / 604800)
    }
}
