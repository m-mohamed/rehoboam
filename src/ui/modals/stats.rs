//! Stats dashboard modal — Overview, Models, Activity, Quality tabs
//!
//! Renders a tabbed overlay with Claude Code usage statistics from
//! stats-cache.json and facet data.

use crate::app::App;
use crate::config::colors;
use ratatui::{
    prelude::*,
    widgets::{
        Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Tabs,
    },
    Frame,
};

/// Render the stats dashboard overlay
pub fn render_stats_viewer(f: &mut Frame, area: Rect, app: &mut App) {
    f.render_widget(Clear, area);

    let tab_titles = vec!["Overview", "Models", "Activity", "Quality"];
    let active_tab = app.stats_viewer.active_tab;

    // Split into tab bar + content
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    // Tab bar
    let tabs = Tabs::new(tab_titles)
        .select(active_tab)
        .style(Style::default().fg(colors::FG))
        .highlight_style(
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )
        .divider(" | ")
        .block(
            Block::default()
                .title(" Stats Dashboard ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::HIGHLIGHT))
                .border_type(ratatui::widgets::BorderType::Rounded)
                .style(Style::default().bg(colors::BG)),
        );
    f.render_widget(tabs, chunks[0]);

    // Content area
    let content_area = chunks[1];
    match active_tab {
        0 => render_overview(f, content_area, app),
        1 => render_models(f, content_area, app),
        2 => render_activity(f, content_area, app),
        3 => render_quality(f, content_area, app),
        _ => {}
    }
}

fn render_overview(f: &mut Frame, area: Rect, app: &mut App) {
    let stats = match &app.state.stats_cache {
        Some(s) => s,
        None => {
            let msg = Paragraph::new("No stats data found. Run Claude Code to generate stats.")
                .style(Style::default().fg(colors::IDLE))
                .block(content_block(" Overview ", " Tab:switch  j/k:scroll  Esc:close "));
            f.render_widget(msg, area);
            return;
        }
    };

    let mut lines: Vec<Line> = Vec::new();

    // Summary section
    lines.push(Line::from(Span::styled(
        "SUMMARY",
        Style::default()
            .fg(colors::HIGHLIGHT)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(format!(
        "  Sessions:  {}",
        format_number(stats.total_sessions)
    )));
    lines.push(Line::from(format!(
        "  Messages:  {}",
        format_number(stats.total_messages)
    )));
    if !stats.first_session_date.is_empty() {
        lines.push(Line::from(format!(
            "  Since:     {}",
            stats.first_session_date
        )));
    }
    if let Some(ref longest) = stats.longest_session {
        let hours = longest.duration_ms / 3_600_000;
        let mins = (longest.duration_ms % 3_600_000) / 60_000;
        lines.push(Line::from(format!(
            "  Longest:   {}h{}m ({} msgs)",
            hours, mins, longest.message_count
        )));
    }

    lines.push(Line::from(""));

    // Activity sparkline (last 14 days)
    if !stats.daily_activity.is_empty() {
        lines.push(Line::from(Span::styled(
            "ACTIVITY (last 14 days)",
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        let recent: Vec<u64> = stats
            .daily_activity
            .iter()
            .rev()
            .take(14)
            .rev()
            .map(|d| d.messages)
            .collect();
        let max = recent.iter().max().copied().unwrap_or(1).max(1);
        let bars: String = recent
            .iter()
            .map(|&v| {
                let level = (v * 8 / max).min(7) as usize;
                ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'][level]
            })
            .collect();
        lines.push(Line::from(format!("  {bars}  messages/day")));
        lines.push(Line::from(""));
    }

    // Hour of day distribution
    let hour_max = stats.hour_counts.iter().max().copied().unwrap_or(1).max(1);
    if hour_max > 0 {
        lines.push(Line::from(Span::styled(
            "HOUR OF DAY",
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        let bars: String = stats
            .hour_counts
            .iter()
            .map(|&v| {
                let level = (v as u64 * 8 / hour_max as u64).min(7) as usize;
                ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'][level]
            })
            .collect();
        lines.push(Line::from(format!("  {bars}")));
        lines.push(Line::from(
            "  00  04  08  12  16  20",
        ));
    }

    let total_lines = lines.len() as u16;
    let inner_height = area.height.saturating_sub(4);
    let max_scroll = total_lines.saturating_sub(inner_height);
    if app.stats_viewer.scroll_offset > max_scroll {
        app.stats_viewer.scroll_offset = max_scroll;
    }

    let paragraph = Paragraph::new(lines)
        .scroll((app.stats_viewer.scroll_offset, 0))
        .block(content_block(" Overview ", " Tab:switch  j/k:scroll  Esc:close "));
    f.render_widget(paragraph, area);

    render_scrollbar(f, area, max_scroll, app.stats_viewer.scroll_offset);
}

fn render_models(f: &mut Frame, area: Rect, app: &mut App) {
    let stats = match &app.state.stats_cache {
        Some(s) => s,
        None => {
            let msg = Paragraph::new("No model usage data available.")
                .style(Style::default().fg(colors::IDLE))
                .block(content_block(" Models ", " Tab:switch  Esc:close "));
            f.render_widget(msg, area);
            return;
        }
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled(
        "TOKEN USAGE BY MODEL",
        Style::default()
            .fg(colors::HIGHLIGHT)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    let max_cache = stats
        .model_usage
        .iter()
        .map(|m| m.cache_read)
        .max()
        .unwrap_or(1)
        .max(1);

    for model in &stats.model_usage {
        let bar_width = (model.cache_read as f64 / max_cache as f64 * 20.0) as usize;
        let bar: String = "█".repeat(bar_width.max(1));
        let padding = " ".repeat(20usize.saturating_sub(bar_width));

        // Format model name (truncate or pad to 14 chars)
        let name = if model.model.len() > 14 {
            format!("{}…", &model.model[..13])
        } else {
            format!("{:<14}", model.model)
        };

        lines.push(Line::from(vec![
            Span::styled(format!("  {name} "), Style::default().fg(colors::FG)),
            Span::styled(bar, Style::default().fg(colors::WORKING)),
            Span::raw(padding),
            Span::styled(
                format!(
                    " {} cache  {}in  {}out",
                    format_tokens(model.cache_read),
                    format_tokens(model.input),
                    format_tokens(model.output),
                ),
                Style::default().fg(colors::IDLE),
            ),
        ]));
    }

    let total_lines = lines.len() as u16;
    let inner_height = area.height.saturating_sub(4);
    let max_scroll = total_lines.saturating_sub(inner_height);
    if app.stats_viewer.scroll_offset > max_scroll {
        app.stats_viewer.scroll_offset = max_scroll;
    }

    let paragraph = Paragraph::new(lines)
        .scroll((app.stats_viewer.scroll_offset, 0))
        .block(content_block(" Models ", " Tab:switch  j/k:scroll  Esc:close "));
    f.render_widget(paragraph, area);
}

fn render_activity(f: &mut Frame, area: Rect, app: &mut App) {
    let stats = match &app.state.stats_cache {
        Some(s) => s,
        None => {
            let msg = Paragraph::new("No activity data available.")
                .style(Style::default().fg(colors::IDLE))
                .block(content_block(" Activity ", " Tab:switch  Esc:close "));
            f.render_widget(msg, area);
            return;
        }
    };

    let mut lines: Vec<Line> = Vec::new();

    // Table header
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<12} {:>8} {:>10} {:>8}", "DATE", "SESSIONS", "MESSAGES", "TOOLS"),
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(
        format!("  {}", "-".repeat(42)),
    ));

    // List daily activity in reverse (newest first)
    for day in stats.daily_activity.iter().rev() {
        lines.push(Line::from(format!(
            "  {:<12} {:>8} {:>10} {:>8}",
            day.date,
            format_number(day.sessions),
            format_number(day.messages),
            format_number(day.tool_calls),
        )));
    }

    let total_lines = lines.len() as u16;
    let inner_height = area.height.saturating_sub(4);
    let max_scroll = total_lines.saturating_sub(inner_height);
    if app.stats_viewer.scroll_offset > max_scroll {
        app.stats_viewer.scroll_offset = max_scroll;
    }

    let paragraph = Paragraph::new(lines)
        .scroll((app.stats_viewer.scroll_offset, 0))
        .block(content_block(" Activity ", " Tab:switch  j/k:scroll  Esc:close "));
    f.render_widget(paragraph, area);

    render_scrollbar(f, area, max_scroll, app.stats_viewer.scroll_offset);
}

fn render_quality(f: &mut Frame, area: Rect, app: &mut App) {
    let quality = match &app.state.session_quality {
        Some(q) => q,
        None => {
            let msg = Paragraph::new("No session quality data available.")
                .style(Style::default().fg(colors::IDLE))
                .block(content_block(" Quality ", " Tab:switch  Esc:close "));
            f.render_widget(msg, area);
            return;
        }
    };

    let mut lines: Vec<Line> = Vec::new();

    // Outcomes section
    let total = quality.total_sessions;
    let achieved = quality.outcomes[0];
    let pct = if total > 0 {
        achieved as f64 / total as f64 * 100.0
    } else {
        0.0
    };

    lines.push(Line::from(Span::styled(
        format!("OUTCOMES ({total} sessions)"),
        Style::default()
            .fg(colors::HIGHLIGHT)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    // Visual bar for achievement rate
    let filled = (pct / 5.0) as usize; // 20 chars = 100%
    let empty = 20usize.saturating_sub(filled);
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("█".repeat(filled), Style::default().fg(colors::WORKING)),
        Span::styled("░".repeat(empty), Style::default().fg(colors::BORDER)),
        Span::styled(format!(" {pct:.0}%"), Style::default().fg(colors::FG)),
    ]));
    lines.push(Line::from(format!(
        "  fully: {}, mostly: {}, partially: {}, not achieved: {}, other: {}",
        quality.outcomes[0],
        quality.outcomes[1],
        quality.outcomes[2],
        quality.outcomes[3],
        quality.outcomes[4]
    )));
    lines.push(Line::from(""));

    // Helpfulness
    if !quality.helpfulness.is_empty() {
        lines.push(Line::from(Span::styled(
            "HELPFULNESS",
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        for (rating, count) in &quality.helpfulness {
            lines.push(Line::from(format!("  {rating}: {count}")));
        }
        lines.push(Line::from(""));
    }

    // Top categories
    if !quality.top_categories.is_empty() {
        lines.push(Line::from(Span::styled(
            "TOP GOALS",
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        for (cat, count) in &quality.top_categories {
            lines.push(Line::from(format!("  {cat}: {count}")));
        }
        lines.push(Line::from(""));
    }

    // Satisfaction
    if !quality.satisfaction.is_empty() {
        lines.push(Line::from(Span::styled(
            "SATISFACTION",
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        for (rating, count) in &quality.satisfaction {
            lines.push(Line::from(format!("  {rating}: {count}")));
        }
        lines.push(Line::from(""));
    }

    // Friction
    if !quality.friction.is_empty() {
        lines.push(Line::from(Span::styled(
            "TOP FRICTION",
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        for (fric, count) in &quality.friction {
            lines.push(Line::from(format!("  {fric}: {count}")));
        }
        lines.push(Line::from(""));
    }

    // Session types
    if !quality.session_types.is_empty() {
        lines.push(Line::from(Span::styled(
            "SESSION MIX",
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        for (st, count) in &quality.session_types {
            lines.push(Line::from(format!("  {st}: {count}")));
        }
        lines.push(Line::from(""));
    }

    // Success patterns
    if !quality.success_patterns.is_empty() {
        lines.push(Line::from(Span::styled(
            "SUCCESS PATTERNS",
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        for (pat, count) in &quality.success_patterns {
            lines.push(Line::from(format!("  {pat}: {count}")));
        }
    }

    let total_lines = lines.len() as u16;
    let inner_height = area.height.saturating_sub(4);
    let max_scroll = total_lines.saturating_sub(inner_height);
    if app.stats_viewer.scroll_offset > max_scroll {
        app.stats_viewer.scroll_offset = max_scroll;
    }

    let paragraph = Paragraph::new(lines)
        .scroll((app.stats_viewer.scroll_offset, 0))
        .block(content_block(" Quality ", " Tab:switch  j/k:scroll  Esc:close "));
    f.render_widget(paragraph, area);

    render_scrollbar(f, area, max_scroll, app.stats_viewer.scroll_offset);
}

fn content_block<'a>(title: &'a str, footer: &'a str) -> Block<'a> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors::BORDER))
        .border_type(ratatui::widgets::BorderType::Rounded)
        .title_bottom(Line::from(footer).centered())
        .style(Style::default().bg(colors::BG))
}

fn render_scrollbar(f: &mut Frame, area: Rect, max_scroll: u16, scroll_offset: u16) {
    if max_scroll > 0 {
        let scrollbar_area = Rect {
            x: area.x + area.width - 1,
            y: area.y + 1,
            width: 1,
            height: area.height.saturating_sub(2),
        };
        let mut state =
            ScrollbarState::new(max_scroll as usize).position(scroll_offset as usize);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .thumb_style(Style::default().fg(colors::HIGHLIGHT))
                .track_style(Style::default().fg(colors::BORDER)),
            scrollbar_area,
            &mut state,
        );
    }
}

/// Format a number with commas (e.g., 1234567 → "1,234,567")
fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

/// Format token counts with suffixes (e.g., 1700000000 → "1.7B")
fn format_tokens(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
