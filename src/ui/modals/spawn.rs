//! Spawn dialog modal

use crate::app::spawn::is_github_url;
use crate::app::SpawnState;
use crate::config::colors;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    prelude::*,
    style::Modifier,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::super::helpers::centered_rect;

pub fn render_spawn_dialog(f: &mut Frame, spawn_state: &SpawnState) {
    let area = centered_rect(70, 75, f.area());

    // Check if we need to show clone destination field
    let show_clone_dest = !spawn_state.use_sprite && is_github_url(&spawn_state.project_path);

    // Field indices:
    // 0=project, 1=prompt, 2=branch, 3=worktree, 4=claude_tasks, 5=task_list_id,
    // 6=sprite, 7=network, 8=ram, 9=cpus, 10=clone_dest
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Project path (0)
            Constraint::Length(3), // Prompt (1)
            Constraint::Length(3), // Branch name (2)
            Constraint::Length(3), // Worktree toggle (3)
            Constraint::Length(3), // Claude Tasks (4) and task_list_id (5)
            Constraint::Length(3), // Sprite toggle (6)
            Constraint::Length(3), // Network policy (7)
            Constraint::Length(3), // Resources: RAM (8), CPUs (9)
            Constraint::Length(if show_clone_dest { 3 } else { 0 }), // Clone destination (10)
            Constraint::Length(2), // Error message
            Constraint::Length(2), // Instructions
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

    // Claude Tasks toggle (4) and task_list_id (5)
    let claude_tasks_area = chunks[4];
    let claude_tasks_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(claude_tasks_area);

    // Claude Tasks checkbox (4)
    let tasks_checkbox = if spawn_state.use_claude_tasks {
        "[x]"
    } else {
        "[ ]"
    };
    let tasks_text = format!("{tasks_checkbox} Use Claude Code Tasks API");
    let tasks_widget = Paragraph::new(tasks_text)
        .style(field_style(spawn_state.active_field == 4))
        .block(
            Block::default()
                .title(" Tasks API ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 4)),
        );

    // Task list ID field (5)
    let task_list_cursor = if spawn_state.active_field == 5 {
        "▏"
    } else {
        ""
    };
    let tasks_enabled = spawn_state.use_claude_tasks;
    let task_list_placeholder = "auto-generated if empty";
    let task_list_display = if spawn_state.task_list_id.is_empty() && spawn_state.active_field != 5
    {
        task_list_placeholder.to_string()
    } else {
        format!("{}{}", spawn_state.task_list_id, task_list_cursor)
    };
    let task_list_style = if !tasks_enabled {
        Style::default().fg(colors::FG).add_modifier(Modifier::DIM)
    } else if spawn_state.task_list_id.is_empty() && spawn_state.active_field != 5 {
        Style::default().fg(Color::DarkGray)
    } else {
        field_style(spawn_state.active_field == 5)
    };
    let task_list_widget = Paragraph::new(task_list_display)
        .style(task_list_style)
        .block(
            Block::default()
                .title(" Task List ID ")
                .borders(Borders::ALL)
                .border_style(if tasks_enabled {
                    border_style(spawn_state.active_field == 5)
                } else {
                    Style::default()
                        .fg(colors::BORDER)
                        .add_modifier(Modifier::DIM)
                }),
        );

    // Sprite toggle (6)
    let sprite_checkbox = if spawn_state.use_sprite { "[x]" } else { "[ ]" };
    let sprite_text = format!("{sprite_checkbox} Run on remote Sprite (cloud VM)");
    let sprite_widget = Paragraph::new(sprite_text)
        .style(field_style(spawn_state.active_field == 6))
        .block(
            Block::default()
                .title(" Sprite Mode ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 6)),
        );

    // Network policy selector (7) - only visible when sprite mode is enabled
    let network_display = if spawn_state.use_sprite {
        spawn_state.network_preset.display()
    } else {
        "(enable Sprite mode to configure)"
    };
    let network_widget = Paragraph::new(format!("<  {network_display}  >"))
        .style(if spawn_state.use_sprite {
            field_style(spawn_state.active_field == 7)
        } else {
            Style::default().fg(colors::FG).add_modifier(Modifier::DIM)
        })
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .title(" Network Policy (←/→ to change) ")
                .borders(Borders::ALL)
                .border_style(if spawn_state.use_sprite {
                    border_style(spawn_state.active_field == 7)
                } else {
                    Style::default()
                        .fg(colors::BORDER)
                        .add_modifier(Modifier::DIM)
                }),
        );

    // Resources row (8 = RAM, 9 = CPUs) - split horizontally
    let resources_area = chunks[7];
    let resources_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(resources_area);

    // RAM field (8)
    let ram_cursor = if spawn_state.active_field == 8 {
        "▏"
    } else {
        ""
    };
    let ram_widget = Paragraph::new(format!("{}{} MB", spawn_state.ram_mb, ram_cursor))
        .style(field_style(spawn_state.active_field == 8))
        .block(
            Block::default()
                .title(" RAM ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 8)),
        );

    // CPUs field (9)
    let cpus_cursor = if spawn_state.active_field == 9 {
        "▏"
    } else {
        ""
    };
    let cpus_widget = Paragraph::new(format!("{}{} cores", spawn_state.cpus, cpus_cursor))
        .style(field_style(spawn_state.active_field == 9))
        .block(
            Block::default()
                .title(" CPUs ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 9)),
        );

    // Clone destination field (10) - only shown when project_path is a GitHub URL in local mode
    let clone_dest_cursor = if spawn_state.active_field == 10 {
        "▏"
    } else {
        ""
    };
    let clone_dest_placeholder = "e.g. ~/projects/my-clone or /tmp/my-repo";
    let clone_dest_display =
        if spawn_state.clone_destination.is_empty() && spawn_state.active_field != 10 {
            clone_dest_placeholder.to_string()
        } else {
            format!("{}{}", spawn_state.clone_destination, clone_dest_cursor)
        };
    let clone_dest_style =
        if spawn_state.clone_destination.is_empty() && spawn_state.active_field != 10 {
            Style::default().fg(Color::DarkGray)
        } else {
            field_style(spawn_state.active_field == 10)
        };
    let clone_dest_widget = Paragraph::new(clone_dest_display)
        .style(clone_dest_style)
        .block(
            Block::default()
                .title(" Clone Destination (where to clone the repo) ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 10)),
        );

    // Validation error display
    let error_widget = if let Some(ref error) = spawn_state.validation_error {
        Paragraph::new(format!("⚠ {}", error))
            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
    } else {
        Paragraph::new("")
    };

    // Instructions
    let instructions = Paragraph::new(
        "[Tab] Navigate  [Space] Toggle  [←/→] Selector  [Enter] Spawn  [Esc] Cancel",
    )
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
    f.render_widget(tasks_widget, claude_tasks_chunks[0]);
    f.render_widget(task_list_widget, claude_tasks_chunks[1]);
    f.render_widget(sprite_widget, chunks[5]);
    f.render_widget(network_widget, chunks[6]);
    f.render_widget(ram_widget, resources_chunks[0]);
    f.render_widget(cpus_widget, resources_chunks[1]);
    if show_clone_dest {
        f.render_widget(clone_dest_widget, chunks[8]);
    }
    f.render_widget(error_widget, chunks[9]);
    f.render_widget(instructions, chunks[10]);
}
