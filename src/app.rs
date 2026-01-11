use crate::config::RehoboamConfig;
use crate::event::Event;
use crate::git::GitController;
use crate::sprite::controller::SpriteController;
use crate::sprite::CheckpointRecord;
use crate::state::AppState;
use crate::tmux::TmuxController;
use crossterm::event::{KeyCode, KeyModifiers};
use sprites::SpritesClient;
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
#[derive(Debug, Clone)]
pub struct SpawnState {
    /// Selected project path
    pub project_path: String,
    /// Prompt to send to the new agent
    pub prompt: String,
    /// Branch name for git worktree (optional)
    pub branch_name: String,
    /// Whether to create a git worktree for isolation
    pub use_worktree: bool,
    /// Which field is being edited
    /// 0 = project, 1 = prompt, 2 = branch, 3 = worktree toggle, 4 = loop toggle,
    /// 5 = max iter, 6 = stop word, 7 = sprite toggle, 8 = network policy
    pub active_field: usize,
    // v0.9.0 Loop Mode fields
    /// Whether to enable loop mode for the new agent
    pub loop_enabled: bool,
    /// Maximum iterations before stopping (default: 50)
    pub loop_max_iterations: String,
    /// Stop word to detect completion (default: "DONE")
    pub loop_stop_word: String,
    /// Whether to spawn on a remote sprite (cloud VM)
    pub use_sprite: bool,
    /// Network policy for sprite (only applies when use_sprite is true)
    pub network_preset: crate::sprite::config::NetworkPreset,
}

impl Default for SpawnState {
    fn default() -> Self {
        Self {
            project_path: String::new(),
            prompt: String::new(),
            branch_name: String::new(),
            use_worktree: false,
            active_field: 0,
            loop_enabled: false,
            loop_max_iterations: "50".to_string(),
            loop_stop_word: "DONE".to_string(),
            use_sprite: false,
            network_preset: crate::sprite::config::NetworkPreset::ClaudeOnly,
        }
    }
}

/// Number of fields in spawn dialog
/// 0=project, 1=prompt, 2=branch, 3=worktree, 4=loop, 5=max_iter, 6=stop_word, 7=sprite, 8=network
const SPAWN_FIELD_COUNT: usize = 9;

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
    /// Application configuration
    pub config: RehoboamConfig,
    /// Sprites API client (None if sprites not enabled)
    pub sprites_client: Option<SpritesClient>,
    /// Show diff modal
    pub show_diff: bool,
    /// Diff content to display
    pub diff_content: String,
    /// Show checkpoint timeline modal
    pub show_checkpoint_timeline: bool,
    /// Checkpoint history for timeline display
    pub checkpoint_timeline: Vec<CheckpointRecord>,
    /// Selected checkpoint index in timeline
    pub selected_checkpoint: usize,
}

