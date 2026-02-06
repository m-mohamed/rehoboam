//! Keyboard input handling for all modes
//!
//! Routes keyboard events to mode-specific handlers based on [`InputMode`].
//!
//! # Keyboard Shortcuts (Normal Mode)
//!
//! ## Navigation
//! - `h`/`←` - Move to left column
//! - `l`/`→` - Move to right column
//! - `j`/`↓` - Move to next card in column
//! - `k`/`↑` - Move to previous card in column
//! - `Enter` - Jump to selected agent's tmux pane
//! - `/` - Enter search mode
//!
//! ## View Controls
//! - `?`/`H` - Toggle help overlay
//! - `d` - Toggle progress dashboard
//! - `D` - Toggle diff view for selected agent
//! - `f` - Freeze/unfreeze display updates
//!
//! ## Agent Actions
//! - `y` - Approve permission request
//! - `n` - Reject permission request
//! - `c` - Enter custom input mode
//! - `Space` - Toggle agent selection (for bulk ops)
//! - `s` - Open spawn dialog
//!
//! ## Git Operations
//! - `g` - Git commit checkpoint
//! - `G` - Git push to remote
//!
//! ## Application
//! - `q` - Quit application
//! - `Esc` - Close overlays or quit
//! - `Ctrl+C` - Force quit

use super::{agent_control, navigation, operations, spawn, App, InputMode};
use crossterm::event::{KeyCode, KeyModifiers};

impl App {
    /// Handle keyboard input
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        // Handle Ctrl+C always
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        // Handle modal overlays first
        if self.show_checkpoint_timeline {
            self.handle_key_checkpoint_timeline(key);
            return;
        }

