//! Spawn dialog modal

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
    let area = centered_rect(70, 55, f.area());

    // Field indices: 0=project/repo, 1=prompt, 2=sprite toggle, 3=network, 4=submit
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Project path / GitHub repo (0)
            Constraint::Length(3), // Prompt (1)
            Constraint::Length(3), // Sprite toggle (2)
            Constraint::Length(3), // Network policy (3)
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

    // Sprite toggle (2)
    let sprite_checkbox = if spawn_state.use_sprite { "[x]" } else { "[ ]" };
    let sprite_text = format!("{sprite_checkbox} Run on remote Sprite (cloud VM)");
    let sprite_widget = Paragraph::new(sprite_text)
        .style(field_style(spawn_state.active_field == 2))
        .block(
            Block::default()
                .title(" Sprite Mode ")
                .borders(Borders::ALL)
                .border_style(border_style(spawn_state.active_field == 2)),
        );

    // Network policy selector (3) - only visible when sprite mode is enabled
    let network_display = if spawn_state.use_sprite {
        spawn_state.network_preset.display()
    } else {
        "(enable Sprite mode to configure)"
    };
    let network_widget = Paragraph::new(format!("<  {network_display}  >"))
        .style(if spawn_state.use_sprite {
            field_style(spawn_state.active_field == 3)
        } else {
            Style::default().fg(colors::FG).add_modifier(Modifier::DIM)
        })
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .title(" Network Policy (←/→ to change) ")
                .borders(Borders::ALL)
                .border_style(if spawn_state.use_sprite {
                    border_style(spawn_state.active_field == 3)
                } else {
                    Style::default()
                        .fg(colors::BORDER)
                        .add_modifier(Modifier::DIM)
                }),
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
    f.render_widget(sprite_widget, chunks[2]);
    f.render_widget(network_widget, chunks[3]);
    f.render_widget(error_widget, chunks[4]);
    f.render_widget(instructions, chunks[5]);
}
