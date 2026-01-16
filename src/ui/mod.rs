//! UI rendering module for Rehoboam TUI
//!
//! Provides a Kanban-style column layout where agents are grouped by status:
//! - Attention (needs user input)
//! - Working (actively processing)
//! - Compacting (context compaction)
//! - Idle (waiting)

mod card;
mod column;

use crate::app::{App, InputMode, SpawnState, ViewMode};
use crate::config::colors;
use crate::diff::LineKind;
use crate::state::{Status, NUM_COLUMNS};
use column::render_status_column;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    prelude::*,
    style::{Modifier, Style},
    symbols,
    widgets::{
        Block, Borders, List, ListItem, Paragraph, RenderDirection, Sparkline, SparklineBar,
    },
    Frame,
};

/// Main render function
pub fn render(f: &mut Frame, app: &App) {
    // Create layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(12),   // Agent columns (Kanban)
            Constraint::Length(8), // Activity sparklines
            Constraint::Length(1), // Footer
        ])
        .split(f.area());

    render_header(f, chunks[0], app);

    // Render based on view mode
    match app.view_mode {
        ViewMode::Kanban => render_agent_columns(f, chunks[1], app),
        ViewMode::Project => render_project_view(f, chunks[1], app),
        ViewMode::Split => render_split_view(f, chunks[1], app),
    }

    render_activity(f, chunks[2], app);
    render_footer(f, chunks[3], app);

    // Render event log if in debug mode
    if app.debug_mode && !app.state.events.is_empty() {
        render_event_log(f, app);
    }

    // Render help popup if active
    if app.show_help {
        render_help(f);
    }

    // Render dashboard overlay if active
    if app.show_dashboard {
        render_dashboard(f, app);
    }

    // Render diff modal if active
    if app.show_diff {
        render_diff_modal(f, app);
    }

    // Render checkpoint timeline if active
    if app.show_checkpoint_timeline {
        render_checkpoint_timeline(f, app);
    }

    // Render input dialog if in input mode
    if app.input_mode == InputMode::Input {
        render_input_dialog(f, app);
    }

    // Render spawn dialog if in spawn mode
    if app.input_mode == InputMode::Spawn {
        render_spawn_dialog(f, &app.spawn_state);
    }
}

fn render_header(f: &mut Frame, area: Rect, app: &App) {
    // Use cached status counts (O(1) instead of O(3n))
    let [attention, working, compacting] = app.state.status_counts;
    let total = app.state.agents.len();
    let sprite_count = app.state.sprite_agent_count();

    // Build status summary
    let status_parts: Vec<String> = [
        (attention, "attention"),
        (working, "working"),
        (compacting, "compacting"),
    ]
    .iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, label)| format!("{count} {label}"))
    .collect();

    let frozen_indicator = if app.frozen { " [FROZEN]" } else { "" };
    let view_indicator = match app.view_mode {
        ViewMode::Kanban => "",
        ViewMode::Project => " [PROJECT VIEW]",
        ViewMode::Split => " [SPLIT VIEW]",
    };
    // Show sprite count with connection status if any remote agents
    let connected_count = app.state.connected_sprite_count();
    let sprite_indicator = if sprite_count > 0 {
        if connected_count > 0 {
            // Show connected/total (e.g., "☁ 2/3 sprites")
            format!(
                " [☁ {}/{} sprite{}]",
                connected_count,
                sprite_count,
                if sprite_count == 1 { "" } else { "s" }
            )
        } else {
            // No sprites connected yet
            format!(
                " [☁ {} sprite{} (offline)]",
                sprite_count,
                if sprite_count == 1 { "" } else { "s" }
            )
        }
    } else {
        String::new()
    };
    let title = if total == 0 {
        format!("Rehoboam{frozen_indicator}{view_indicator}")
    } else {
        format!(
            "Rehoboam ({} agents: {}){}{}{}",
            total,
            status_parts.join(", "),
            sprite_indicator,
            frozen_indicator,
            view_indicator
        )
    };

    let header = Paragraph::new(title)
        .style(Style::default().fg(colors::FG).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER))
                .border_type(ratatui::widgets::BorderType::Rounded),
        );
    f.render_widget(header, area);
}

