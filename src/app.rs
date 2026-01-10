use crate::event::Event;
use crate::git::GitController;
use crate::state::AppState;
use crate::tmux::TmuxController;
use crossterm::event::{KeyCode, KeyModifiers};
use std::path::PathBuf;
use std::process::Command;

/// Input mode for the application
#[derive(Debug, Clone, PartialEq, Default)]
pub enum InputMode {
    /// Normal navigation mode
    #[default]
    Normal,
    /// Text input mode (typing custom input for agent)
    Input,
    /// Spawn dialog mode (creating new agent)
    Spawn,
}

/// View mode for the main display
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ViewMode {
    /// Kanban-style columns by status (Attention, Working, Compact, Idle)
    #[default]
    Kanban,
    /// Grouped by project name
    Project,
}

/// State for the spawn dialog
#[derive(Debug, Clone, Default)]
pub struct SpawnState {
    /// Selected project path
    pub project_path: String,
    /// Prompt to send to the new agent
    pub prompt: String,
    /// Branch name for git worktree (optional)
    pub branch_name: String,
    /// Whether to create a git worktree for isolation
    pub use_worktree: bool,
    /// Which field is being edited (0 = project, 1 = prompt, 2 = branch, 3 = worktree toggle)
    pub active_field: usize,
}

/// Number of fields in spawn dialog
const SPAWN_FIELD_COUNT: usize = 4;

/// Application state and logic
pub struct App {
    pub state: AppState,
    pub should_quit: bool,
    pub debug_mode: bool,
    pub show_help: bool,
    /// Freeze display - stops UI updates but events still received
    pub frozen: bool,
    /// Dirty flag: true if UI needs re-render (render-on-change optimization)
    pub needs_render: bool,
    /// Auto-accept mode: automatically approve low-risk operations
    pub auto_accept: bool,
    /// Current input mode (Normal or Input)
    pub input_mode: InputMode,
    /// Text buffer for input mode
    pub input_buffer: String,
    /// Current view mode (Kanban or Project)
    pub view_mode: ViewMode,
    /// Spawn dialog state
    pub spawn_state: SpawnState,
}

impl App {
    pub fn new(debug_mode: bool) -> Self {
        Self {
            state: AppState::new(),
            should_quit: false,
            debug_mode,
            show_help: false,
            frozen: false,
            needs_render: true, // Always render first frame
            auto_accept: false, // Manual approval by default
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            view_mode: ViewMode::Kanban,
            spawn_state: SpawnState::default(),
        }
    }

    /// Handle incoming events
    pub fn handle_event(&mut self, event: Event) {
        match event {
            Event::Hook(hook_event) => {
                // Only process hook events if not frozen
                if !self.frozen {
                    self.state.process_event(*hook_event);
                    self.needs_render = true; // State changed
                }
            }
            Event::Key(key) => {
                self.handle_key(key);
                self.needs_render = true; // Any key press triggers render
            }
        }
    }

    /// Handle keyboard input
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        // Handle Ctrl+C always
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        // Route to mode-specific handlers
        match self.input_mode {
            InputMode::Normal => self.handle_key_normal(key),
            InputMode::Input => self.handle_key_input(key),
            InputMode::Spawn => self.handle_key_spawn(key),
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
                self.jump_to_selected();
            }
            // Toggle help (use '?' since 'h' is now column navigation)
            KeyCode::Char('?') | KeyCode::Char('H') => {
                self.show_help = !self.show_help;
            }
            // Toggle debug mode
            KeyCode::Char('d') => {
                self.debug_mode = !self.debug_mode;
            }
            // Toggle freeze mode (stops UI updates for stable selection)
            KeyCode::Char('f') => {
                self.frozen = !self.frozen;
            }

            // === Phase 1.1: Permission shortcuts ===
            // Approve permission (sends "y" + Enter to selected agent's pane)
            KeyCode::Char('y') => {
                self.approve_selected();
            }
            // Reject permission (sends "n" + Enter to selected agent's pane)
            KeyCode::Char('n') => {
                self.reject_selected();
            }

            // === Phase 1.2: Custom input injection ===
            // Open input dialog to send custom text to selected agent
            KeyCode::Char('c') => {
                if self.state.selected_agent().is_some() {
                    self.input_mode = InputMode::Input;
                    self.input_buffer.clear();
                    tracing::debug!("Entering input mode");
                }
            }

