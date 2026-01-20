//! Application state and logic
//!
//! This module contains the main `App` struct and related types.
//! Keyboard handling, spawning, and operations are in sub-modules.
//!
//! # API Stability
//!
//! The `App` struct fields are public for sub-module access within the crate.
//! External consumers should use public methods only:
//!
//! - [`App::new()`] - Constructor
//! - [`App::handle_event()`] - Event processing
//! - [`App::tick()`] - Timer updates
//! - [`App::rendered()`] - Mark frame as rendered
//! - [`App::show_status()`] - Display status messages
//!
//! Direct field access is internal API and may change between versions.

mod agent_control;
mod keyboard;
mod navigation;
mod operations;
pub mod spawn;

pub use spawn::SpawnState;

use crate::config::ReconciliationConfig;
use crate::diff::ParsedDiff;
use crate::event::{Event, EventSource, SpriteStatusType};
use crate::reconcile::Reconciler;
use crate::sprite::{CheckpointRecord, SpritePool};
use crate::state::AppState;
use sprites::SpritesClient;
use std::collections::HashSet;
use tokio::sync::mpsc;

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
    /// Search mode (filtering agents)
    Search,
}

/// View mode for the main display
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ViewMode {
    /// Kanban-style columns by status (Attention, Working, Compact, Idle)
    #[default]
    Kanban,
    /// Grouped by project name
    Project,
    /// Split view: agent list on left, live output on right
    Split,
}

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
    /// Sprites API client (None if sprites not enabled)
    pub sprites_client: Option<SpritesClient>,
    /// Event sender for async operations
    pub event_tx: Option<mpsc::Sender<Event>>,
    /// Show diff modal
    pub show_diff: bool,
    /// Raw diff content (for backwards compatibility)
    pub diff_content: String,
    /// Parsed diff with structured data (for enhanced modal)
    pub parsed_diff: Option<ParsedDiff>,
    /// Vertical scroll position in diff view
    pub diff_scroll: u16,
    /// Currently selected file index in diff
    pub diff_selected_file: usize,
    /// Set of collapsed hunks: (file_idx, hunk_idx)
    pub diff_collapsed_hunks: HashSet<(usize, usize)>,
    /// Currently selected hunk index within the file
    pub diff_selected_hunk: usize,
    /// Show checkpoint timeline modal
    pub show_checkpoint_timeline: bool,
    /// Checkpoint history for timeline display
    pub checkpoint_timeline: Vec<CheckpointRecord>,
    /// Selected checkpoint index in timeline
    pub selected_checkpoint: usize,
    /// Status message to display in footer (message, timestamp)
    pub status_message: Option<(String, std::time::Instant)>,
    /// Live output from selected agent's pane (for split view)
    pub live_output: String,
    /// Last time we captured pane output
    pub last_output_capture: std::time::Instant,
    /// Scroll offset for live output view
    pub output_scroll: u16,
    /// Show subagent tree panel
    pub show_subagents: bool,
    /// Show progress dashboard overlay
    pub show_dashboard: bool,
    /// Search query for agent filtering
    pub search_query: String,
    /// v1.5: Sprite pool for distributed workers
    pub sprite_pool: Option<SpritePool>,
    /// Show pool management modal
    pub show_pool_management: bool,
    /// Session start time for dashboard
    pub session_start: std::time::Instant,
    /// Tmux reconciler for detecting stuck agents
    reconciler: Reconciler,
}

