//! Help modal

use crate::config::colors;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::super::helpers::centered_rect;

pub fn render_help(f: &mut Frame) {
    let area = centered_rect(55, 80, f.area());

    let help_text = r"
  Views (uppercase)
  T            Task board
  P            Plan viewer
  S            Stats dashboard
  L            History log
  D            Debug viewer
  I            Insights report
  ?, H         This help

  Navigation
  j/k, Up/Dn   Move between agents
  Enter        Jump to agent's terminal
  /            Search agents

  Actions
  s            Spawn agent

  Search Mode
  Esc          Cancel search
  Enter        Confirm / jump to match
  Type         Filter agents

  Spawn Mode
  Tab / Dn     Next field
  Shift+Tab/Up Previous field
  Enter        Submit / toggle
  Esc          Cancel

  q, Esc       Quit (Esc closes modals first)
  Ctrl+C       Force quit
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