            // === Phase 2.0: View mode toggle ===
            // Toggle between Kanban (status columns) and Project (grouped by project) views
            KeyCode::Char('P') => {
                self.view_mode = match self.view_mode {
                    ViewMode::Kanban => ViewMode::Project,
                    ViewMode::Project => ViewMode::Kanban,
                };
                tracing::debug!(view_mode = ?self.view_mode, "Toggled view mode");
            }

            // === Phase 3.0: Agent spawning ===
            // Open spawn dialog to create new Claude agent
            KeyCode::Char('s') => {
                self.input_mode = InputMode::Spawn;
                self.spawn_state = SpawnState::default();
                // Pre-fill with current working directory
                if let Ok(cwd) = std::env::current_dir() {
                    self.spawn_state.project_path = cwd.display().to_string();
                }
                tracing::debug!("Entering spawn mode");
            }

            // === Phase 3.1: Auto-accept toggle ===
            KeyCode::Char('A') => {
                self.auto_accept = !self.auto_accept;
                tracing::info!(
                    auto_accept = self.auto_accept,
                    "Auto-accept mode {}",
                    if self.auto_accept { "enabled" } else { "disabled" }
                );
            }

            // === Phase 3.2: Bulk operations ===
            // Toggle selection of current agent
            KeyCode::Char(' ') => {
                self.state.toggle_selection();
                tracing::debug!(
                    selected_count = self.state.selected_agents.len(),
                    "Toggled agent selection"
                );
            }
            // Bulk approve all selected agents
            KeyCode::Char('Y') => {
                self.bulk_approve();
            }
            // Bulk reject all selected agents
            KeyCode::Char('N') => {
                self.bulk_reject();
            }
            // Kill selected agents (send Ctrl+C)
            KeyCode::Char('K') => {
                self.bulk_kill();
            }
            // Clear all selections
            KeyCode::Char('x') => {
                self.state.clear_selection();
                tracing::debug!("Cleared all selections");
            }

