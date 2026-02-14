//! Keyboard input handling for all modes
//!
//! Routes keyboard events to mode-specific handlers based on [`InputMode`].
//!
//! # Keyboard Layout
//!
//! **Uppercase = Views** (open overlays), **lowercase = actions** (do things).
//!
//! ## Navigation
//! - `j`/`↓` - Move to next agent
//! - `k`/`↑` - Move to previous agent
//! - `Enter` - Jump to selected agent's tmux pane
//! - `/` - Enter search mode
//!
//! ## Views (uppercase)
//! - `T` - Toggle task board overlay
//! - `P` - Toggle plan viewer
//! - `S` - Toggle stats dashboard
//! - `L` - Toggle history log
//! - `D` - Toggle debug viewer
//! - `I` - Toggle insights report
//! - `?`/`H` - Toggle help
//!
//! ## Actions (lowercase)
//! - `s` - Open spawn dialog
//!
//! ## Application
//! - `q` - Quit application
//! - `Esc` - Close current overlay or quit
//! - `Ctrl+C` - Force quit

use super::{navigation, spawn, App, InputMode};
use crossterm::event::{KeyCode, KeyModifiers};

impl App {
    /// Handle keyboard input
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        // Handle Ctrl+C always
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        // Route to mode-specific handlers
        match self.input_mode {
            InputMode::Normal => self.handle_key_normal(key),
            InputMode::Spawn => self.handle_key_spawn(key),
            InputMode::Search => self.handle_key_search(key),
            InputMode::PlanViewer => self.handle_key_plan_viewer(key),
            InputMode::StatsViewer => self.handle_key_stats_viewer(key),
            InputMode::HistoryViewer => self.handle_key_history_viewer(key),
            InputMode::DebugViewer => self.handle_key_debug_viewer(key),
            InputMode::InsightsViewer => self.handle_key_insights_viewer(key),
        }
    }

    /// Handle keyboard input in Normal mode
    fn handle_key_normal(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            // Quit (but Esc first closes overlays)
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            // Esc cascade: close overlays in priority order
            // Note: Stats/History/Debug/Insights/Plan viewers use dedicated InputModes
            // and handle their own Esc — they never reach this Normal mode handler.
            // Only help and task_board stay in Normal mode, so only they need handling here.
            KeyCode::Esc => {
                if self.show_help {
                    self.show_help = false;
                } else if self.show_task_board {
                    self.show_task_board = false;
                } else {
                    self.should_quit = true;
                }
            }
            // Agent navigation (flat across all teams)
            KeyCode::Char('j') | KeyCode::Down => {
                self.state.next_agent();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.state.prev_agent();
            }
            // Jump to agent
            KeyCode::Enter => {
                navigation::jump_to_selected(&self.state);
            }
            // Toggle help
            KeyCode::Char('?' | 'H') => {
                self.show_help = !self.show_help;
            }
            // Toggle task board
            KeyCode::Char('T') => {
                self.show_task_board = !self.show_task_board;
                tracing::debug!(show_task_board = self.show_task_board, "Toggled task board");
            }

            // === Agent spawning ===
            KeyCode::Char('s') => {
                self.input_mode = InputMode::Spawn;
                self.spawn_state = spawn::SpawnState::default();
                if let Ok(cwd) = std::env::current_dir() {
                    self.spawn_state.project_path = cwd.display().to_string();
                }
                tracing::debug!("Entering spawn mode");
            }

            // Plan viewer
            KeyCode::Char('P') => {
                if self.show_plan_viewer {
                    self.show_plan_viewer = false;
                    self.input_mode = InputMode::Normal;
                } else {
                    self.plan_viewer.plans = crate::plans::discover_plans();
                    self.plan_viewer.selected_index = 0;
                    self.plan_viewer.viewing = false;
                    self.show_plan_viewer = true;
                    self.input_mode = InputMode::PlanViewer;
                    tracing::debug!(
                        count = self.plan_viewer.plans.len(),
                        "Opened plan viewer"
                    );
                }
            }

            // Stats dashboard
            KeyCode::Char('S') => {
                if self.show_stats_viewer {
                    self.show_stats_viewer = false;
                    self.input_mode = InputMode::Normal;
                } else {
                    self.show_stats_viewer = true;
                    self.stats_viewer = super::StatsViewerState::default();
                    self.input_mode = InputMode::StatsViewer;
                    tracing::debug!("Opened stats dashboard");
                }
            }

            // History timeline
            KeyCode::Char('L') => {
                if self.show_history_viewer {
                    self.show_history_viewer = false;
                    self.input_mode = InputMode::Normal;
                } else {
                    // Force initial data load
                    self.state.refresh_history_data();
                    self.show_history_viewer = true;
                    self.history_viewer = super::HistoryViewerState::default();
                    self.input_mode = InputMode::HistoryViewer;
                    tracing::debug!("Opened history viewer");
                }
            }

            // Debug log viewer
            KeyCode::Char('D') => {
                if self.show_debug_viewer {
                    self.show_debug_viewer = false;
                    self.input_mode = InputMode::Normal;
                } else {
                    // Force initial data load
                    self.state.refresh_debug_data();
                    self.show_debug_viewer = true;
                    self.debug_viewer = super::DebugViewerState::default();
                    self.input_mode = InputMode::DebugViewer;
                    tracing::debug!("Opened debug viewer");
                }
            }

            // Insights report
            KeyCode::Char('I') => {
                if self.show_insights_viewer {
                    self.show_insights_viewer = false;
                    self.input_mode = InputMode::Normal;
                } else {
                    // Force initial data load
                    self.state.refresh_insights_data();
                    self.show_insights_viewer = true;
                    self.insights_viewer = super::InsightsViewerState::default();
                    self.input_mode = InputMode::InsightsViewer;
                    tracing::debug!("Opened insights viewer");
                }
            }

            // Agent search
            KeyCode::Char('/') => {
                self.input_mode = InputMode::Search;
                self.search_query.clear();
                tracing::debug!("Entering search mode");
            }

            _ => {}
        }
    }

    /// Handle keyboard input in Spawn mode
    fn handle_key_spawn(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.spawn_state = spawn::SpawnState::default();
                tracing::debug!("Cancelled spawn mode");
            }
            KeyCode::Tab | KeyCode::Down => {
                self.spawn_state.active_field =
                    (self.spawn_state.active_field + 1) % spawn::SPAWN_FIELD_COUNT;
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.spawn_state.active_field =
                    (self.spawn_state.active_field + spawn::SPAWN_FIELD_COUNT - 1)
                        % spawn::SPAWN_FIELD_COUNT;
            }
            KeyCode::Enter => {
                // Toggle fields (2 = sprite toggle)
                match self.spawn_state.active_field {
                    2 => self.spawn_state.use_sprite = !self.spawn_state.use_sprite,
                    _ => match spawn::validate_spawn(
                        &self.spawn_state,
                        self.sprites_client.is_some(),
                    ) {
                        Ok(()) => {
                            self.spawn_state.validation_error = None;
                            spawn::spawn_agent(
                                &self.spawn_state,
                                self.sprites_client.as_ref(),
                                &mut self.state,
                            );
                            self.input_mode = InputMode::Normal;
                            self.spawn_state = spawn::SpawnState::default();
                        }
                        Err(msg) => {
                            self.spawn_state.validation_error = Some(msg);
                        }
                    },
                }
            }
            KeyCode::Char(' ') => match self.spawn_state.active_field {
                2 => self.spawn_state.use_sprite = !self.spawn_state.use_sprite,
                0 => {
                    if self.spawn_state.use_sprite {
                        self.spawn_state.github_repo.push(' ');
                    } else {
                        self.spawn_state.project_path.push(' ');
                    }
                }
                1 => self.spawn_state.prompt.push(' '),
                _ => {}
            },
            KeyCode::Left => {
                if self.spawn_state.active_field == 3 {
                    self.spawn_state.network_preset = self.spawn_state.network_preset.prev();
                }
            }
            KeyCode::Right => {
                if self.spawn_state.active_field == 3 {
                    self.spawn_state.network_preset = self.spawn_state.network_preset.next();
                }
            }
            KeyCode::Backspace => match self.spawn_state.active_field {
                0 => {
                    if self.spawn_state.use_sprite {
                        self.spawn_state.github_repo.pop();
                    } else {
                        self.spawn_state.project_path.pop();
                    }
                }
                1 => {
                    self.spawn_state.prompt.pop();
                }
                _ => {}
            },
            KeyCode::Char(c) => match self.spawn_state.active_field {
                0 => {
                    if self.spawn_state.use_sprite {
                        self.spawn_state.github_repo.push(c);
                    } else {
                        self.spawn_state.project_path.push(c);
                    }
                }
                1 => self.spawn_state.prompt.push(c),
                2 => {
                    // Sprite toggle - y/n to toggle
                    if c == 'y' || c == 'Y' {
                        self.spawn_state.use_sprite = true;
                    } else if c == 'n' || c == 'N' {
                        self.spawn_state.use_sprite = false;
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    /// Handle keyboard input in Search mode
    fn handle_key_search(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.search_query.clear();
                tracing::debug!("Cancelled search mode");
            }
            KeyCode::Enter => {
                if !self.search_query.is_empty() {
                    navigation::jump_to_search_match(&mut self.state, &self.search_query);
                }
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Backspace => {
                self.search_query.pop();
            }
            KeyCode::Char(c) => {
                self.search_query.push(c);
            }
            _ => {}
        }
    }

    /// Handle keyboard input in PlanViewer mode
    fn handle_key_plan_viewer(&mut self, key: crossterm::event::KeyEvent) {
        if self.plan_viewer.viewing {
            // Reader mode: scrolling through a plan
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    // Back to list
                    self.plan_viewer.viewing = false;
                    self.plan_viewer.content.clear();
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if self.plan_viewer.scroll_offset
                        < self.plan_viewer.rendered_height.saturating_sub(1)
                    {
                        self.plan_viewer.scroll_offset += 1;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.plan_viewer.scroll_offset =
                        self.plan_viewer.scroll_offset.saturating_sub(1);
                }
                KeyCode::Char('d') | KeyCode::PageDown => {
                    let jump = 20;
                    self.plan_viewer.scroll_offset = self
                        .plan_viewer
                        .scroll_offset
                        .saturating_add(jump)
                        .min(self.plan_viewer.rendered_height.saturating_sub(1));
                }
                KeyCode::Char('u') | KeyCode::PageUp => {
                    self.plan_viewer.scroll_offset =
                        self.plan_viewer.scroll_offset.saturating_sub(20);
                }
                KeyCode::Char('g') => {
                    self.plan_viewer.scroll_offset = 0;
                }
                KeyCode::Char('G') => {
                    self.plan_viewer.scroll_offset =
                        self.plan_viewer.rendered_height.saturating_sub(1);
                }
                KeyCode::Char('n') => {
                    self.plan_viewer.next_plan();
                }
                KeyCode::Char('p') => {
                    self.plan_viewer.prev_plan();
                }
                _ => {}
            }
        } else {
            // List mode: browsing plans
            match key.code {
                KeyCode::Esc => {
                    self.show_plan_viewer = false;
                    self.input_mode = InputMode::Normal;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if !self.plan_viewer.plans.is_empty() {
                        self.plan_viewer.selected_index =
                            (self.plan_viewer.selected_index + 1) % self.plan_viewer.plans.len();
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    if !self.plan_viewer.plans.is_empty() {
                        self.plan_viewer.selected_index =
                            if self.plan_viewer.selected_index == 0 {
                                self.plan_viewer.plans.len() - 1
                            } else {
                                self.plan_viewer.selected_index - 1
                            };
                    }
                }
                KeyCode::Enter => {
                    self.plan_viewer.load_selected();
                }
                _ => {}
            }
        }
    }

    /// Handle keyboard input in StatsViewer mode
    fn handle_key_stats_viewer(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.show_stats_viewer = false;
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Tab => {
                self.stats_viewer.active_tab = (self.stats_viewer.active_tab + 1) % 4;
                self.stats_viewer.scroll_offset = 0;
            }
            KeyCode::BackTab => {
                self.stats_viewer.active_tab =
                    (self.stats_viewer.active_tab + 3) % 4;
                self.stats_viewer.scroll_offset = 0;
            }
            KeyCode::Char('1') => {
                self.stats_viewer.active_tab = 0;
                self.stats_viewer.scroll_offset = 0;
            }
            KeyCode::Char('2') => {
                self.stats_viewer.active_tab = 1;
                self.stats_viewer.scroll_offset = 0;
            }
            KeyCode::Char('3') => {
                self.stats_viewer.active_tab = 2;
                self.stats_viewer.scroll_offset = 0;
            }
            KeyCode::Char('4') => {
                self.stats_viewer.active_tab = 3;
                self.stats_viewer.scroll_offset = 0;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.stats_viewer.scroll_offset =
                    self.stats_viewer.scroll_offset.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.stats_viewer.scroll_offset =
                    self.stats_viewer.scroll_offset.saturating_sub(1);
            }
            _ => {}
        }
    }

    /// Handle keyboard input in HistoryViewer mode
    fn handle_key_history_viewer(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.show_history_viewer = false;
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                let count = self.state.history_entries.len();
                if count > 0 {
                    self.history_viewer.selected_index =
                        (self.history_viewer.selected_index + 1).min(count - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.history_viewer.selected_index =
                    self.history_viewer.selected_index.saturating_sub(1);
            }
            _ => {}
        }
    }

    /// Handle keyboard input in DebugViewer mode
    fn handle_key_debug_viewer(&mut self, key: crossterm::event::KeyEvent) {
        if self.debug_viewer.viewing {
            // Reader mode: scrolling through log content
            match key.code {
                KeyCode::Esc => {
                    self.debug_viewer.viewing = false;
                    self.debug_viewer.content.clear();
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if self.debug_viewer.scroll_offset
                        < self.debug_viewer.rendered_height.saturating_sub(1)
                    {
                        self.debug_viewer.scroll_offset += 1;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.debug_viewer.scroll_offset =
                        self.debug_viewer.scroll_offset.saturating_sub(1);
                }
                KeyCode::Char('d') | KeyCode::PageDown => {
                    self.debug_viewer.scroll_offset = self
                        .debug_viewer
                        .scroll_offset
                        .saturating_add(20)
                        .min(self.debug_viewer.rendered_height.saturating_sub(1));
                }
                KeyCode::Char('u') | KeyCode::PageUp => {
                    self.debug_viewer.scroll_offset =
                        self.debug_viewer.scroll_offset.saturating_sub(20);
                }
                KeyCode::Char('g') => {
                    self.debug_viewer.scroll_offset = 0;
                }
                KeyCode::Char('G') => {
                    self.debug_viewer.scroll_offset =
                        self.debug_viewer.rendered_height.saturating_sub(1);
                }
                _ => {}
            }
        } else {
            // List mode: browsing debug logs
            match key.code {
                KeyCode::Esc => {
                    self.show_debug_viewer = false;
                    self.input_mode = InputMode::Normal;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    let count = self.state.debug_log_entries.len();
                    if count > 0 {
                        self.debug_viewer.selected_index =
                            (self.debug_viewer.selected_index + 1).min(count - 1);
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.debug_viewer.selected_index =
                        self.debug_viewer.selected_index.saturating_sub(1);
                }
                KeyCode::Enter => {
                    // Load selected debug log content
                    if let Some(entry) = self
                        .state
                        .debug_log_entries
                        .get(self.debug_viewer.selected_index)
                    {
                        self.debug_viewer.content =
                            std::fs::read_to_string(&entry.path).unwrap_or_default();
                        self.debug_viewer.scroll_offset = 0;
                        self.debug_viewer.rendered_height = 0;
                        self.debug_viewer.viewing = true;
                    }
                }
                _ => {}
            }
        }
    }

    /// Handle keyboard input in InsightsViewer mode
    fn handle_key_insights_viewer(&mut self, key: crossterm::event::KeyEvent) {
        let section_count = self
            .state
            .insights_report
            .as_ref()
            .map(|r| r.sections.len())
            .unwrap_or(0);

        match key.code {
            KeyCode::Esc => {
                self.show_insights_viewer = false;
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Tab => {
                if section_count > 0 {
                    self.insights_viewer.active_section =
                        (self.insights_viewer.active_section + 1) % section_count;
                    self.insights_viewer.scroll_offset = 0;
                }
            }
            KeyCode::BackTab => {
                if section_count > 0 {
                    self.insights_viewer.active_section =
                        (self.insights_viewer.active_section + section_count - 1) % section_count;
                    self.insights_viewer.scroll_offset = 0;
                }
            }
            KeyCode::Char(c @ '1'..='9') => {
                let idx = (c as usize) - ('1' as usize);
                if idx < section_count {
                    self.insights_viewer.active_section = idx;
                    self.insights_viewer.scroll_offset = 0;
                }
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.insights_viewer.scroll_offset =
                    self.insights_viewer.scroll_offset.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.insights_viewer.scroll_offset =
                    self.insights_viewer.scroll_offset.saturating_sub(1);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HealthConfig, TimeoutConfig};
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    /// Create a test App instance
    fn test_app() -> App {
        App::new(
            false,
            None,
            &HealthConfig::default(),
            &TimeoutConfig::default(),
        )
    }

    /// Create a key event from a character
    fn key(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    /// Create a key event from a KeyCode
    fn key_code(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    /// Create a Ctrl+key event
    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            modifiers: KeyModifiers::CONTROL,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn test_quit_on_q() {
        let mut app = test_app();
        assert!(!app.should_quit);

        app.handle_key(key('q'));
        assert!(app.should_quit, "pressing 'q' should quit");
    }

    #[test]
    fn test_quit_on_ctrl_c() {
        let mut app = test_app();
        assert!(!app.should_quit);

        app.handle_key(ctrl_key('c'));
        assert!(app.should_quit, "Ctrl+C should quit");
    }

    #[test]
    fn test_help_toggle() {
        let mut app = test_app();
        assert!(!app.show_help);

        app.handle_key(key('?'));
        assert!(app.show_help, "'?' should toggle help on");

        app.handle_key(key('?'));
        assert!(!app.show_help, "'?' should toggle help off");
    }

    #[test]
    fn test_spawn_mode_entry() {
        let mut app = test_app();
        assert_eq!(app.input_mode, InputMode::Normal);

        app.handle_key(key('s'));
        assert_eq!(
            app.input_mode,
            InputMode::Spawn,
            "'s' should enter spawn mode"
        );
    }

    #[test]
    fn test_spawn_mode_escape() {
        let mut app = test_app();
        app.input_mode = InputMode::Spawn;

        app.handle_key(key_code(KeyCode::Esc));
        assert_eq!(
            app.input_mode,
            InputMode::Normal,
            "Esc should exit spawn mode"
        );
    }

    #[test]
    fn test_search_mode_entry() {
        let mut app = test_app();
        assert_eq!(app.input_mode, InputMode::Normal);

        app.handle_key(key('/'));
        assert_eq!(
            app.input_mode,
            InputMode::Search,
            "'/' should enter search mode"
        );
    }

    #[test]
    fn test_search_mode_typing() {
        let mut app = test_app();
        app.input_mode = InputMode::Search;

        app.handle_key(key('t'));
        app.handle_key(key('e'));
        app.handle_key(key('s'));
        app.handle_key(key('t'));

        assert_eq!(app.search_query, "test", "characters should be appended");
    }

    #[test]
    fn test_search_mode_backspace() {
        let mut app = test_app();
        app.input_mode = InputMode::Search;
        app.search_query = "test".to_string();

        app.handle_key(key_code(KeyCode::Backspace));
        assert_eq!(app.search_query, "tes", "backspace should remove last char");
    }

    #[test]
    fn test_search_mode_escape() {
        let mut app = test_app();
        app.input_mode = InputMode::Search;
        app.search_query = "test".to_string();

        app.handle_key(key_code(KeyCode::Esc));
        assert_eq!(app.input_mode, InputMode::Normal, "Esc should exit search");
        assert!(app.search_query.is_empty(), "Esc should clear query");
    }

    #[test]
    fn test_esc_closes_help_first() {
        let mut app = test_app();
        app.show_help = true;

        app.handle_key(key_code(KeyCode::Esc));
        assert!(!app.show_help, "Esc should close help overlay");
        assert!(!app.should_quit, "Esc should not quit when help is open");
    }

    #[test]
    fn test_spawn_field_navigation() {
        let mut app = test_app();
        app.input_mode = InputMode::Spawn;
        assert_eq!(app.spawn_state.active_field, 0);

        app.handle_key(key_code(KeyCode::Tab));
        assert_eq!(
            app.spawn_state.active_field, 1,
            "Tab should move to next field"
        );

        app.handle_key(key_code(KeyCode::BackTab));
        assert_eq!(
            app.spawn_state.active_field, 0,
            "Shift+Tab should move to previous field"
        );
    }

    #[test]
    fn test_task_board_toggle() {
        let mut app = test_app();
        assert!(!app.show_task_board);

        app.handle_key(key('T'));
        assert!(app.show_task_board, "'T' should toggle task board on");

        app.handle_key(key('T'));
        assert!(!app.show_task_board, "'T' should toggle task board off");
    }

    #[test]
    fn test_esc_closes_task_board() {
        let mut app = test_app();
        app.show_task_board = true;

        app.handle_key(key_code(KeyCode::Esc));
        assert!(!app.show_task_board, "Esc should close task board");
        assert!(
            !app.should_quit,
            "Esc should not quit when task board is open"
        );
    }

    #[test]
    fn test_stats_viewer_toggle() {
        let mut app = test_app();
        assert!(!app.show_stats_viewer);

        app.handle_key(key('S'));
        assert!(app.show_stats_viewer, "'S' should open stats viewer");
        assert_eq!(app.input_mode, InputMode::StatsViewer);
    }

    #[test]
    fn test_history_viewer_toggle() {
        let mut app = test_app();
        assert!(!app.show_history_viewer);

        app.handle_key(key('L'));
        assert!(app.show_history_viewer, "'L' should open history viewer");
        assert_eq!(app.input_mode, InputMode::HistoryViewer);
    }

    #[test]
    fn test_debug_viewer_toggle() {
        let mut app = test_app();
        assert!(!app.show_debug_viewer);

        app.handle_key(key('D'));
        assert!(app.show_debug_viewer, "'D' should open debug viewer");
        assert_eq!(app.input_mode, InputMode::DebugViewer);
    }

    #[test]
    fn test_insights_viewer_toggle() {
        let mut app = test_app();
        assert!(!app.show_insights_viewer);

        app.handle_key(key('I'));
        assert!(app.show_insights_viewer, "'I' should open insights viewer");
        assert_eq!(app.input_mode, InputMode::InsightsViewer);
    }

    #[test]
    fn test_esc_cascade_stats() {
        let mut app = test_app();
        app.show_stats_viewer = true;
        app.input_mode = InputMode::StatsViewer;

        // Esc in StatsViewer mode should close it
        app.handle_key(key_code(KeyCode::Esc));
        assert!(!app.show_stats_viewer);
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn test_esc_cascade_order() {
        let mut app = test_app();
        // In Normal mode, only help and task_board are handled.
        // Viewers (Stats/History/Debug/Insights) use dedicated InputModes.
        app.show_help = true;
        app.show_task_board = true;

        // First Esc closes help (highest priority)
        app.handle_key(key_code(KeyCode::Esc));
        assert!(!app.show_help, "Esc should close help first");
        assert!(app.show_task_board, "Task board should still be open");
        assert!(!app.should_quit);

        // Second Esc closes task board
        app.handle_key(key_code(KeyCode::Esc));
        assert!(!app.show_task_board, "Esc should close task board");
        assert!(!app.should_quit);

        // Third Esc quits
        app.handle_key(key_code(KeyCode::Esc));
        assert!(app.should_quit, "Final Esc should quit");
    }

    #[test]
    fn test_stats_tab_switching() {
        let mut app = test_app();
        app.input_mode = InputMode::StatsViewer;
        app.show_stats_viewer = true;
        assert_eq!(app.stats_viewer.active_tab, 0);

        app.handle_key(key_code(KeyCode::Tab));
        assert_eq!(app.stats_viewer.active_tab, 1, "Tab should advance tab");

        app.handle_key(key('3'));
        assert_eq!(app.stats_viewer.active_tab, 2, "'3' should jump to tab 3");
    }
}