impl App {
    pub fn new(debug_mode: bool, config: RehoboamConfig, sprites_client: Option<SpritesClient>) -> Self {
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
            config,
            sprites_client,
            show_diff: false,
            diff_content: String::new(),
            show_checkpoint_timeline: false,
            checkpoint_timeline: Vec::new(),
            selected_checkpoint: 0,
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
            Event::RemoteHook { sprite_id, event } => {
                // Process remote hook events from sprites
                if !self.frozen {
                    // Mark the event as coming from a sprite
                    let mut hook_event = *event;
                    hook_event.source = crate::event::EventSource::Sprite {
                        sprite_id: sprite_id.clone(),
                    };
                    self.state.process_event(hook_event);
                    self.needs_render = true;
                }
            }
            Event::SpriteStatus { sprite_id, status } => {
                // Handle sprite status changes (connected/disconnected/destroyed)
                use crate::event::SpriteStatusType;
                match status {
                    SpriteStatusType::Connected => {
                        tracing::info!("Sprite connected: {}", sprite_id);
                        self.state.sprite_connected(&sprite_id);
                    }
                    SpriteStatusType::Disconnected | SpriteStatusType::Destroyed => {
                        tracing::info!("Sprite disconnected: {}", sprite_id);
                        self.state.sprite_disconnected(&sprite_id);
                    }
                }
                self.needs_render = true;
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
            KeyCode::Char('?' | 'H') => {
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
                    if self.auto_accept {
                        "enabled"
                    } else {
                        "disabled"
                    }
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

            // === v0.9.0 Loop Mode Controls ===
            // Cancel loop on selected agent (X key)
            KeyCode::Char('X') => {
                if let Some(pane_id) = self.state.selected_pane_id() {
                    self.state.cancel_loop(&pane_id);
                }
            }
            // Restart loop on selected agent (R key)
            KeyCode::Char('R') => {
                if let Some(pane_id) = self.state.selected_pane_id() {
                    self.state.restart_loop(&pane_id);
                }
            }

            // === v1.0 Git Operations ===
            // Git commit (stage all + commit)
            KeyCode::Char('g') => {
                self.git_commit_selected();
            }
            // Git push
            KeyCode::Char('p') => {
                self.git_push_selected();
            }
            // Git diff viewer
            KeyCode::Char('D') => {
                self.toggle_diff_view();
            }
            // Checkpoint timeline (sprites only)
            KeyCode::Char('t') => {
                self.toggle_checkpoint_timeline();
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
                // Toggle fields (3 = worktree, 4 = loop mode, 7 = sprite)
                match self.spawn_state.active_field {
                    3 => self.spawn_state.use_worktree = !self.spawn_state.use_worktree,
                    4 => self.spawn_state.loop_enabled = !self.spawn_state.loop_enabled,
                    7 => self.spawn_state.use_sprite = !self.spawn_state.use_sprite,
                    _ => {
                        // Spawn if we have a project path
                        if !self.spawn_state.project_path.is_empty() {
                            self.spawn_agent();
                            self.input_mode = InputMode::Normal;
                            self.spawn_state = SpawnState::default();
                        }
                    }
                }
            }
            // Toggle with Space (when on toggle fields)
            KeyCode::Char(' ') => match self.spawn_state.active_field {
                3 => self.spawn_state.use_worktree = !self.spawn_state.use_worktree,
                4 => self.spawn_state.loop_enabled = !self.spawn_state.loop_enabled,
                7 => self.spawn_state.use_sprite = !self.spawn_state.use_sprite,
                _ => {}
            },
            // Cycle network policy with left/right arrows
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
                5 => {
                    self.spawn_state.loop_max_iterations.pop();
                }
                6 => {
                    self.spawn_state.loop_stop_word.pop();
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
                4 => {
                    // Toggle loop mode on 'y' or 'n'
                    if c == 'y' || c == 'Y' {
                        self.spawn_state.loop_enabled = true;
                    } else if c == 'n' || c == 'N' {
                        self.spawn_state.loop_enabled = false;
                    }
                }
                7 => {
                    // Toggle sprite mode on 'y' or 'n'
                    if c == 'y' || c == 'Y' {
                        self.spawn_state.use_sprite = true;
                    } else if c == 'n' || c == 'N' {
                        self.spawn_state.use_sprite = false;
                    }
                }
                5 => {
                    // Only allow digits for max iterations
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

    /// Spawn a new Claude agent in a tmux pane
    ///
    /// If `use_worktree` is enabled and `branch_name` is set, creates an
    /// isolated git worktree for the agent to work in.
    /// If `loop_enabled` is set, registers a loop config to be applied when
    /// the agent sends its first hook event.
    fn spawn_agent(&mut self) {
        if self.spawn_state.project_path.is_empty() {
            tracing::warn!("Cannot spawn: no project path specified");
            return;
        }

        let project_path = &self.spawn_state.project_path;
        let prompt = &self.spawn_state.prompt;
        let use_worktree = self.spawn_state.use_worktree;
        let branch_name = &self.spawn_state.branch_name;

        // Branch: Sprite spawning vs tmux spawning
        if self.spawn_state.use_sprite {
            if let Some(client) = &self.sprites_client {
                tracing::info!(
                    project = %project_path,
                    prompt_len = prompt.len(),
                    loop_enabled = self.spawn_state.loop_enabled,
                    region = %self.config.sprites.default_region,
                    "Spawning agent on remote sprite"
                );

                // Create sprite asynchronously
                let client = client.clone();
                let project = project_path.clone();
                let prompt_clone = prompt.clone();
                let loop_enabled = self.spawn_state.loop_enabled;
                let max_iter = self
                    .spawn_state
                    .loop_max_iterations
                    .parse::<u32>()
                    .unwrap_or(50);
                let stop_word = self.spawn_state.loop_stop_word.clone();

                tokio::spawn(async move {
                    // Generate sprite name from project
                    let sprite_name = format!(
                        "rehoboam-{}",
                        project.replace(['/', '.'], "-")
                    );

                    tracing::info!(sprite_name = %sprite_name, "Creating sprite...");

                    match client.create(&sprite_name).await {
                        Ok(sprite) => {
                            tracing::info!(sprite_name = %sprite_name, "Sprite created");

                            // Start Claude Code in the sprite
                            let claude_cmd = if prompt_clone.is_empty() {
                                "claude".to_string()
                            } else {
                                format!("claude '{}'", prompt_clone.replace('\'', "'\\''"))
                            };

                            match sprite
                                .command("bash")
                                .arg("-c")
                                .arg(&claude_cmd)
                                .current_dir("/workspace")
                                .spawn()
                                .await
                            {
                                Ok(_) => {
                                    tracing::info!(
                                        sprite_name = %sprite_name,
                                        loop_enabled = loop_enabled,
                                        "Claude Code started in sprite"
                                    );
                                    if loop_enabled {
                                        tracing::debug!(
                                            max_iter = max_iter,
                                            stop_word = %stop_word,
                                            "Loop mode enabled for sprite"
                                        );
                                    }
                                }
                                Err(e) => {
                                    tracing::error!(
                                        error = %e,
                                        sprite_name = %sprite_name,
                                        "Failed to start Claude in sprite"
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                error = %e,
                                sprite_name = %sprite_name,
                                "Failed to create sprite"
                            );
                        }
                    }
                });

                return;
            }
            tracing::warn!(
                "Sprite mode requested but no sprites token configured. \
                 Set SPRITES_TOKEN or use --sprites-token. Falling back to tmux."
            );
            // Fall through to tmux spawning
        }

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

                // Register loop config if loop mode is enabled
                if self.spawn_state.loop_enabled {
                    let max_iter = self
                        .spawn_state
                        .loop_max_iterations
                        .parse::<u32>()
                        .unwrap_or(50);
                    self.state.register_loop_config(
                        &pane_id,
                        max_iter,
                        &self.spawn_state.loop_stop_word,
                    );
                }

                // Store working directory for git operations
                self.state
                    .set_agent_working_dir(&pane_id, working_dir.clone());

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

    /// Send custom input to selected agent
    ///
    /// Mode-aware: Uses TmuxController for local agents,
    /// logs for sprite agents (async not yet wired).
    fn send_custom_input(&self) {
        if self.input_buffer.is_empty() {
            return;
        }

        if let Some(agent) = self.state.selected_agent() {
            let pane_id = &agent.pane_id;

            tracing::info!(
                pane_id = %pane_id,
                project = %agent.project,
                input_len = self.input_buffer.len(),
                is_sprite = agent.is_sprite,
                "Sending custom input"
            );

            if agent.is_sprite {
                // Sprite agents: input would go through SpriteController (async)
                tracing::info!(
                    pane_id = %pane_id,
                    "Sprite input queued (async via SpriteController)"
                );
                // TODO: Wire async sprite input through event system
            } else if pane_id.starts_with('%') {
                // Tmux panes: send directly
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
            } else {
                tracing::warn!(
                    pane_id = %pane_id,
                    "Cannot send input: unknown pane type"
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
    /// Mode-aware: Uses TmuxController for local tmux agents,
    /// SpriteController for sprite agents (async).
    fn approve_selected(&self) {
        if let Some(agent) = self.state.selected_agent() {
            let pane_id = &agent.pane_id;

            tracing::info!(
                pane_id = %pane_id,
                project = %agent.project,
                is_sprite = agent.is_sprite,
                "Approving permission request"
            );

            if agent.is_sprite {
                // Sprite agents: approval goes through SpriteController (async)
                if let Some(client) = &self.sprites_client {
                    let sprite = client.sprite(pane_id);
                    tokio::spawn(async move {
                        if let Err(e) = SpriteController::approve(&sprite).await {
                            tracing::error!(error = %e, "Sprite approval failed");
                        }
                    });
                } else {
                    tracing::warn!(
                        pane_id = %pane_id,
                        "Cannot approve sprite: sprites client not configured"
                    );
                }
            } else if pane_id.starts_with('%') {
                // Tmux panes: send directly
                if let Err(e) = TmuxController::send_keys(pane_id, "y") {
                    tracing::error!(
                        pane_id = %pane_id,
                        error = %e,
                        "Failed to send approval"
                    );
                }
            } else {
                tracing::warn!(
                    pane_id = %pane_id,
                    "Cannot approve: unknown pane type"
                );
            }
        }
    }

    /// Reject permission request for selected agent
    ///
    /// Mode-aware: Uses TmuxController for local tmux agents,
    /// SpriteController for sprite agents (async).
    fn reject_selected(&self) {
        if let Some(agent) = self.state.selected_agent() {
            let pane_id = &agent.pane_id;

            tracing::info!(
                pane_id = %pane_id,
                project = %agent.project,
                is_sprite = agent.is_sprite,
                "Rejecting permission request"
            );

            if agent.is_sprite {
                // Sprite agents: rejection goes through SpriteController (async)
                if let Some(client) = &self.sprites_client {
                    let sprite = client.sprite(pane_id);
                    tokio::spawn(async move {
                        if let Err(e) = SpriteController::reject(&sprite).await {
                            tracing::error!(error = %e, "Sprite rejection failed");
                        }
                    });
                } else {
                    tracing::warn!(
                        pane_id = %pane_id,
                        "Cannot reject sprite: sprites client not configured"
                    );
                }
            } else if pane_id.starts_with('%') {
                // Tmux panes: send directly
                if let Err(e) = TmuxController::send_keys(pane_id, "n") {
                    tracing::error!(
                        pane_id = %pane_id,
                        error = %e,
                        "Failed to send rejection"
                    );
                }
            } else {
                tracing::warn!(
                    pane_id = %pane_id,
                    "Cannot reject: unknown pane type"
                );
            }
        }
    }

    /// Bulk approve all selected agents
    ///
    /// Mode-aware: Handles both tmux and sprite agents.
    fn bulk_approve(&mut self) {
        let tmux_panes = self.state.selected_tmux_panes();
        let sprite_agents = self.state.selected_sprite_agents();

        if tmux_panes.is_empty() && sprite_agents.is_empty() {
            tracing::warn!("No agents selected for bulk approval");
            return;
        }

        // Handle tmux agents
        if !tmux_panes.is_empty() {
            tracing::info!(count = tmux_panes.len(), "Bulk approving tmux agents");
            for pane_id in &tmux_panes {
                if let Err(e) = TmuxController::send_keys(pane_id, "y") {
                    tracing::error!(pane_id = %pane_id, error = %e, "Failed to approve");
                }
            }
        }

        // Handle sprite agents (async)
        if !sprite_agents.is_empty() {
            if let Some(client) = &self.sprites_client {
                tracing::info!(count = sprite_agents.len(), "Bulk approving sprite agents");
                for sprite_id in sprite_agents {
                    let sprite = client.sprite(&sprite_id);
                    tokio::spawn(async move {
                        if let Err(e) = SpriteController::approve(&sprite).await {
                            tracing::error!(sprite_id = %sprite_id, error = %e, "Sprite approval failed");
                        }
                    });
                }
            } else {
                tracing::warn!(
                    count = sprite_agents.len(),
                    "Cannot approve sprites: sprites client not configured"
                );
            }
        }

        self.state.clear_selection();
    }

    /// Bulk reject all selected agents
    ///
    /// Mode-aware: Handles both tmux and sprite agents.
    fn bulk_reject(&mut self) {
        let tmux_panes = self.state.selected_tmux_panes();
        let sprite_agents = self.state.selected_sprite_agents();

        if tmux_panes.is_empty() && sprite_agents.is_empty() {
            tracing::warn!("No agents selected for bulk rejection");
            return;
        }

        // Handle tmux agents
        if !tmux_panes.is_empty() {
            tracing::info!(count = tmux_panes.len(), "Bulk rejecting tmux agents");
            for pane_id in &tmux_panes {
                if let Err(e) = TmuxController::send_keys(pane_id, "n") {
                    tracing::error!(pane_id = %pane_id, error = %e, "Failed to reject");
                }
            }
        }

        // Handle sprite agents (async)
        if !sprite_agents.is_empty() {
            if let Some(client) = &self.sprites_client {
                tracing::info!(count = sprite_agents.len(), "Bulk rejecting sprite agents");
                for sprite_id in sprite_agents {
                    let sprite = client.sprite(&sprite_id);
                    tokio::spawn(async move {
                        if let Err(e) = SpriteController::reject(&sprite).await {
                            tracing::error!(sprite_id = %sprite_id, error = %e, "Sprite rejection failed");
                        }
                    });
                }
            } else {
                tracing::warn!(
                    count = sprite_agents.len(),
                    "Cannot reject sprites: sprites client not configured"
                );
            }
        }

        self.state.clear_selection();
    }

    /// Bulk kill all selected agents (send Ctrl+C)
    ///
    /// Mode-aware: Handles both tmux and sprite agents.
    fn bulk_kill(&mut self) {
        let tmux_panes = self.state.selected_tmux_panes();
        let sprite_agents = self.state.selected_sprite_agents();

        if tmux_panes.is_empty() && sprite_agents.is_empty() {
            tracing::warn!("No agents selected for kill");
            return;
        }

        // Handle tmux agents
        if !tmux_panes.is_empty() {
            tracing::info!(count = tmux_panes.len(), "Bulk killing tmux agents");
            for pane_id in &tmux_panes {
                // Send Ctrl+C (C-c in tmux)
                if let Err(e) = TmuxController::send_keys_raw(pane_id, "C-c") {
                    tracing::error!(pane_id = %pane_id, error = %e, "Failed to kill");
                }
            }
        }

        // Handle sprite agents (async)
        if !sprite_agents.is_empty() {
            if let Some(client) = &self.sprites_client {
                tracing::info!(count = sprite_agents.len(), "Bulk killing sprite agents");
                for sprite_id in sprite_agents {
                    let sprite = client.sprite(&sprite_id);
                    tokio::spawn(async move {
                        if let Err(e) = SpriteController::kill(&sprite).await {
                            tracing::error!(sprite_id = %sprite_id, error = %e, "Sprite kill failed");
                        }
                    });
                }
            } else {
                tracing::warn!(
                    count = sprite_agents.len(),
                    "Cannot kill sprites: sprites client not configured"
                );
            }
        }

        self.state.clear_selection();
    }

    // === v1.0 Git Operations ===

    /// Git commit on selected agent's worktree
    ///
    /// Stages all changes and creates a checkpoint commit.
    fn git_commit_selected(&mut self) {
        let Some(agent) = self.state.selected_agent() else {
            tracing::warn!("No agent selected for git commit");
            return;
        };

        let Some(ref working_dir) = agent.working_dir else {
            tracing::warn!(
                pane_id = %agent.pane_id,
                project = %agent.project,
                "No working directory set for agent"
            );
            return;
        };

        let git = GitController::new(working_dir.clone());

        // Check for changes first
        match git.has_changes() {
            Ok(false) => {
                tracing::info!(
                    pane_id = %agent.pane_id,
                    project = %agent.project,
                    "No changes to commit"
                );
                return;
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    pane_id = %agent.pane_id,
                    "Failed to check for changes"
                );
                return;
            }
            Ok(true) => {}
        }

        // Create checkpoint commit
        let unix_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let message = format!("Checkpoint from Rehoboam ({unix_ts})");

        match git.checkpoint(&message) {
            Ok(()) => {
                tracing::info!(
                    pane_id = %agent.pane_id,
                    project = %agent.project,
                    message = %message,
                    "Git commit created"
                );
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    pane_id = %agent.pane_id,
                    project = %agent.project,
                    "Git commit failed"
                );
            }
        }
    }

    /// Git push on selected agent's worktree
    fn git_push_selected(&mut self) {
        let Some(agent) = self.state.selected_agent() else {
            tracing::warn!("No agent selected for git push");
            return;
        };

        let Some(ref working_dir) = agent.working_dir else {
            tracing::warn!(
                pane_id = %agent.pane_id,
                project = %agent.project,
                "No working directory set for agent"
            );
            return;
        };

        let git = GitController::new(working_dir.clone());

        match git.push() {
            Ok(()) => {
                tracing::info!(
                    pane_id = %agent.pane_id,
                    project = %agent.project,
                    "Git push completed"
                );
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    pane_id = %agent.pane_id,
                    project = %agent.project,
                    "Git push failed"
                );
            }
        }
    }

    /// Toggle diff view for selected agent
    fn toggle_diff_view(&mut self) {
        // If already showing, hide
        if self.show_diff {
            self.show_diff = false;
            return;
        }

        let Some(agent) = self.state.selected_agent() else {
            tracing::warn!("No agent selected for diff view");
            return;
        };

        let Some(ref working_dir) = agent.working_dir else {
            tracing::warn!(
                pane_id = %agent.pane_id,
                project = %agent.project,
                "No working directory set for agent"
            );
            return;
        };

        let git = GitController::new(working_dir.clone());

        // Get full diff (line-by-line changes)
        match git.diff_full() {
            Ok(diff) => {
                if diff.is_empty() {
                    tracing::info!(
                        pane_id = %agent.pane_id,
                        project = %agent.project,
                        "No changes to display"
                    );
                    self.diff_content = "No uncommitted changes.".to_string();
                } else {
                    self.diff_content = diff;
                }
                self.show_diff = true;
                tracing::debug!(
                    pane_id = %agent.pane_id,
                    project = %agent.project,
                    "Showing diff view"
                );
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    pane_id = %agent.pane_id,
                    project = %agent.project,
                    "Failed to get diff"
                );
            }
        }
    }

    /// Toggle checkpoint timeline modal
    fn toggle_checkpoint_timeline(&mut self) {
        // If already showing, hide
        if self.show_checkpoint_timeline {
            self.show_checkpoint_timeline = false;
            return;
        }

        let Some(agent) = self.state.selected_agent() else {
            tracing::warn!("No agent selected for checkpoint timeline");
            return;
        };

        // Only sprites have checkpoints
        if !agent.is_sprite {
            tracing::info!(
                pane_id = %agent.pane_id,
                project = %agent.project,
                "Checkpoint timeline only available for sprite agents"
            );
            return;
        }

        // For now, show empty timeline (checkpoints would be populated from SpriteManager)
        // In a full implementation, we'd query the SpriteManager for checkpoint history
        self.checkpoint_timeline = Vec::new();
        self.selected_checkpoint = 0;
        self.show_checkpoint_timeline = true;

        tracing::debug!(
            pane_id = %agent.pane_id,
            project = %agent.project,
            "Showing checkpoint timeline"
        );
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
                // Restore to selected checkpoint
                self.restore_selected_checkpoint();
            }
            _ => {}
        }
    }

    /// Restore sprite to selected checkpoint
    fn restore_selected_checkpoint(&mut self) {
        if self.checkpoint_timeline.is_empty() {
            tracing::warn!("No checkpoints to restore");
            return;
        }

        let Some(checkpoint) = self.checkpoint_timeline.get(self.selected_checkpoint) else {
            return;
        };

        let checkpoint_id = checkpoint.id.clone();

        let Some(agent) = self.state.selected_agent() else {
            return;
        };

        tracing::info!(
            pane_id = %agent.pane_id,
            checkpoint_id = %checkpoint_id,
            "Restoring to checkpoint"
        );

        // Close the timeline modal
        self.show_checkpoint_timeline = false;

        // Note: Actual restore would be async through SpriteManager
        // This would be wired through an event system in a full implementation
    }
}