        // Route to mode-specific handlers
        match self.input_mode {
            InputMode::Normal => self.handle_key_normal(key),
            InputMode::Input => self.handle_key_input(key),
            InputMode::Spawn => self.handle_key_spawn(key),
            InputMode::Search => self.handle_key_search(key),
        }
    }

    /// Handle keyboard input in Normal mode
    fn handle_key_normal(&mut self, key: crossterm::event::KeyEvent) {
        // If diff modal is open, route to diff handler
        if self.show_diff {
            self.handle_key_diff(key);
            return;
        }

        match key.code {
            // Quit (but Esc first closes overlays like help)
            KeyCode::Char('q') => {
                self.should_quit = true;
            }
            KeyCode::Esc => {
                if self.show_help {
                    self.show_help = false;
                } else if self.show_dashboard {
                    self.show_dashboard = false;
                } else {
                    self.should_quit = true;
                }
            }
            // Column navigation (horizontal)
            KeyCode::Char('h') | KeyCode::Left => {
                self.state.move_column_left();
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.state.move_column_right();
            }
            // Agent navigation (vertical, flat across all columns)
            KeyCode::Char('j') | KeyCode::Down => {
                self.state.next_agent_flat();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.state.previous_agent_flat();
            }
            // Jump to agent
            KeyCode::Enter => {
                navigation::jump_to_selected(&self.state);
            }
            // Toggle help (use '?' since 'h' is now column navigation)
            KeyCode::Char('?' | 'H') => {
                self.show_help = !self.show_help;
            }
            // Toggle progress dashboard
            KeyCode::Char('d') => {
                self.show_dashboard = !self.show_dashboard;
                tracing::debug!(show_dashboard = self.show_dashboard, "Toggled dashboard");
            }
            // Toggle freeze mode (stops UI updates for stable selection)
            KeyCode::Char('f') => {
                self.frozen = !self.frozen;
            }

            // === Permission shortcuts ===
            KeyCode::Char('y') => {
                agent_control::approve_selected(&self.state, self.sprites_client.as_ref());
            }
            KeyCode::Char('n') => {
                agent_control::reject_selected(&self.state, self.sprites_client.as_ref());
            }

            // === Custom input injection ===
            KeyCode::Char('c') => {
                if self.state.selected_agent().is_some() {
                    self.input_mode = InputMode::Input;
                    self.input_buffer.clear();
                    tracing::debug!("Entering input mode");
                }
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

            // === Bulk operations ===
            KeyCode::Char(' ') => {
                self.state.toggle_selection();
                tracing::debug!(
                    selected_count = self.state.selected_agents.len(),
                    "Toggled agent selection"
                );
            }
            KeyCode::Char('Y') => {
                agent_control::bulk_approve(&mut self.state, self.sprites_client.as_ref());
            }
            KeyCode::Char('N') => {
                agent_control::bulk_reject(&mut self.state, self.sprites_client.as_ref());
            }
            KeyCode::Char('K') => {
                agent_control::bulk_kill(&mut self.state, self.sprites_client.as_ref());
            }
            KeyCode::Char('x') => {
                self.state.clear_selection();
                tracing::debug!("Cleared all selections");
            }

            // === Git Operations ===
            KeyCode::Char('g') => {
                operations::git_commit_selected(&self.state);
            }
            KeyCode::Char('p') => {
                operations::git_push_selected(&self.state);
            }
            KeyCode::Char('D') => {
                self.toggle_diff_view();
            }
            KeyCode::Char('t') => {
                self.toggle_checkpoint_timeline();
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

    /// Handle keyboard input in Input mode
    fn handle_key_input(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
                tracing::debug!("Cancelled input mode");
            }
            KeyCode::Enter => {
                agent_control::send_custom_input(
                    &self.state,
                    self.sprites_client.as_ref(),
                    &self.input_buffer,
                );
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
            }
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
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
                // Toggle fields (3 = worktree, 4 = claude_tasks, 6 = sprite)
                match self.spawn_state.active_field {
                    3 => self.spawn_state.use_worktree = !self.spawn_state.use_worktree,
                    4 => self.spawn_state.use_claude_tasks = !self.spawn_state.use_claude_tasks,
                    6 => self.spawn_state.use_sprite = !self.spawn_state.use_sprite,
                    _ => match spawn::validate_spawn(
                        &self.spawn_state,
                        self.sprites_client.is_some(),
                    ) {
                        Ok(()) => {
                            self.spawn_state.validation_error = None;
                            if let Some(err) = spawn::spawn_agent(
                                &self.spawn_state,
                                self.sprites_client.as_ref(),
                                &mut self.state,
                            ) {
                                self.show_status(&err);
                            }
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
                3 => self.spawn_state.use_worktree = !self.spawn_state.use_worktree,
                4 => self.spawn_state.use_claude_tasks = !self.spawn_state.use_claude_tasks,
                6 => self.spawn_state.use_sprite = !self.spawn_state.use_sprite,
                0 => {
                    if self.spawn_state.use_sprite {
                        self.spawn_state.github_repo.push(' ');
                    } else {
                        self.spawn_state.project_path.push(' ');
                    }
                }
                1 => self.spawn_state.prompt.push(' '),
                2 => self.spawn_state.branch_name.push(' '),
                5 => self.spawn_state.task_list_id.push(' '),
                _ => {}
            },
            KeyCode::Left => {
                if self.spawn_state.active_field == 7 {
                    self.spawn_state.network_preset = self.spawn_state.network_preset.prev();
                }
            }
            KeyCode::Right => {
                if self.spawn_state.active_field == 7 {
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
                2 => {
                    self.spawn_state.branch_name.pop();
                }
                5 => {
                    self.spawn_state.task_list_id.pop();
                }
                8 => {
                    self.spawn_state.ram_mb.pop();
                }
                9 => {
                    self.spawn_state.cpus.pop();
                }
                10 => {
                    self.spawn_state.clone_destination.pop();
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
                2 => self.spawn_state.branch_name.push(c),
                3 => {
                    if c == 'y' || c == 'Y' {
                        self.spawn_state.use_worktree = true;
                    } else if c == 'n' || c == 'N' {
                        self.spawn_state.use_worktree = false;
                    }
                }
                4 => {
                    // Claude Tasks toggle - y/n to toggle
                    if c == 'y' || c == 'Y' {
                        self.spawn_state.use_claude_tasks = true;
                    } else if c == 'n' || c == 'N' {
                        self.spawn_state.use_claude_tasks = false;
                    }
                }
                5 => {
                    // Task list ID - alphanumeric + dash/underscore
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        self.spawn_state.task_list_id.push(c);
                    }
                }
                6 => {
                    // Sprite toggle - y/n to toggle
                    if c == 'y' || c == 'Y' {
                        self.spawn_state.use_sprite = true;
                    } else if c == 'n' || c == 'N' {
                        self.spawn_state.use_sprite = false;
                    }
                }
                8 => {
                    if c.is_ascii_digit() {
                        self.spawn_state.ram_mb.push(c);
                    }
                }
                9 => {
                    if c.is_ascii_digit() {
                        self.spawn_state.cpus.push(c);
                    }
                }
                10 => {
                    self.spawn_state.clone_destination.push(c);
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

    /// Handle keyboard input in checkpoint timeline modal
    pub fn handle_key_checkpoint_timeline(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('t') => {
                self.show_checkpoint_timeline = false;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected_checkpoint > 0 {
                    self.selected_checkpoint -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.selected_checkpoint < self.checkpoint_timeline.len().saturating_sub(1) {
                    self.selected_checkpoint += 1;
                }
            }
            KeyCode::Enter => {
                self.restore_selected_checkpoint();
            }
            _ => {}
        }
    }

    /// Toggle diff view for selected agent
    fn toggle_diff_view(&mut self) {
        if self.show_diff {
            self.show_diff = false;
            return;
        }

        match operations::get_diff_content(&self.state) {
            Ok((raw, parsed)) => {
                self.diff_content = raw;
                self.parsed_diff = Some(parsed);
                self.diff_scroll = 0;
                self.diff_selected_file = 0;
                self.diff_collapsed_hunks.clear();
                self.show_diff = true;
            }
            Err(msg) => {
                self.show_status(&format!("Cannot show diff: {}", msg));
            }
        }
    }

    /// Handle keyboard input in diff modal
    fn handle_key_diff(&mut self, key: crossterm::event::KeyEvent) {
        let file_count = self
            .parsed_diff
            .as_ref()
            .map(|d| d.files.len())
            .unwrap_or(0);

        match key.code {
            // Close diff
            KeyCode::Esc | KeyCode::Char('D' | 'q') => {
                self.show_diff = false;
            }

            // Scroll up/down
            KeyCode::Char('j') | KeyCode::Down => {
                self.diff_scroll = self.diff_scroll.saturating_add(1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.diff_scroll = self.diff_scroll.saturating_sub(1);
            }

            // Page up/down
            KeyCode::PageDown | KeyCode::Char('d')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                self.diff_scroll = self.diff_scroll.saturating_add(20);
            }
            KeyCode::PageUp | KeyCode::Char('u')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                self.diff_scroll = self.diff_scroll.saturating_sub(20);
            }

            // Next/previous file
            KeyCode::Char('n') | KeyCode::Tab => {
                if file_count > 0 {
                    self.diff_selected_file = (self.diff_selected_file + 1) % file_count;
                    self.diff_selected_hunk = 0; // Reset hunk selection
                }
            }
            KeyCode::Char('p') | KeyCode::BackTab => {
                if file_count > 0 {
                    self.diff_selected_file =
                        (self.diff_selected_file + file_count - 1) % file_count;
                    self.diff_selected_hunk = 0; // Reset hunk selection
                }
            }

            // Next/previous hunk within current file
            KeyCode::Char(']') => {
                if let Some(diff) = &self.parsed_diff {
                    if let Some(file) = diff.files.get(self.diff_selected_file) {
                        let hunk_count = file.hunks.len();
                        if hunk_count > 0 {
                            self.diff_selected_hunk = (self.diff_selected_hunk + 1) % hunk_count;
                        }
                    }
                }
            }
            KeyCode::Char('[') => {
                if let Some(diff) = &self.parsed_diff {
                    if let Some(file) = diff.files.get(self.diff_selected_file) {
                        let hunk_count = file.hunks.len();
                        if hunk_count > 0 {
                            self.diff_selected_hunk =
                                (self.diff_selected_hunk + hunk_count - 1) % hunk_count;
                        }
                    }
                }
            }

            // Toggle hunk collapse
            KeyCode::Char('o') => {
                // Toggle collapse for current hunk at scroll position
                // For simplicity, we toggle based on file + first hunk
                let key = (self.diff_selected_file, 0);
                if self.diff_collapsed_hunks.contains(&key) {
                    self.diff_collapsed_hunks.remove(&key);
                } else {
                    self.diff_collapsed_hunks.insert(key);
                }
            }

            // Collapse/expand all hunks in current file
            KeyCode::Char('O') => {
                if let Some(diff) = &self.parsed_diff {
                    if let Some(file) = diff.files.get(self.diff_selected_file) {
                        let all_collapsed = (0..file.hunks.len()).all(|i| {
                            self.diff_collapsed_hunks
                                .contains(&(self.diff_selected_file, i))
                        });

                        for i in 0..file.hunks.len() {
                            let key = (self.diff_selected_file, i);
                            if all_collapsed {
                                self.diff_collapsed_hunks.remove(&key);
                            } else {
                                self.diff_collapsed_hunks.insert(key);
                            }
                        }
                    }
                }
            }

            // Git commit from diff view
            KeyCode::Char('g') => {
                operations::git_commit_selected(&self.state);
                self.show_status("Committed changes");
            }

            // Git push from diff view
            KeyCode::Char('G') => {
                operations::git_push_selected(&self.state);
                self.show_status("Pushed changes");
            }

            _ => {}
        }
    }

    /// Toggle checkpoint timeline modal
    fn toggle_checkpoint_timeline(&mut self) {
        if self.show_checkpoint_timeline {
            self.show_checkpoint_timeline = false;
            return;
        }

        if let Some(_sprite_id) = operations::fetch_checkpoints(
            &self.state,
            self.sprites_client.as_ref(),
            self.event_tx.as_ref(),
        ) {
            self.checkpoint_timeline = Vec::new();
            self.selected_checkpoint = 0;
            self.show_checkpoint_timeline = true;
        }
    }

    /// Restore sprite to selected checkpoint
    fn restore_selected_checkpoint(&mut self) {
        if self.checkpoint_timeline.is_empty() {
            tracing::warn!("No checkpoints to restore");
            return;
        }

        if let Some(checkpoint) = self.checkpoint_timeline.get(self.selected_checkpoint) {
            operations::restore_checkpoint(&self.state, checkpoint);
        }

        self.show_checkpoint_timeline = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ReconciliationConfig;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    /// Create a test App instance
    fn test_app() -> App {
        App::new(false, None, None, &ReconciliationConfig::default())
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
    fn test_freeze_toggle() {
        let mut app = test_app();
        assert!(!app.frozen);

        app.handle_key(key('f'));
        assert!(app.frozen, "'f' should freeze display");

        app.handle_key(key('f'));
        assert!(!app.frozen, "'f' should unfreeze display");
    }

    #[test]
    fn test_dashboard_toggle() {
        let mut app = test_app();
        assert!(!app.show_dashboard);

        app.handle_key(key('d'));
        assert!(app.show_dashboard, "'d' should toggle dashboard on");

        app.handle_key(key('d'));
        assert!(!app.show_dashboard, "'d' should toggle dashboard off");
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
    fn test_esc_closes_dashboard_first() {
        let mut app = test_app();
        app.show_dashboard = true;

        app.handle_key(key_code(KeyCode::Esc));
        assert!(!app.show_dashboard, "Esc should close dashboard");
        assert!(
            !app.should_quit,
            "Esc should not quit when dashboard is open"
        );
    }

    #[test]
    fn test_navigation_h_l_columns() {
        let mut app = test_app();
        let initial_col = app.state.selected_column;

        app.handle_key(key('l'));
        assert_eq!(
            app.state.selected_column,
            (initial_col + 1) % 3,
            "'l' should move right"
        );

        app.handle_key(key('h'));
        assert_eq!(
            app.state.selected_column, initial_col,
            "'h' should move left"
        );
    }

    #[test]
    fn test_diff_view_key_close() {
        let mut app = test_app();
        app.show_diff = true;

        app.handle_key(key_code(KeyCode::Esc));
        assert!(!app.show_diff, "Esc should close diff view");
    }

    #[test]
    fn test_diff_view_scroll() {
        let mut app = test_app();
        app.show_diff = true;
        app.diff_scroll = 5;

        app.handle_key(key('j'));
        assert_eq!(app.diff_scroll, 6, "'j' should scroll down in diff view");

        app.handle_key(key('k'));
        assert_eq!(app.diff_scroll, 5, "'k' should scroll up in diff view");
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
    fn test_input_mode_typing() {
        let mut app = test_app();
        app.input_mode = InputMode::Input;

        app.handle_key(key('h'));
        app.handle_key(key('i'));

        assert_eq!(
            app.input_buffer, "hi",
            "characters should be appended to input buffer"
        );
    }

    #[test]
    fn test_input_mode_backspace() {
        let mut app = test_app();
        app.input_mode = InputMode::Input;
        app.input_buffer = "hello".to_string();

        app.handle_key(key_code(KeyCode::Backspace));
        assert_eq!(
            app.input_buffer, "hell",
            "backspace should remove last char"
        );
    }

    #[test]
    fn test_input_mode_escape() {
        let mut app = test_app();
        app.input_mode = InputMode::Input;
        app.input_buffer = "test".to_string();

        app.handle_key(key_code(KeyCode::Esc));
        assert_eq!(
            app.input_mode,
            InputMode::Normal,
            "Esc should exit input mode"
        );
        assert!(app.input_buffer.is_empty(), "Esc should clear input buffer");
    }

    /// Create a test checkpoint record
    fn test_checkpoint() -> crate::sprite::CheckpointRecord {
        crate::sprite::CheckpointRecord {
            id: "test".to_string(),
            comment: "test checkpoint".to_string(),
            created_at: 0,
        }
    }

    #[test]
    fn test_checkpoint_timeline_navigation() {
        let mut app = test_app();
        app.show_checkpoint_timeline = true;
        app.checkpoint_timeline = vec![test_checkpoint(), test_checkpoint(), test_checkpoint()];
        app.selected_checkpoint = 1;

        app.handle_key(key('k'));
        assert_eq!(
            app.selected_checkpoint, 0,
            "'k' should select previous checkpoint"
        );

        app.handle_key(key('j'));
        assert_eq!(
            app.selected_checkpoint, 1,
            "'j' should select next checkpoint"
        );
    }

    #[test]
    fn test_checkpoint_timeline_close() {
        let mut app = test_app();
        app.show_checkpoint_timeline = true;

        app.handle_key(key_code(KeyCode::Esc));
        assert!(
            !app.show_checkpoint_timeline,
            "Esc should close checkpoint timeline"
        );
    }
}
