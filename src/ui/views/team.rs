//! Team view - agents grouped by team with hierarchy

use crate::app::App;
use crate::config::colors;
use crate::state::{AttentionType, Status};
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
                Status::Attention(AttentionType::Permission) => "Permission",
                Status::Attention(AttentionType::Input) => "Input",
                Status::Attention(AttentionType::Notification) => {
                    // Show notification_type if available
                    match agent.last_notification_type.as_deref() {
                        Some("permission_prompt") => "Permission",
                        Some("idle_prompt") => "Idle",
                        Some("auth_success") => "Auth OK",
                        _ => "Notification",
                    }
                }
                Status::Attention(AttentionType::Waiting) => "Waiting",
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

            // Model name (shorten for display)
            let model_tag = agent.model.as_deref().map(|m| {
                // Shorten common model names: "claude-opus-4-6" -> "opus-4.6"
                let short = m
                    .strip_prefix("claude-")
                    .unwrap_or(m)
                    .split('-')
                    .take(3)
                    .collect::<Vec<_>>()
                    .join("-");
                short
            });

            // Context usage indicator
            let ctx_tag = agent
                .context_usage_percent
                .map(|pct| format!("ctx:{:.0}%", pct));

            let is_selected = selected_pane_id == Some(agent.pane_id.as_str());
            let select_prefix = if is_selected { "\u{25b6} " } else { "  " }; // ▶ or spaces

            // Build optional tags string
            let mut tags = String::new();
            if let Some(ref m) = model_tag {
                tags.push_str(m);
            }
            if let Some(ref c) = ctx_tag {
                if !tags.is_empty() {
                    tags.push(' ');
                }
                tags.push_str(c);
            }
            if let Some(ref effort) = agent.effort_level {
                if !tags.is_empty() {
                    tags.push(' ');
                }
                tags.push_str("effort:");
                tags.push_str(effort);
            }
            if agent.compaction_count > 0 {
                if !tags.is_empty() {
                    tags.push(' ');
                }
                tags.push_str(&format!("compact:{}", agent.compaction_count));
            }
            let tags_display = if tags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", tags)
            };

            let line = format!(
                "{}{} {}{} {} ({}){} {} {}",
                select_prefix,
                glyph,
                lead_prefix,
                icon,
                display_name,
                status_str,
                tags_display,
                tool_info,
                elapsed
            );

            // Context burn warning: override color when usage > 80%
            let ctx_warning = agent.context_usage_percent.is_some_and(|pct| pct > 80.0);
            let effective_color = if ctx_warning {
                colors::COMPACTING // Yellow warning for high context burn
            } else {
                color
            };

            let style = if is_selected {
                Style::default()
                    .fg(effective_color)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default().fg(effective_color)
            };

            items.push(ListItem::new(Line::from(vec![Span::styled(line, style)])));

            let continuation = if is_last { "   " } else { "\u{2502}  " }; // │ or space

            // Show error message when last tool failed
            if agent.last_tool_failed {
                let error_msg = if agent.failed_tool_interrupt {
                    format!(
                        "  {}  \u{26a0} {} interrupted",
                        continuation,
                        agent.failed_tool_name.as_deref().unwrap_or("tool")
                    )
                } else if let Some(ref err) = agent.failed_tool_error {
                    // Truncate long error messages
                    let truncated = if err.len() > 60 {
                        format!("{}...", &err[..57])
                    } else {
                        err.clone()
                    };
                    format!(
                        "  {}  \u{274c} {}: {}",
                        continuation,
                        agent.failed_tool_name.as_deref().unwrap_or("tool"),
                        truncated
                    )
                } else {
                    format!(
                        "  {}  \u{274c} {} failed",
                        continuation,
                        agent.failed_tool_name.as_deref().unwrap_or("tool")
                    )
                };
                items.push(ListItem::new(Line::from(vec![Span::styled(
                    error_msg,
                    Style::default().fg(Color::Red),
                )])));
            }

            // Show stop_hook_active indicator (Claude continues after Stop)
            if agent.stop_hook_active
                && matches!(agent.status, Status::Attention(AttentionType::Waiting))
            {
                let hook_line = format!("  {}  \u{1f517} stop hook active", continuation); // 🔗
                items.push(ListItem::new(Line::from(vec![Span::styled(
                    hook_line,
                    Style::default().fg(colors::WORKING),
                )])));
            }

            // Show session metadata line (session_source + permission_mode + cwd + files + subagents)
            {
                let mut meta_parts: Vec<String> = Vec::new();
                if let Some(ref src) = agent.session_source {
                    meta_parts.push(src.clone());
                }
                if let Some(ref mode) = agent.permission_mode {
                    meta_parts.push(format!("mode:{}", mode));
                }
                // Show shortened cwd (~/path instead of /Users/name/path)
                if let Some(ref cwd) = agent.cwd {
                    let short_cwd = shorten_path(cwd);
                    meta_parts.push(short_cwd);
                }
                // Show modified files count
                let file_count = agent.modified_files.len();
                if file_count > 0 {
                    meta_parts.push(format!(
                        "{} file{} modified",
                        file_count,
                        if file_count == 1 { "" } else { "s" }
                    ));
                }
                let running_subagents: Vec<_> = agent
                    .subagents
                    .iter()
                    .filter(|s| s.status == "running")
                    .collect();
                let total_subagents = agent.subagents.len();
                if total_subagents > 0 {
                    // Show running subagent types if available
                    let running_types: Vec<&str> = running_subagents
                        .iter()
                        .filter_map(|s| s.subagent_type.as_deref())
                        .collect();
                    if running_types.is_empty() {
                        meta_parts.push(format!(
                            "{} sub ({} running)",
                            total_subagents,
                            running_subagents.len()
                        ));
                    } else {
                        meta_parts.push(format!(
                            "{} sub ({})",
                            total_subagents,
                            running_types.join(", ")
                        ));
                    }
                }
                if !meta_parts.is_empty() {
                    let meta_line =
                        format!("  {}  {}", continuation, meta_parts.join(" \u{2502} ")); // │ separator
                    items.push(ListItem::new(Line::from(vec![Span::styled(
                        meta_line,
                        Style::default().fg(colors::IDLE),
                    )])));
                }
            }

            // Show running subagent descriptions (truncated)
            {
                let running_subagents: Vec<_> = agent
                    .subagents
                    .iter()
                    .filter(|s| s.status == "running")
                    .collect();
                for sub in running_subagents {
                    if !sub.description.is_empty() && sub.description != "subagent" {
                        let desc = if sub.description.len() > 40 {
                            format!("{}...", &sub.description[..37])
                        } else {
                            sub.description.clone()
                        };
                        let sub_line = format!("  {}  \u{2192} {}", continuation, desc); // → prefix
                        items.push(ListItem::new(Line::from(vec![Span::styled(
                            sub_line,
                            Style::default().fg(colors::IDLE),
                        )])));
                    }
                }
            }

            // Show current_task_subject indented below agent when present
            if let Some(ref task_subject) = agent.current_task_subject {
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

/// Shorten a path for display: replace home dir with ~
fn shorten_path(path: &str) -> String {
    if let Ok(home) = std::env::var("HOME") {
        if let Some(rest) = path.strip_prefix(&home) {
            return format!("~{rest}");
        }
    }
    path.to_string()
}