            _ => {}
        }
    }

    /// Handle keyboard input in Input mode
    fn handle_key_input(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            // Cancel and return to Normal mode
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
                tracing::debug!("Cancelled input mode");
            }
            // Send the input to selected agent
            KeyCode::Enter => {
                self.send_custom_input();
                self.input_mode = InputMode::Normal;
                self.input_buffer.clear();
            }
            // Delete last character
            KeyCode::Backspace => {
                self.input_buffer.pop();
            }
            // Type a character
            KeyCode::Char(c) => {
                self.input_buffer.push(c);
            }
            _ => {}
        }
    }

    /// Handle keyboard input in Spawn mode
    fn handle_key_spawn(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            // Cancel and return to Normal mode
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.spawn_state = SpawnState::default();
                tracing::debug!("Cancelled spawn mode");
            }
            // Navigate between fields
            KeyCode::Tab | KeyCode::Down => {
                self.spawn_state.active_field =
                    (self.spawn_state.active_field + 1) % SPAWN_FIELD_COUNT;
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.spawn_state.active_field =
                    (self.spawn_state.active_field + SPAWN_FIELD_COUNT - 1) % SPAWN_FIELD_COUNT;
            }
            // Spawn the agent (from any field)
            KeyCode::Enter => {
                // Field 3 is the worktree toggle - toggle it on Enter
                if self.spawn_state.active_field == 3 {
                    self.spawn_state.use_worktree = !self.spawn_state.use_worktree;
                } else {
                    // Spawn if we have a project path
                    if !self.spawn_state.project_path.is_empty() {
                        self.spawn_agent();
                        self.input_mode = InputMode::Normal;
                        self.spawn_state = SpawnState::default();
                    }
                }
            }
            // Toggle worktree with Space (when on that field)
            KeyCode::Char(' ') if self.spawn_state.active_field == 3 => {
                self.spawn_state.use_worktree = !self.spawn_state.use_worktree;
            }
            // Delete last character from active text field
            KeyCode::Backspace => match self.spawn_state.active_field {
                0 => {
                    self.spawn_state.project_path.pop();
                }
                1 => {
                    self.spawn_state.prompt.pop();
                }
                2 => {
                    self.spawn_state.branch_name.pop();
                }
                _ => {}
            },
            // Type a character into active text field
            KeyCode::Char(c) => match self.spawn_state.active_field {
                0 => self.spawn_state.project_path.push(c),
                1 => self.spawn_state.prompt.push(c),
                2 => self.spawn_state.branch_name.push(c),
                3 => {
                    // Toggle worktree on 'y' or 'n'
                    if c == 'y' || c == 'Y' {
                        self.spawn_state.use_worktree = true;
                    } else if c == 'n' || c == 'N' {
                        self.spawn_state.use_worktree = false;
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    /// Spawn a new Claude agent in a tmux pane
    ///
    /// If `use_worktree` is enabled and `branch_name` is set, creates an
    /// isolated git worktree for the agent to work in.
    fn spawn_agent(&self) {
        if self.spawn_state.project_path.is_empty() {
            tracing::warn!("Cannot spawn: no project path specified");
            return;
        }

        let project_path = &self.spawn_state.project_path;
        let prompt = &self.spawn_state.prompt;
        let use_worktree = self.spawn_state.use_worktree;
        let branch_name = &self.spawn_state.branch_name;

        // Determine working directory (worktree or project)
        let working_dir: PathBuf = if use_worktree && !branch_name.is_empty() {
            let git = GitController::new(PathBuf::from(project_path));

            if !git.is_git_repo() {
                tracing::warn!(
                    project = %project_path,
                    "Cannot create worktree: not a git repository"
                );
                PathBuf::from(project_path)
            } else {
                match git.create_worktree(branch_name) {
                    Ok(worktree_path) => {
                        tracing::info!(
                            branch = %branch_name,
                            path = %worktree_path.display(),
                            "Created isolated worktree for agent"
                        );
                        worktree_path
                    }
                    Err(e) => {
                        tracing::error!(
                            error = %e,
                            branch = %branch_name,
                            "Failed to create worktree, using project path"
                        );
                        PathBuf::from(project_path)
                    }
                }
            }
        } else {
            PathBuf::from(project_path)
        };

        tracing::info!(
            project = %project_path,
            working_dir = %working_dir.display(),
            use_worktree = use_worktree,
            prompt_len = prompt.len(),
            "Spawning new Claude agent"
        );

        // Create new tmux pane in the working directory
        let working_dir_str = working_dir.to_string_lossy().to_string();
        match TmuxController::split_pane(true, &working_dir_str) {
            Ok(pane_id) => {
                tracing::info!(pane_id = %pane_id, "Created new tmux pane");

                // Start Claude Code in the new pane
                if let Err(e) = TmuxController::send_keys(&pane_id, "claude") {
                    tracing::error!(error = %e, "Failed to start Claude");
                    return;
                }

                // If we have a prompt, send it after a short delay
                // (Claude needs time to start up)
                if !prompt.is_empty() {
                    // Use tokio::spawn for async-friendly delay
                    let pane_id_clone = pane_id.clone();
                    let prompt_clone = prompt.clone();
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        if let Err(e) = TmuxController::send_buffered(&pane_id_clone, &prompt_clone)
                        {
                            tracing::error!(error = %e, "Failed to send prompt");
                        } else {
                            tracing::info!(pane_id = %pane_id_clone, "Sent initial prompt");
                        }
                    });
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to create tmux pane");
            }
        }
    }

    /// Send custom input to selected agent's pane
    fn send_custom_input(&self) {
        if self.input_buffer.is_empty() {
            return;
        }

        if let Some(agent) = self.state.selected_agent() {
            let pane_id = &agent.pane_id;

            // Only tmux panes are supported for now
            if !pane_id.starts_with('%') {
                tracing::warn!(
                    pane_id = %pane_id,
                    "Cannot send input: not a tmux pane"
                );
                return;
            }

            tracing::info!(
                pane_id = %pane_id,
                project = %agent.project,
                input_len = self.input_buffer.len(),
                "Sending custom input"
            );

            // Use buffered send for multi-line or long input, simple send for short
            let result = if self.input_buffer.contains('\n') || self.input_buffer.len() > 100 {
                TmuxController::send_buffered(pane_id, &self.input_buffer)
            } else {
                TmuxController::send_keys(pane_id, &self.input_buffer)
            };

            if let Err(e) = result {
                tracing::error!(
                    pane_id = %pane_id,
                    error = %e,
                    "Failed to send custom input"
                );
            }
        }
    }

    /// Tick for triggering re-renders
    ///
    /// Best practice from ratatui async-template:
    /// - Events update state (hook events push activity data)
    /// - Ticks trigger re-render only (no new data)
    ///
    /// This ensures sparkline consistency - activity values only come
    /// from real hook events, not synthesized tick data.
    pub fn tick(&mut self) {
        // Process timeout-based state transitions
        self.state.tick();
        // Tick triggers re-render for elapsed time updates
        self.needs_render = true;
    }

    /// Called after render to reset dirty flag
    pub fn rendered(&mut self) {
        self.needs_render = false;
    }

    /// Jump to selected agent using terminal-appropriate CLI
    ///
    /// Detects terminal type from pane_id format:
    /// - Tmux: %0, %1, etc. (starts with %)
    /// - WezTerm: numeric ID
    fn jump_to_selected(&self) {
        if let Some(agent) = self.state.selected_agent() {
            let pane_id = &agent.pane_id;
            tracing::debug!("Jumping to pane {}", pane_id);

            let result = if pane_id.starts_with('%') {
                // Tmux pane format: %0, %1, etc.
                Command::new("tmux")
                    .args(["select-pane", "-t", pane_id])
                    .output()
            } else {
                // WezTerm pane (numeric ID)
                Command::new("wezterm")
                    .args(["cli", "activate-pane", "--pane-id", pane_id])
                    .output()
            };

            match result {
                Ok(output) if !output.status.success() => {
                    tracing::warn!(
                        "Failed to activate pane: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                Err(e) => {
                    tracing::error!("Failed to run CLI: {}", e);
                }
                _ => {}
            }
        }
    }

    /// Approve permission request for selected agent
    ///
    /// Sends "y" + Enter to the selected agent's tmux pane.
    /// Only works for tmux panes (pane_id starts with %).
    fn approve_selected(&self) {
        if let Some(agent) = self.state.selected_agent() {
            let pane_id = &agent.pane_id;

            // Only tmux panes are supported for now
            if !pane_id.starts_with('%') {
                tracing::warn!(
                    pane_id = %pane_id,
                    "Cannot approve: not a tmux pane"
                );
                return;
            }

            tracing::info!(
                pane_id = %pane_id,
                project = %agent.project,
                "Approving permission request"
            );

            if let Err(e) = TmuxController::send_keys(pane_id, "y") {
                tracing::error!(
                    pane_id = %pane_id,
                    error = %e,
                    "Failed to send approval"
                );
            }
        }
    }

    /// Reject permission request for selected agent
    ///
    /// Sends "n" + Enter to the selected agent's tmux pane.
    fn reject_selected(&self) {
        if let Some(agent) = self.state.selected_agent() {
            let pane_id = &agent.pane_id;

            if !pane_id.starts_with('%') {
                tracing::warn!(
                    pane_id = %pane_id,
                    "Cannot reject: not a tmux pane"
                );
                return;
            }

            tracing::info!(
                pane_id = %pane_id,
                project = %agent.project,
                "Rejecting permission request"
            );

            if let Err(e) = TmuxController::send_keys(pane_id, "n") {
                tracing::error!(
                    pane_id = %pane_id,
                    error = %e,
                    "Failed to send rejection"
                );
            }
        }
    }

    /// Bulk approve all selected agents
    fn bulk_approve(&mut self) {
        let panes = self.state.selected_tmux_panes();
        if panes.is_empty() {
            tracing::warn!("No agents selected for bulk approval");
            return;
        }

        tracing::info!(count = panes.len(), "Bulk approving agents");

        for pane_id in &panes {
            if let Err(e) = TmuxController::send_keys(pane_id, "y") {
                tracing::error!(pane_id = %pane_id, error = %e, "Failed to approve");
            }
        }

        self.state.clear_selection();
    }

    /// Bulk reject all selected agents
    fn bulk_reject(&mut self) {
        let panes = self.state.selected_tmux_panes();
        if panes.is_empty() {
            tracing::warn!("No agents selected for bulk rejection");
            return;
        }

        tracing::info!(count = panes.len(), "Bulk rejecting agents");

        for pane_id in &panes {
            if let Err(e) = TmuxController::send_keys(pane_id, "n") {
                tracing::error!(pane_id = %pane_id, error = %e, "Failed to reject");
            }
        }

        self.state.clear_selection();
    }

    /// Bulk kill all selected agents (send Ctrl+C)
    fn bulk_kill(&mut self) {
        let panes = self.state.selected_tmux_panes();
        if panes.is_empty() {
            tracing::warn!("No agents selected for kill");
            return;
        }

        tracing::info!(count = panes.len(), "Bulk killing agents");

        for pane_id in &panes {
            // Send Ctrl+C (C-c in tmux)
            if let Err(e) = TmuxController::send_keys_raw(pane_id, "C-c") {
                tracing::error!(pane_id = %pane_id, error = %e, "Failed to kill");
            }
        }

        self.state.clear_selection();
    }
}
