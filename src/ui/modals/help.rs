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
