//! Help modal

use crate::config::colors;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::super::helpers::centered_rect;

pub fn render_help(f: &mut Frame) {
    let area = centered_rect(55, 75, f.area());

    let help_text = r"
  Navigation
  j/k         Move between agents
  Enter       Jump to agent's terminal
  /           Search agents

  Actions
  y/n         Approve/reject permission
  c           Custom input
  s           Spawn agent

  Views
  d           Dashboard
  T           Task board
  D           Diff viewer
  f           Freeze display
  ?, H        This help

  Git
  g           Commit
  p           Push

  Selection
  Space       Toggle select
  Y/N         Bulk approve/reject
  K           Kill selected
  x           Clear selection

  q, Esc      Quit (Esc closes modals first)
  Ctrl+C      Force quit
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
