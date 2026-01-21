//! Diff viewer modal

use crate::app::App;
use crate::config::colors;
use crate::diff::LineKind;
use ratatui::{
    prelude::*,
    style::Modifier,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::super::helpers::centered_rect;

pub fn render_diff_modal(f: &mut Frame, app: &App) {
    let area = centered_rect(85, 85, f.area());

    // Get project name for title
    let title = if let Some(agent) = app.state.selected_agent() {
        format!(" Git Diff: {} ", agent.project)
    } else {
        " Git Diff ".to_string()
    };

    // Build enhanced diff content with parsed data
    let lines: Vec<Line> = if let Some(ref parsed) = app.parsed_diff {
        if parsed.is_empty() {
            vec![
                Line::from(""),
                Line::styled(
                    "  No uncommitted changes",
                    Style::default().fg(colors::FG).add_modifier(Modifier::DIM),
                ),
                Line::from(""),
                Line::styled(
                    "  Working directory is clean.",
                    Style::default()
                        .fg(colors::IDLE)
                        .add_modifier(Modifier::DIM),
                ),
            ]
        } else {
            build_enhanced_diff_lines(parsed, app)
        }
    } else {
        // Fallback to raw diff content
        app.diff_content
            .lines()
            .map(|line| {
                let style = if line.starts_with('+') && !line.starts_with("+++") {
                    Style::default().fg(Color::Green)
                } else if line.starts_with('-') && !line.starts_with("---") {
                    Style::default().fg(Color::Red)
                } else if line.starts_with("@@") {
                    Style::default().fg(Color::Cyan)
                } else if line.starts_with("diff ") || line.starts_with("index ") {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::DIM)
                } else {
                    Style::default().fg(colors::FG)
                };
                Line::styled(line, style)
            })
            .collect()
    };

    // Apply scroll offset
    let scroll_offset = app.diff_scroll as usize;
    let visible_lines: Vec<Line> = lines.into_iter().skip(scroll_offset).collect();

    let content = Paragraph::new(visible_lines);

    // Build summary string
    let summary = if let Some(ref parsed) = app.parsed_diff {
        if !parsed.is_empty() {
            format!(
                " {} │ [j/k] scroll  [n/p] file  [\\[/\\]] hunk  [o] toggle  [g] commit  [q] close ",
                parsed.summary_string()
            )
        } else {
            " [D/q] Close ".to_string()
        }
    } else {
        " [D/q] Close  [g] Commit  [P] Push ".to_string()
    };

    let diff_widget = content.block(
        Block::default()
            .title(title)
            .title_bottom(summary)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::HIGHLIGHT))
            .border_type(ratatui::widgets::BorderType::Double)
            .style(Style::default().bg(colors::BG)),
    );

    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(diff_widget, area);
}

/// Build enhanced diff lines with file sections and line numbers
fn build_enhanced_diff_lines(parsed: &crate::diff::ParsedDiff, app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // Summary header
    lines.push(Line::styled(
        format!(
            "  {} files changed, +{} -{}",
            parsed.summary.files_changed, parsed.summary.insertions, parsed.summary.deletions
        ),
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::from(""));

    // Render each file
    for (file_idx, file) in parsed.files.iter().enumerate() {
        let is_selected = file_idx == app.diff_selected_file;

        // File header with collapse indicator
        let file_collapsed = file
            .hunks
            .iter()
            .enumerate()
            .all(|(hunk_idx, _)| app.diff_collapsed_hunks.contains(&(file_idx, hunk_idx)));

        let collapse_indicator = if file_collapsed { "▶" } else { "▼" };
        let selection_marker = if is_selected { "►" } else { " " };

        let file_header = format!(
            "{} {} {} (+{} -{})",
            selection_marker, collapse_indicator, file.path, file.insertions, file.deletions
        );

        let header_style = if is_selected {
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        };

        lines.push(Line::styled(file_header, header_style));

        // Skip hunks if collapsed
        if file_collapsed {
            lines.push(Line::styled(
                "    [collapsed]",
                Style::default().fg(colors::FG).add_modifier(Modifier::DIM),
            ));
            lines.push(Line::from(""));
            continue;
        }

        // Render hunks
        for (hunk_idx, hunk) in file.hunks.iter().enumerate() {
            let hunk_collapsed = app.diff_collapsed_hunks.contains(&(file_idx, hunk_idx));
            let is_selected_hunk = is_selected && hunk_idx == app.diff_selected_hunk;

            // Hunk header with size indicator and selection marker
            let hunk_marker = if is_selected_hunk { "►" } else { " " };
            let size_indicator = hunk.size_indicator().unwrap_or_default();
            let header_text = if !size_indicator.is_empty() {
                format!("  {}  {} {}", hunk_marker, hunk.header, size_indicator)
            } else {
                format!("  {}  {}", hunk_marker, hunk.header)
            };

            let hunk_style = if is_selected_hunk {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };

            lines.push(Line::styled(header_text, hunk_style));

            if hunk_collapsed {
                lines.push(Line::styled(
                    "      [collapsed]",
                    Style::default().fg(colors::FG).add_modifier(Modifier::DIM),
                ));
                continue;
            }

            // Render hunk lines with line numbers
            for diff_line in &hunk.lines {
                let (prefix, style) = match diff_line.kind {
                    LineKind::Context => (" ", Style::default().fg(colors::FG)),
                    LineKind::Addition => ("+", Style::default().fg(Color::Green)),
                    LineKind::Deletion => ("-", Style::default().fg(Color::Red)),
                };

                // Format line numbers
                let old_no = diff_line
                    .old_line_no
                    .map(|n| format!("{:4}", n))
                    .unwrap_or_else(|| "    ".to_string());
                let new_no = diff_line
                    .new_line_no
                    .map(|n| format!("{:4}", n))
                    .unwrap_or_else(|| "    ".to_string());

                let line_text =
                    format!("   {} │{} │{}{}", old_no, new_no, prefix, diff_line.content);

                lines.push(Line::styled(line_text, style));
            }
        }

        lines.push(Line::from(""));
    }

    lines
}