impl App {
    pub fn new(
        debug_mode: bool,
        sprites_client: Option<SpritesClient>,
        event_tx: Option<mpsc::Sender<Event>>,
        reconciliation_config: &ReconciliationConfig,
    ) -> Self {
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
            sprites_client,
            event_tx,
            show_diff: false,
            diff_content: String::new(),
            parsed_diff: None,
            diff_scroll: 0,
            diff_selected_file: 0,
            diff_collapsed_hunks: HashSet::new(),
            diff_selected_hunk: 0,
            show_checkpoint_timeline: false,
            checkpoint_timeline: Vec::new(),
            selected_checkpoint: 0,
            status_message: None,
            live_output: String::new(),
            last_output_capture: std::time::Instant::now(),
            output_scroll: 0,
            show_subagents: false,
            show_dashboard: false,
            search_query: String::new(),
            session_start: std::time::Instant::now(),
            reconciler: Reconciler::new(reconciliation_config),
            sprite_pool: None,
            show_pool_management: false,
        }
    }

    /// Show a status message in the footer (clears after 5 seconds)
    pub fn show_status(&mut self, msg: &str) {
        self.status_message = Some((msg.to_string(), std::time::Instant::now()));
        self.needs_render = true;
    }

    /// Handle incoming events
    pub fn handle_event(&mut self, event: Event) {
        match event {
            Event::Hook(hook_event) => {
                // Only process hook events if not frozen
                if !self.frozen {
                    let changed = self.state.process_event(*hook_event);
                    self.needs_render = self.needs_render || changed;
                }
            }
            Event::Key(key) => {
                self.handle_key(key);
                self.needs_render = true;
            }
            Event::RemoteHook { sprite_id, event } => {
                // Process remote hook events from sprites
                if !self.frozen {
                    let event_name = event.event.clone();
                    let mut hook_event = *event;
                    hook_event.source = EventSource::Sprite {
                        sprite_id: sprite_id.clone(),
                    };
                    let changed = self.state.process_event(hook_event);
                    self.needs_render = self.needs_render || changed;

                    // Handle sprite loop continuation
                    // After Stop event, check if sprite needs loop continuation
                    if event_name == "Stop" {
                        if let Some(agent) = self.state.agents.get(&sprite_id) {
                            if agent.is_sprite && agent.loop_mode == crate::state::LoopMode::Active
                            {
                                // Sprite needs loop continuation - send Enter keystroke
                                if let Some(client) = &self.sprites_client {
                                    let sprite = client.sprite(&sprite_id);
                                    let iteration = agent.loop_iteration;
                                    let max = agent.loop_max;
                                    tokio::spawn(async move {
                                        tracing::info!(
                                            sprite_id = %sprite.name(),
                                            iteration = iteration,
                                            max = max,
                                            "Sending Enter for sprite loop continuation"
                                        );
                                        // Send Enter keystroke to continue the loop
                                        if let Err(e) =
                                            crate::sprite::controller::SpriteController::send_input(
                                                &sprite, "\n",
                                            )
                                            .await
                                        {
                                            tracing::error!(
                                                sprite_id = %sprite.name(),
                                                error = %e,
                                                "Failed to send Enter for sprite loop continuation"
                                            );
                                        }
                                    });
                                } else {
                                    tracing::warn!(
                                        sprite_id = %sprite_id,
                                        "Sprite loop continuation needed but no sprites client configured"
                                    );
                                }
                            }
                        }
                    }
                }
            }
            Event::SpriteStatus { sprite_id, status } => {
                match status {
                    SpriteStatusType::Connected => {
                        tracing::info!("Sprite connected: {}", sprite_id);
                        self.state.sprite_connected(&sprite_id);
                    }
                    SpriteStatusType::Disconnected => {
                        tracing::info!("Sprite disconnected: {}", sprite_id);
                        self.state.sprite_disconnected(&sprite_id);
                    }
                }
                self.needs_render = true;
            }
            Event::CheckpointData {
                sprite_id,
                checkpoints,
            } => {
                tracing::debug!(
                    sprite_id = %sprite_id,
                    count = checkpoints.len(),
                    "Received checkpoint data"
                );
                self.checkpoint_timeline = checkpoints
                    .into_iter()
                    .map(CheckpointRecord::from)
                    .collect();
                self.selected_checkpoint = 0;
                self.needs_render = true;
            }
        }
    }

    /// Tick for triggering re-renders
    ///
    /// Events update state, ticks trigger re-render only.
    pub fn tick(&mut self) {
        // Process timeout-based state transitions
        self.state.tick();

        // Run tmux reconciliation (throttled to every 5s internally)
        // Detects stuck agents by checking pane output for permission prompts
        if self.reconciler.should_run() {
            let modified = self.reconciler.run(&mut self.state);
            self.needs_render = self.needs_render || modified;
        }

        // Capture pane output periodically in split view
        if self.view_mode == ViewMode::Split {
            // Rate limit captures
            if self.last_output_capture.elapsed() >= std::time::Duration::from_millis(500) {
                self.live_output = navigation::capture_selected_output(&self.state);
                self.last_output_capture = std::time::Instant::now();
            }
        }

        // Tick triggers re-render for elapsed time updates
        self.needs_render = true;
    }

    /// Called after render to reset dirty flag
    pub fn rendered(&mut self) {
        self.needs_render = false;
    }
}
