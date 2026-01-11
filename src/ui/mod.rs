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
    // Use cached status counts (O(1) instead of O(4n))
    let [attention, working, compacting, idle] = app.state.status_counts;
    let total = app.state.agents.len();

    // Build status summary
    let status_parts: Vec<String> = [
        (attention, "attention"),
        (working, "working"),
        (compacting, "compacting"),
        (idle, "idle"),
    ]
    .iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, label)| format!("{} {}", count, label))
    .collect();

    let frozen_indicator = if app.frozen { " [FROZEN]" } else { "" };
    let view_indicator = match app.view_mode {
        ViewMode::Kanban => "",
        ViewMode::Project => " [PROJECT VIEW]",
    };
    let title = if total == 0 {
        format!("Rehoboam{}{}", frozen_indicator, view_indicator)
    } else {
        format!(
            "Rehoboam ({} agents: {}){}{}",
            total,
            status_parts.join(", "),
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

    // 4 equal-width columns
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, NUM_COLUMNS as u32),
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
        let header = format!("📁 {} ({} agent{})", project_name, agents.len(), if agents.len() == 1 { "" } else { "s" });
        items.push(ListItem::new(Line::from(vec![
            Span::styled(header, Style::default().fg(colors::HIGHLIGHT).add_modifier(Modifier::BOLD)),
        ])));

        // Agent entries under this project
        for agent in agents {
            let (icon, color) = match &agent.status {
                Status::Attention(_) => ("🔔", colors::ATTENTION),
                Status::Working => ("🤖", colors::WORKING),
                Status::Compacting => ("🔄", colors::COMPACTING),
                Status::Idle => ("⏸️ ", colors::IDLE),
            };

            let status_str = match &agent.status {
                Status::Attention(_) => "Attention",
                Status::Working => "Working",
                Status::Compacting => "Compacting",
                Status::Idle => "Idle",
            };

            let tool_info = agent.tool_display();
            let elapsed = agent.elapsed_display();

            let line = format!(
                "  {} {} ({}) {} {}",
                icon,
                agent.pane_id,
                status_str,
                tool_info,
                elapsed
            );

            items.push(ListItem::new(Line::from(vec![
                Span::styled(line, Style::default().fg(color)),
            ])));
        }

        // Add spacing between projects
        items.push(ListItem::new(""));
    }

    let list = List::new(items).block(
        Block::default()
            .title(" Projects [P to toggle Kanban] ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER))
            .border_type(ratatui::widgets::BorderType::Rounded),
    );

    f.render_widget(list, area);
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
    let selection_count = app.state.selected_agents.len();
    let mode_indicators: Vec<String> = [
        app.debug_mode.then_some("[debug]".to_string()),
        app.frozen.then_some("[frozen]".to_string()),
        app.auto_accept.then_some("[AUTO]".to_string()),
        if selection_count > 0 {
            Some(format!("[{} selected]", selection_count))
        } else {
            None
        },
    ]
    .into_iter()
    .flatten()
    .collect();

    let mode = if mode_indicators.is_empty() {
        String::new()
    } else {
        format!("{} ", mode_indicators.join(" "))
    };

    let help = format!(
        "{}Space:select  Y/N:bulk  K:kill  y/n:approve  c:input  s:spawn  ?:help",
        mode
    );

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
    let area = centered_rect(50, 60, f.area());

    let help_text = r#"
  Navigation
  h/l         Column left/right
  j/k         Card up/down
  Enter       Jump to agent pane

  Single Agent
  y/n         Approve/reject permission
  c           Custom input (type & send)
  s           Spawn new agent

  Bulk Operations
  Space       Toggle selection
  Y/N         Bulk approve/reject
  K           Kill selected agents
  x           Clear selection

  Display
  P           Toggle Kanban/Project view
  A           Toggle auto-accept mode
  f           Freeze display
  d           Debug mode
  ?, H        This help
  q, Esc      Quit
"#;

    let help = Paragraph::new(help_text)
        .style(Style::default().fg(colors::FG))
        .block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::HIGHLIGHT))
                .border_type(ratatui::widgets::BorderType::Rounded)
                .style(Style::default().bg(colors::BG)),
        );

    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(help, area);
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
    let area = centered_rect(70, 70, f.area());

    // Split into fields: project, prompt, branch, worktree toggle, loop toggle, loop options, instructions
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Project path (0)
            Constraint::Length(3), // Prompt (1)
            Constraint::Length(3), // Branch name (2)
            Constraint::Length(3), // Worktree toggle (3)
            Constraint::Length(3), // Loop mode toggle (4)
            Constraint::Length(3), // Loop options (5, 6)
            Constraint::Length(2), // Instructions
        ])
        .margin(1)
        .split(area);

    // Helper for field styling
    let field_style = |active: bool| {
        if active {
            Style::default().fg(colors::HIGHLIGHT)
        } else {
            Style::default().fg(colors::FG)
        }
    };
    let border_style = |active: bool| {
        if active {
            Style::default().fg(colors::HIGHLIGHT)
        } else {
            Style::default().fg(colors::BORDER)
        }
    };

    // Project path field (0)
    let project_cursor = if spawn_state.active_field == 0 { "▏" } else { "" };
    let project_widget = Paragraph::new(format!("{}{}", spawn_state.project_path, project_cursor))
        .style(field_style(spawn_state.active_field == 0))
        .block(
            Block::default()
                .title(" Project Path ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 0)),
        );

    // Prompt field (1)
    let prompt_cursor = if spawn_state.active_field == 1 { "▏" } else { "" };
    let prompt_widget = Paragraph::new(format!("{}{}", spawn_state.prompt, prompt_cursor))
        .style(field_style(spawn_state.active_field == 1))
        .block(
            Block::default()
                .title(" Prompt (optional) ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 1)),
        );

    // Branch name field (2)
    let branch_cursor = if spawn_state.active_field == 2 { "▏" } else { "" };
    let branch_widget = Paragraph::new(format!("{}{}", spawn_state.branch_name, branch_cursor))
        .style(field_style(spawn_state.active_field == 2))
        .block(
            Block::default()
                .title(" Branch Name (for worktree) ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 2)),
        );

    // Worktree toggle (3)
    let checkbox = if spawn_state.use_worktree { "[x]" } else { "[ ]" };
    let worktree_text = format!("{} Create isolated git worktree", checkbox);
    let worktree_widget = Paragraph::new(worktree_text)
        .style(field_style(spawn_state.active_field == 3))
        .block(
            Block::default()
                .title(" Git Isolation ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 3)),
        );

    // Loop mode toggle (4)
    let loop_checkbox = if spawn_state.loop_enabled { "[x]" } else { "[ ]" };
    let loop_text = format!("{} Enable Loop Mode (Ralph-style autonomy)", loop_checkbox);
    let loop_widget = Paragraph::new(loop_text)
        .style(field_style(spawn_state.active_field == 4))
        .block(
            Block::default()
                .title(" Loop Mode ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 4)),
        );

    // Loop options (5 = max iterations, 6 = stop word) - show side by side
    let loop_options_area = chunks[5];
    let loop_options_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(loop_options_area);

    // Max iterations field (5)
    let iter_cursor = if spawn_state.active_field == 5 { "▏" } else { "" };
    let iter_widget =
        Paragraph::new(format!("{}{}", spawn_state.loop_max_iterations, iter_cursor))
            .style(field_style(spawn_state.active_field == 5))
            .block(
                Block::default()
                    .title(" Max Iter ")
                    .borders(Borders::ALL)
                    .border_style(border_style(spawn_state.active_field == 5)),
            );

    // Stop word field (6)
    let stop_cursor = if spawn_state.active_field == 6 { "▏" } else { "" };
    let stop_widget = Paragraph::new(format!("{}{}", spawn_state.loop_stop_word, stop_cursor))
        .style(field_style(spawn_state.active_field == 6))
        .block(
            Block::default()
                .title(" Stop Word ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 6)),
        );

    // Instructions
    let instructions =
        Paragraph::new("[Tab/↑↓] Navigate  [Space] Toggle  [Enter] Spawn  [Esc] Cancel")
            .style(Style::default().fg(colors::IDLE))
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
    f.render_widget(instructions, chunks[6]);
}

// Helper functions

fn status_base_color(status: &Status) -> Color {
    match status {
        Status::Working => colors::WORKING,
        Status::Attention(_) => colors::ATTENTION,
        Status::Compacting => colors::COMPACTING,
        Status::Idle => colors::IDLE,
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
        format!("{:02}:{:02}:{:02}", hours, mins, secs)
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
