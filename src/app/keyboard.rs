//! Keyboard input handling for all modes

use super::{agent_control, navigation, operations, spawn, App, InputMode, ViewMode};
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
        match key.code {
            // Quit
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            // Column navigation (horizontal)
            KeyCode::Char('h') | KeyCode::Left => {
                self.state.move_column_left();
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.state.move_column_right();
            }
            // Card navigation (vertical within column)
            KeyCode::Char('j') | KeyCode::Down => {
                self.state.next_card();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.state.previous_card();
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

            // === View mode toggle ===
            KeyCode::Char('P') => {
                self.view_mode = match self.view_mode {
                    ViewMode::Kanban => ViewMode::Project,
                    ViewMode::Project => ViewMode::Split,
                    ViewMode::Split => ViewMode::Kanban,
                };
                // Capture output when entering split view
                if self.view_mode == ViewMode::Split {
                    self.live_output = navigation::capture_selected_output(&self.state);
                    self.output_scroll = 0;
                }
                tracing::debug!(view_mode = ?self.view_mode, "Toggled view mode");
            }

            // Toggle subagent tree panel
            KeyCode::Char('T') => {
                self.show_subagents = !self.show_subagents;
                tracing::debug!(show_subagents = self.show_subagents, "Toggled subagent panel");
            }

            // Scroll output (in split view)
            KeyCode::PageUp => {
                if self.view_mode == ViewMode::Split {
                    self.output_scroll = self.output_scroll.saturating_add(10);
                    self.needs_render = true;
                }
            }
            KeyCode::PageDown => {
                if self.view_mode == ViewMode::Split {
                    self.output_scroll = self.output_scroll.saturating_sub(10);
                    self.needs_render = true;
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

            // === Auto-accept toggle ===
            KeyCode::Char('A') => {
                self.auto_accept = !self.auto_accept;
                tracing::info!(
                    auto_accept = self.auto_accept,
                    "Auto-accept mode {}",
                    if self.auto_accept { "enabled" } else { "disabled" }
                );
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

            // === Loop Mode Controls ===
            KeyCode::Char('X') => {
                if let Some(pane_id) = self.state.selected_pane_id() {
                    self.state.cancel_loop(&pane_id);
                }
            }
            KeyCode::Char('R') => {
                if let Some(pane_id) = self.state.selected_pane_id() {
                    self.state.restart_loop(&pane_id);
                }
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
                agent_control::send_custom_input(&self.state, &self.input_buffer);
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
                self.spawn_state.active_field = (self.spawn_state.active_field
                    + spawn::SPAWN_FIELD_COUNT
                    - 1)
                    % spawn::SPAWN_FIELD_COUNT;
            }
            KeyCode::Enter => {
                // Toggle fields (3 = worktree, 4 = loop mode, 7 = sprite)
                match self.spawn_state.active_field {
                    3 => self.spawn_state.use_worktree = !self.spawn_state.use_worktree,
                    4 => self.spawn_state.loop_enabled = !self.spawn_state.loop_enabled,
                    7 => self.spawn_state.use_sprite = !self.spawn_state.use_sprite,
                    _ => {
                        match spawn::validate_spawn(&self.spawn_state) {
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
                        }
                    }
                }
            }
            KeyCode::Char(' ') => match self.spawn_state.active_field {
                3 => self.spawn_state.use_worktree = !self.spawn_state.use_worktree,
                4 => self.spawn_state.loop_enabled = !self.spawn_state.loop_enabled,
                7 => self.spawn_state.use_sprite = !self.spawn_state.use_sprite,
                0 => {
                    if self.spawn_state.use_sprite {
                        self.spawn_state.github_repo.push(' ');
                    } else {
                        self.spawn_state.project_path.push(' ');
                    }
                }
                1 => self.spawn_state.prompt.push(' '),
                2 => self.spawn_state.branch_name.push(' '),
                6 => self.spawn_state.loop_stop_word.push(' '),
                _ => {}
            },
            KeyCode::Left => {
                if self.spawn_state.active_field == 8 {
                    self.spawn_state.network_preset = self.spawn_state.network_preset.prev();
                }
            }
            KeyCode::Right => {
                if self.spawn_state.active_field == 8 {
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
                    self.spawn_state.loop_max_iterations.pop();
                }
                6 => {
                    self.spawn_state.loop_stop_word.pop();
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
                    if c == 'y' || c == 'Y' {
                        self.spawn_state.loop_enabled = true;
                    } else if c == 'n' || c == 'N' {
                        self.spawn_state.loop_enabled = false;
                    }
                }
                7 => {
                    if c == 'y' || c == 'Y' {
                        self.spawn_state.use_sprite = true;
                    } else if c == 'n' || c == 'N' {
                        self.spawn_state.use_sprite = false;
                    }
                }
                5 => {
                    if c.is_ascii_digit() {
                        self.spawn_state.loop_max_iterations.push(c);
                    }
                }
                6 => self.spawn_state.loop_stop_word.push(c),
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

        if let Some(content) = operations::get_diff_content(&self.state) {
            self.diff_content = content;
            self.show_diff = true;
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