/// Render agents in Kanban-style columns by status
fn render_agent_columns(f: &mut Frame, area: Rect, app: &App) {
    let columns = app.state.agents_by_column();

    // 3 equal-width columns (Attention, Working, Compacting)
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, NUM_COLUMNS as u32),
            Constraint::Ratio(1, NUM_COLUMNS as u32),
            Constraint::Ratio(1, NUM_COLUMNS as u32),
        ])
        .split(area);

    for (i, agents) in columns.iter().enumerate() {
        let selected_card = if app.state.selected_column == i {
            Some(app.state.selected_card)
        } else {
            None
        };
        let column_active = app.state.selected_column == i;
        render_status_column(
            f,
            chunks[i],
            i,
            agents,
            selected_card,
            column_active,
            &app.state.selected_agents,
        );
    }
}

/// Render agents grouped by project
fn render_project_view(f: &mut Frame, area: Rect, app: &App) {
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

/// Render split view: agent list on left, live output on right
fn render_split_view(f: &mut Frame, area: Rect, app: &App) {
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
fn render_agent_list_compact(f: &mut Frame, area: Rect, app: &App) {
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

            // Show loop iteration if in Rehoboam mode
            let loop_info = if agent.loop_mode != crate::state::LoopMode::None {
                format!(" [{}]", agent.loop_iteration)
            } else {
                String::new()
            };

            let line = format!(
                "{} {}{} {}{}",
                selected,
                sprite_prefix,
                icon,
                truncate(&agent.project, 15),
                loop_info
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
fn render_live_output(f: &mut Frame, area: Rect, app: &App) {
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

/// Render subagent tree panel
fn render_subagent_tree(f: &mut Frame, area: Rect, app: &App) {
    let agent = app.state.selected_agent();

    let content = if let Some(agent) = agent {
        if agent.subagents.is_empty() {
            vec![
                Line::from("No subagents spawned."),
                Line::from(""),
                Line::from(Span::styled(
                    "Subagents appear when Claude",
                    Style::default().fg(colors::IDLE),
                )),
                Line::from(Span::styled(
                    "uses the Task tool.",
                    Style::default().fg(colors::IDLE),
                )),
            ]
        } else {
            let mut lines = vec![Line::from(Span::styled(
                format!("Subagents: {}", agent.subagents.len()),
                Style::default()
                    .fg(colors::HIGHLIGHT)
                    .add_modifier(Modifier::BOLD),
            ))];

            for subagent in &agent.subagents {
                let duration = subagent
                    .duration_ms
                    .map(|d| format!("{}ms", d))
                    .unwrap_or_else(|| "running...".to_string());

                // Status indicator with color
                let (status_icon, status_color) = match subagent.status.as_str() {
                    "running" => ("⚡", colors::WORKING),
                    "completed" => ("✓", Color::Green),
                    "failed" => ("✗", Color::Red),
                    _ => ("?", colors::IDLE),
                };

                lines.push(Line::from(vec![
                    Span::styled("├─ ", Style::default().fg(colors::BORDER)),
                    Span::styled(
                        format!("{} {}", status_icon, truncate(&subagent.id, 8)),
                        Style::default().fg(status_color),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("│  ", Style::default().fg(colors::BORDER)),
                    Span::styled(
                        truncate(&subagent.description, area.width.saturating_sub(6) as usize),
                        Style::default().fg(colors::IDLE),
                    ),
                ]));
                lines.push(Line::from(vec![
                    Span::styled("│  ", Style::default().fg(colors::BORDER)),
                    Span::styled(duration, Style::default().fg(colors::COMPACTING)),
                ]));
            }

            lines
        }
    } else {
        vec![Line::from("Select an agent to see subagents.")]
    };

    let tree = Paragraph::new(content).block(
        Block::default()
            .title(" Subagents [T] ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER)),
    );

    f.render_widget(tree, area);
}

fn render_activity(f: &mut Frame, area: Rect, app: &App) {
    // Flatten already-sorted columns instead of O(n log n) sorted_agents()
    let columns = app.state.agents_by_column();
    let agents: Vec<&_> = columns.iter().flatten().copied().collect();

    // Create horizontal layout for sparklines
    let num_agents = agents.len().max(1);
    let constraints: Vec<Constraint> = (0..num_agents)
        .map(|_| Constraint::Ratio(1, num_agents as u32))
        .collect();

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    // Get current time for pulse animation
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    for (i, agent) in agents.iter().enumerate() {
        if i >= chunks.len() {
            break;
        }

        let base_color = status_base_color(&agent.status);
        let is_working = matches!(agent.status, Status::Working);

        // Create SparklineBars with pulse effect for working status
        let data: Vec<SparklineBar> = agent
            .activity
            .iter()
            .enumerate()
            .map(|(idx, v)| {
                let value = (v * 8.0) as u64;
                let bar = SparklineBar::from(value);

                // Pulse effect: recent bars glow brighter when working
                if is_working && idx >= agent.activity.len().saturating_sub(5) {
                    // Pulse based on time - creates a breathing effect
                    let pulse = ((now / 100) % 10) as usize;
                    let intensity = if (idx + pulse).is_multiple_of(3) {
                        colors::WORKING_BRIGHT // Brighter pulse
                    } else {
                        base_color
                    };
                    bar.style(Style::default().fg(intensity))
                } else {
                    bar.style(Style::default().fg(base_color))
                }
            })
            .collect();

        let sparkline = Sparkline::default()
            .data(data)
            .direction(RenderDirection::LeftToRight)
            .bar_set(symbols::bar::NINE_LEVELS)
            .absent_value_style(Style::default().fg(colors::BG_LIGHT))
            .absent_value_symbol(symbols::bar::NINE_LEVELS.empty)
            .block(
                Block::default()
                    .title(format!(" {} ", truncate(&agent.project, 12)))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(colors::BORDER))
                    .border_type(ratatui::widgets::BorderType::Rounded),
            );

        f.render_widget(sparkline, chunks[i]);
    }

    // Show placeholder if no agents
    if agents.is_empty() {
        let placeholder = Block::default()
            .title(" Activity ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER))
            .border_type(ratatui::widgets::BorderType::Rounded);
        f.render_widget(placeholder, area);
    }
}

fn render_footer(f: &mut Frame, area: Rect, app: &App) {
    use crate::app::InputMode;

    // Check if we have a status message to display
    if let Some((ref msg, timestamp)) = app.status_message {
        // Only show if less than 5 seconds old
        if timestamp.elapsed() < std::time::Duration::from_secs(5) {
            let style = if msg.starts_with("Error") || msg.starts_with("⚠") {
                Style::default().fg(Color::Red)
            } else if msg.starts_with("✓") {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Yellow)
            };
            let status = Paragraph::new(msg.as_str())
                .style(style)
                .alignment(Alignment::Center);
            f.render_widget(status, area);
            return;
        }
    }

    // Search mode: show search input
    if app.input_mode == InputMode::Search {
        let search_text = format!("🔍 Search: {}█", app.search_query);
        let footer = Paragraph::new(search_text)
            .style(Style::default().fg(Color::Yellow))
            .alignment(Alignment::Center);
        f.render_widget(footer, area);
        return;
    }

    let selection_count = app.state.selected_agents.len();

    // Context-aware help based on selection state
    let help = if selection_count > 0 {
        // Multi-select mode
        format!("[{selection_count} selected]  Y/N:bulk approve  K:kill all  x:clear  Space:toggle")
    } else if let Some(agent) = app.state.selected_agent() {
        // Single agent selected - show relevant commands
        use crate::state::LoopMode;
        let loop_info = if agent.loop_mode != LoopMode::None {
            format!(" iter:{}/{}", agent.loop_iteration, agent.loop_max)
        } else {
            String::new()
        };

        let mode_indicators: Vec<&str> = [
            app.debug_mode.then_some("[debug]"),
            app.frozen.then_some("[frozen]"),
            app.auto_accept.then_some("[AUTO]"),
        ]
        .into_iter()
        .flatten()
        .collect();

        let prefix = if mode_indicators.is_empty() {
            String::new()
        } else {
            format!("{} ", mode_indicators.join(" "))
        };

        format!(
            "{prefix}Enter:jump  y/n:approve  c:input  X:cancel  R:restart{loop_info}  d:dash  ?:help"
        )
    } else {
        // No selection - show general commands
        let mode_indicators: Vec<&str> = [
            app.debug_mode.then_some("[debug]"),
            app.frozen.then_some("[frozen]"),
            app.auto_accept.then_some("[AUTO]"),
        ]
        .into_iter()
        .flatten()
        .collect();

        let prefix = if mode_indicators.is_empty() {
            String::new()
        } else {
            format!("{} ", mode_indicators.join(" "))
        };

        // View-aware hints
        match app.view_mode {
            crate::app::ViewMode::Kanban => {
                format!("{prefix}s:spawn  d:dashboard  v:view  /:search  ?:help  q:quit")
            }
            crate::app::ViewMode::Project => {
                format!("{prefix}j/k:nav  v:view  /:search  ?:help  q:quit")
            }
            crate::app::ViewMode::Split => {
                format!("{prefix}PgUp/Dn:scroll  T:subagents  v:view  ?:help  q:quit")
            }
        }
    };

    let footer = Paragraph::new(help)
        .style(Style::default().fg(colors::IDLE))
        .alignment(Alignment::Center);

    f.render_widget(footer, area);
}

fn render_event_log(f: &mut Frame, app: &App) {
    let area = centered_rect(60, 50, f.area());

    let items: Vec<ListItem> = app
        .state
        .events
        .iter()
        .take(15)
        .map(|event| {
            let line = format!(
                "{} │ {:12} │ {:15} │ {}",
                format_timestamp(event.timestamp),
                event.event,
                truncate(&event.project, 15),
                event.status
            );
            ListItem::new(line).style(Style::default().fg(colors::FG))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" Event Log ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER))
            .border_type(ratatui::widgets::BorderType::Rounded)
            .style(Style::default().bg(colors::BG)),
    );

    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(list, area);
}

fn render_help(f: &mut Frame) {
    let area = centered_rect(55, 75, f.area());

    let help_text = r"
  Navigation
  h/l         Move between columns
  j/k         Move between cards
  Enter       Jump to agent's terminal
  /           Search agents by name

  Agent Actions
  y/n         Approve/reject permission
  c           Send custom input
  s           Spawn new agent

  Views
  v           Cycle: Kanban → Project → Split
  T           Toggle subagent panel (split view)
  PgUp/PgDn   Scroll output (split view)
  d           Dashboard overview
  f           Freeze display
  ?, H        This help

  Git
  D           Open diff viewer
  g           Git commit
  p           Git push

  Loop Mode
  X           Cancel loop
  R           Restart loop

  Bulk Operations
  Space       Toggle selection
  Y/N         Bulk approve/reject
  K           Kill selected agents
  x           Clear selection

  Sprites
  t           Checkpoint timeline

  Diff Viewer (when open)
  j/k         Scroll
  n/p         Next/prev file
  o/O         Collapse hunk/all
  g           Commit
  G           Push
  q/Esc       Close

  A           Auto-accept (use caution)
  q, Esc      Quit (Esc closes modals first)
";

    let help = Paragraph::new(help_text)
        .style(Style::default().fg(colors::FG))
        .block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::HIGHLIGHT))
                .border_type(ratatui::widgets::BorderType::Double)
                .title_bottom(Line::from(" ?:close ").centered())
                .style(Style::default().bg(colors::BG)),
        );

    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(help, area);
}

fn render_dashboard(f: &mut Frame, app: &App) {
    use crate::state::LoopMode;
    use std::collections::HashMap;

    let area = centered_rect(70, 70, f.area());

    // Calculate session duration
    let session_secs = app.session_start.elapsed().as_secs();
    let session_hours = session_secs / 3600;
    let session_mins = (session_secs % 3600) / 60;
    let session_str = if session_hours > 0 {
        format!("{}h {}m", session_hours, session_mins)
    } else {
        format!("{}m", session_mins)
    };

    // Get counts
    let [attention, working, compacting] = app.state.status_counts;
    let total = app.state.agents.len();
    let sprite_count = app.state.sprite_agent_count();
    let local_count = total - sprite_count;

    // Calculate loop stats
    let mut total_iterations: u32 = 0;
    let mut active_loops: u32 = 0;
    let mut completed_loops: u32 = 0;
    let mut project_counts: HashMap<String, (u32, u32)> = HashMap::new(); // (agents, iters)

    for agent in app.state.agents.values() {
        total_iterations += agent.loop_iteration;

        match agent.loop_mode {
            LoopMode::Active => active_loops += 1,
            LoopMode::Complete => completed_loops += 1,
            _ => {}
        }

        // Group by project
        let entry = project_counts
            .entry(agent.project.clone())
            .or_insert((0, 0));
        entry.0 += 1;
        entry.1 += agent.loop_iteration;
    }

    // Build dashboard text
    let mut lines = vec![
        String::new(),
        format!(
            "  SESSION: {}            AGENTS: {} ({} local, {} sprites)",
            session_str, total, local_count, sprite_count
        ),
        String::new(),
        format!("  ┌─ Status ──────────────┐  ┌─ Loop Progress ─────────────┐"),
        format!(
            "  │ 🔔 Attention:   {:>3}   │  │ Total iterations:   {:>5}   │",
            attention, total_iterations
        ),
        format!(
            "  │ 🤖 Working:     {:>3}   │  │ Completed loops:    {:>5}   │",
            working, completed_loops
        ),
        format!(
            "  │ 🔄 Compacting:  {:>3}   │  │ Active loops:       {:>5}   │",
            compacting, active_loops
        ),
        "  └──────────────────────┘  └─────────────────────────────┘".to_string(),
        String::new(),
    ];

    // Add project breakdown
    if !project_counts.is_empty() {
        lines.push("  ┌─ By Project ───────────────────────────────────────┐".to_string());
        let mut projects: Vec<_> = project_counts.iter().collect();
        projects.sort_by(|a, b| b.1 .1.cmp(&a.1 .1)); // Sort by iterations desc

        for (project, (agents, iters)) in projects.iter().take(5) {
            let bar_len = (*iters as usize).min(20);
            let bar = "█".repeat(bar_len);
            let project_short = if project.len() > 15 {
                format!("{}...", &project[..12])
            } else {
                project.to_string()
            };
            lines.push(format!(
                "  │ {:15} {:20} {:>2} agents {:>4} iters │",
                project_short, bar, agents, iters
            ));
        }
        lines.push("  └─────────────────────────────────────────────────────┘".to_string());
    }

    let dashboard_text = lines.join("\n");

    let dashboard = Paragraph::new(dashboard_text)
        .style(Style::default().fg(colors::FG))
        .block(
            Block::default()
                .title(" Rehoboam Dashboard ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::HIGHLIGHT))
                .border_type(ratatui::widgets::BorderType::Double)
                .title_bottom(Line::from(" d:close ").centered())
                .style(Style::default().bg(colors::BG)),
        );

    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(dashboard, area);
}

fn render_diff_modal(f: &mut Frame, app: &App) {
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
            vec![Line::styled(
                "No uncommitted changes",
                Style::default().fg(colors::FG).add_modifier(Modifier::DIM),
            )]
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
                " {} │ [j/k] scroll  [n/p] file  [o] toggle  [g] commit  [q] close ",
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

            // Hunk header
            lines.push(Line::styled(
                format!("   {}", hunk.header),
                Style::default().fg(Color::Cyan),
            ));

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

fn render_checkpoint_timeline(f: &mut Frame, app: &App) {
    let area = centered_rect(70, 60, f.area());

    // Get project name for title
    let title = if let Some(agent) = app.state.selected_agent() {
        format!(" Checkpoint Timeline: {} ", agent.project)
    } else {
        " Checkpoint Timeline ".to_string()
    };

    let content = if app.checkpoint_timeline.is_empty() {
        // Empty state
        let empty_msg = vec![
            Line::from(""),
            Line::styled(
                "No checkpoints yet",
                Style::default().fg(colors::FG).add_modifier(Modifier::DIM),
            ),
            Line::from(""),
            Line::styled(
                "Checkpoints are created automatically during sprite execution",
                Style::default().fg(colors::FG).add_modifier(Modifier::DIM),
            ),
            Line::styled(
                "or manually via the Sprites API.",
                Style::default().fg(colors::FG).add_modifier(Modifier::DIM),
            ),
        ];
        Paragraph::new(empty_msg).alignment(Alignment::Center)
    } else {
        // Build timeline list
        let items: Vec<Line> = app
            .checkpoint_timeline
            .iter()
            .enumerate()
            .map(|(i, cp)| {
                let prefix = if i == app.selected_checkpoint {
                    "▶ "
                } else {
                    "  "
                };
                let style = if i == app.selected_checkpoint {
                    Style::default()
                        .fg(colors::HIGHLIGHT)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(colors::FG)
                };

                // Format elapsed time from Unix timestamp
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let elapsed_secs = (now - cp.created_at).max(0) as u64;
                let elapsed = std::time::Duration::from_secs(elapsed_secs);
                let elapsed_str = format_checkpoint_elapsed(elapsed);

                // Show iteration if in loop mode
                let iter_str = if cp.iteration > 0 {
                    format!(" [iter {}]", cp.iteration)
                } else {
                    String::new()
                };

                Line::styled(
                    format!(
                        "{}{} │ {} ago{} │ {}",
                        prefix,
                        &cp.id[..cp.id.len().min(8)],
                        elapsed_str,
                        iter_str,
                        cp.comment
                    ),
                    style,
                )
            })
            .collect();

        Paragraph::new(items)
    };

    let timeline_widget = content.block(
        Block::default()
            .title(title)
            .title_bottom(" [↑/k] Up  [↓/j] Down  [Enter] Restore  [t/Esc] Close ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::HIGHLIGHT))
            .border_type(ratatui::widgets::BorderType::Double)
            .style(Style::default().bg(colors::BG)),
    );

    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(timeline_widget, area);
}

/// Format elapsed time for checkpoint display
fn format_checkpoint_elapsed(elapsed: std::time::Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}

fn render_input_dialog(f: &mut Frame, app: &App) {
    let area = centered_rect(60, 20, f.area());

    // Get selected agent info for the title
    let title = if let Some(agent) = app.state.selected_agent() {
        format!(" Send to: {} ({}) ", agent.project, agent.pane_id)
    } else {
        " Send Input ".to_string()
    };

    // Build input display with cursor
    let input_display = format!("{}▏", app.input_buffer);

    let input_widget = Paragraph::new(input_display)
        .style(Style::default().fg(colors::FG))
        .block(
            Block::default()
                .title(title)
                .title_bottom(" [Enter] Send  [Esc] Cancel ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::HIGHLIGHT))
                .border_type(ratatui::widgets::BorderType::Double)
                .style(Style::default().bg(colors::BG)),
        );

    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(input_widget, area);
}

fn render_spawn_dialog(f: &mut Frame, spawn_state: &SpawnState) {
    use crate::app::spawn::is_github_url;

    let area = centered_rect(70, 80, f.area());

    // Check if we need to show clone destination field
    let show_clone_dest = !spawn_state.use_sprite && is_github_url(&spawn_state.project_path);

    // Split into fields: project, prompt, branch, worktree toggle, loop toggle, loop options (inc. role), sprite toggle, network policy, resources, clone_dest (conditional), error, instructions
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),                                   // Project path (0)
            Constraint::Length(3),                                   // Prompt (1)
            Constraint::Length(3),                                   // Branch name (2)
            Constraint::Length(3),                                   // Worktree toggle (3)
            Constraint::Length(3),                                   // Loop mode toggle (4)
            Constraint::Length(3), // Loop options (5=max_iter, 6=stop_word, 7=role)
            Constraint::Length(3), // Sprite toggle (8)
            Constraint::Length(3), // Network policy (9)
            Constraint::Length(3), // Resources: RAM (10), CPUs (11)
            Constraint::Length(if show_clone_dest { 3 } else { 0 }), // Clone destination (12) - conditional
            Constraint::Length(2),                                   // Error message
            Constraint::Length(2),                                   // Instructions
        ])
        .margin(1)
        .split(area);

    // Helper for field styling - bold when active
    let field_style = |active: bool| {
        if active {
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(colors::FG)
        }
    };
    let border_style = |active: bool| {
        if active {
            Style::default()
                .fg(colors::HIGHLIGHT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(colors::BORDER)
        }
    };

    // Project path / GitHub repo field (0)
    // When sprite mode is on, this becomes a GitHub repo field
    let project_cursor = if spawn_state.active_field == 0 {
        "▏"
    } else {
        ""
    };
    let (field_value, field_title, placeholder) = if spawn_state.use_sprite {
        (
            &spawn_state.github_repo,
            " GitHub Repo (owner/repo) ",
            "e.g. owner/repo or https://github.com/owner/repo",
        )
    } else {
        (
            &spawn_state.project_path,
            " Local Directory ",
            "e.g. ~/projects/my-app or /path/to/project",
        )
    };
    let display_text = if field_value.is_empty() && spawn_state.active_field != 0 {
        placeholder.to_string()
    } else {
        format!("{}{}", field_value, project_cursor)
    };
    let text_style = if field_value.is_empty() && spawn_state.active_field != 0 {
        Style::default().fg(Color::DarkGray)
    } else {
        field_style(spawn_state.active_field == 0)
    };
    let project_widget = Paragraph::new(display_text).style(text_style).block(
        Block::default()
            .title(field_title)
            .borders(Borders::ALL)
            .border_style(border_style(spawn_state.active_field == 0)),
    );

    // Prompt field (1)
    let prompt_cursor = if spawn_state.active_field == 1 {
        "▏"
    } else {
        ""
    };
    let prompt_placeholder = "e.g. Build a REST API with authentication...";
    let prompt_display = if spawn_state.prompt.is_empty() && spawn_state.active_field != 1 {
        prompt_placeholder.to_string()
    } else {
        format!("{}{}", spawn_state.prompt, prompt_cursor)
    };
    let prompt_style = if spawn_state.prompt.is_empty() && spawn_state.active_field != 1 {
        Style::default().fg(Color::DarkGray)
    } else {
        field_style(spawn_state.active_field == 1)
    };
    let prompt_widget = Paragraph::new(prompt_display).style(prompt_style).block(
        Block::default()
            .title(" Prompt (optional) ")
            .borders(Borders::ALL)
            .border_style(border_style(spawn_state.active_field == 1)),
    );

    // Branch name field (2)
    let branch_cursor = if spawn_state.active_field == 2 {
        "▏"
    } else {
        ""
    };
    let branch_widget = Paragraph::new(format!("{}{}", spawn_state.branch_name, branch_cursor))
        .style(field_style(spawn_state.active_field == 2))
        .block(
            Block::default()
                .title(" Branch Name (for worktree) ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 2)),
        );

    // Worktree toggle (3)
    let checkbox = if spawn_state.use_worktree {
        "[x]"
    } else {
        "[ ]"
    };
    let worktree_text = format!("{checkbox} Create isolated git worktree");
    let worktree_widget = Paragraph::new(worktree_text)
        .style(field_style(spawn_state.active_field == 3))
        .block(
            Block::default()
                .title(" Git Isolation ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 3)),
        );

    // Loop mode toggle (4)
    let loop_checkbox = if spawn_state.loop_enabled {
        "[x]"
    } else {
        "[ ]"
    };
    let loop_text = format!("{loop_checkbox} Enable Loop Mode (Rehoboam-style autonomy)");
    let loop_widget = Paragraph::new(loop_text)
        .style(field_style(spawn_state.active_field == 4))
        .block(
            Block::default()
                .title(" Loop Mode ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 4)),
        );

    // Loop options (5 = max iterations, 6 = stop word, 7 = role) - show side by side
    let loop_options_area = chunks[5];
    let loop_options_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25), // Max iterations
            Constraint::Percentage(30), // Stop word
            Constraint::Percentage(45), // Role selector
        ])
        .split(loop_options_area);

    // Max iterations field (5)
    let iter_cursor = if spawn_state.active_field == 5 {
        "▏"
    } else {
        ""
    };
    let iter_widget = Paragraph::new(format!(
        "{}{}",
        spawn_state.loop_max_iterations, iter_cursor
    ))
    .style(field_style(spawn_state.active_field == 5))
    .block(
        Block::default()
            .title(" Max Iter ")
            .borders(Borders::ALL)
            .border_style(border_style(spawn_state.active_field == 5)),
    );

    // Stop word field (6)
    let stop_cursor = if spawn_state.active_field == 6 {
        "▏"
    } else {
        ""
    };
    let stop_widget = Paragraph::new(format!("{}{}", spawn_state.loop_stop_word, stop_cursor))
        .style(field_style(spawn_state.active_field == 6))
        .block(
            Block::default()
                .title(" Stop Word ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 6)),
        );

    // Role selector (7) - Cursor-aligned behavioral patterns
    let role_display = if spawn_state.loop_enabled {
        spawn_state.loop_role.display()
    } else {
        "(enable Loop mode)"
    };
    let role_widget = Paragraph::new(format!("<  {}  >", role_display))
        .style(if spawn_state.loop_enabled {
            field_style(spawn_state.active_field == 7)
        } else {
            Style::default().fg(colors::FG).add_modifier(Modifier::DIM)
        })
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .title(" Role (←/→) ")
                .borders(Borders::ALL)
                .border_style(if spawn_state.loop_enabled {
                    border_style(spawn_state.active_field == 7)
                } else {
                    Style::default()
                        .fg(colors::BORDER)
                        .add_modifier(Modifier::DIM)
                }),
        );

    // Sprite toggle (8)
    let sprite_checkbox = if spawn_state.use_sprite { "[x]" } else { "[ ]" };
    let sprite_text = format!("{sprite_checkbox} Run on remote Sprite (cloud VM)");
    let sprite_widget = Paragraph::new(sprite_text)
        .style(field_style(spawn_state.active_field == 8))
        .block(
            Block::default()
                .title(" Sprite Mode ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 8)),
        );

    // Network policy selector (9) - only visible when sprite mode is enabled
    let network_display = if spawn_state.use_sprite {
        spawn_state.network_preset.display()
    } else {
        "(enable Sprite mode to configure)"
    };
    let network_widget = Paragraph::new(format!("<  {network_display}  >"))
        .style(if spawn_state.use_sprite {
            field_style(spawn_state.active_field == 9)
        } else {
            Style::default().fg(colors::FG).add_modifier(Modifier::DIM)
        })
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .title(" Network Policy (←/→ to change) ")
                .borders(Borders::ALL)
                .border_style(if spawn_state.use_sprite {
                    border_style(spawn_state.active_field == 9)
                } else {
                    Style::default()
                        .fg(colors::BORDER)
                        .add_modifier(Modifier::DIM)
                }),
        );

    // Resources row (10 = RAM, 11 = CPUs) - split horizontally
    let resources_area = chunks[8];
    let resources_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(resources_area);

    // RAM field (10)
    let ram_cursor = if spawn_state.active_field == 10 {
        "▏"
    } else {
        ""
    };
    let ram_widget = Paragraph::new(format!("{}{} MB", spawn_state.ram_mb, ram_cursor))
        .style(field_style(spawn_state.active_field == 10))
        .block(
            Block::default()
                .title(" RAM ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 10)),
        );

    // CPUs field (11)
    let cpus_cursor = if spawn_state.active_field == 11 {
        "▏"
    } else {
        ""
    };
    let cpus_widget = Paragraph::new(format!("{}{} cores", spawn_state.cpus, cpus_cursor))
        .style(field_style(spawn_state.active_field == 11))
        .block(
            Block::default()
                .title(" CPUs ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 11)),
        );

    // Clone destination field (12) - only shown when project_path is a GitHub URL in local mode
    let clone_dest_cursor = if spawn_state.active_field == 12 {
        "▏"
    } else {
        ""
    };
    let clone_dest_placeholder = "e.g. ~/projects/my-clone or /tmp/my-repo";
    let clone_dest_display =
        if spawn_state.clone_destination.is_empty() && spawn_state.active_field != 12 {
            clone_dest_placeholder.to_string()
        } else {
            format!("{}{}", spawn_state.clone_destination, clone_dest_cursor)
        };
    let clone_dest_style =
        if spawn_state.clone_destination.is_empty() && spawn_state.active_field != 12 {
            Style::default().fg(Color::DarkGray)
        } else {
            field_style(spawn_state.active_field == 12)
        };
    let clone_dest_widget = Paragraph::new(clone_dest_display)
        .style(clone_dest_style)
        .block(
            Block::default()
                .title(" Clone Destination (where to clone the repo) ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 12)),
        );

    // Validation error display
    let error_widget = if let Some(ref error) = spawn_state.validation_error {
        Paragraph::new(format!("⚠ {}", error))
            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
    } else {
        Paragraph::new("")
    };

    // Instructions - context-aware
    let instructions_text = if spawn_state.use_sprite {
        "[Tab] Navigate  [Space] Toggle  [←/→] Network  [Enter] Spawn  [Esc] Cancel"
    } else {
        "[Tab] Navigate  [Space] Toggle  [Enter] Spawn  [Esc] Cancel"
    };
    let instructions = Paragraph::new(instructions_text)
        .style(
            Style::default()
                .fg(colors::IDLE)
                .add_modifier(Modifier::DIM),
        )
        .alignment(Alignment::Center);

    // Main dialog block
    let dialog = Block::default()
        .title(" Spawn New Agent ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(colors::HIGHLIGHT))
        .border_type(ratatui::widgets::BorderType::Double)
        .style(Style::default().bg(colors::BG));

    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(dialog, area);
    f.render_widget(project_widget, chunks[0]);
    f.render_widget(prompt_widget, chunks[1]);
    f.render_widget(branch_widget, chunks[2]);
    f.render_widget(worktree_widget, chunks[3]);
    f.render_widget(loop_widget, chunks[4]);
    f.render_widget(iter_widget, loop_options_chunks[0]);
    f.render_widget(stop_widget, loop_options_chunks[1]);
    f.render_widget(role_widget, loop_options_chunks[2]);
    f.render_widget(sprite_widget, chunks[6]);
    f.render_widget(network_widget, chunks[7]);
    f.render_widget(ram_widget, resources_chunks[0]);
    f.render_widget(cpus_widget, resources_chunks[1]);
    if show_clone_dest {
        f.render_widget(clone_dest_widget, chunks[9]);
    }
    f.render_widget(error_widget, chunks[10]);
    f.render_widget(instructions, chunks[11]);
}

// Helper functions

fn status_base_color(status: &Status) -> Color {
    match status {
        Status::Working => colors::WORKING,
        Status::Attention(_) => colors::ATTENTION,
        Status::Compacting => colors::COMPACTING,
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len - 1])
    }
}

fn format_timestamp(ts: i64) -> String {
    use std::time::{Duration, UNIX_EPOCH};
    let datetime = UNIX_EPOCH + Duration::from_secs(ts as u64);
    let now = std::time::SystemTime::now();

    // Simple HH:MM:SS format
    if let Ok(duration) = now.duration_since(datetime) {
        let secs = duration.as_secs();
        let hours = (secs / 3600) % 24;
        let mins = (secs / 60) % 60;
        let secs = secs % 60;
        format!("{hours:02}:{mins:02}:{secs:02}")
    } else {
        "??:??:??".to_string()
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
