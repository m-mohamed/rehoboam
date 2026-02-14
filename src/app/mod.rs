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
//!
//! Direct field access is internal API and may change between versions.

mod keyboard;
mod navigation;
pub mod spawn;

pub use spawn::SpawnState;

use crate::config::{HealthConfig, TimeoutConfig};
use crate::event::{Event, EventSource, SpriteStatusType};
use crate::health::HealthChecker;
use crate::plans::PlanViewerState;
use crate::state::AppState;
use sprites::SpritesClient;

/// Input mode for the application
#[derive(Debug, Clone, PartialEq, Default)]
pub enum InputMode {
    /// Normal navigation mode
    #[default]
    Normal,
    /// Spawn dialog mode (creating new agent)
    Spawn,
    /// Search mode (filtering agents)
    Search,
    /// Plan viewer mode (browsing/reading plans)
    PlanViewer,
    /// Stats dashboard mode
    StatsViewer,
    /// History timeline mode
    HistoryViewer,
    /// Debug log viewer mode
    DebugViewer,
    /// Insights report viewer mode
    InsightsViewer,
}

/// State for the stats dashboard overlay
#[derive(Debug, Default)]
pub struct StatsViewerState {
    /// Active tab: 0=Overview, 1=Models, 2=Activity, 3=Quality
    pub active_tab: usize,
    /// Scroll offset within the current tab
    pub scroll_offset: u16,
}

/// State for the history timeline overlay
#[derive(Debug, Default)]
pub struct HistoryViewerState {
    /// Currently selected entry index
    pub selected_index: usize,
    /// Scroll offset for the visible window
    pub scroll_offset: usize,
}

/// State for the debug log viewer overlay
#[derive(Debug, Default)]
pub struct DebugViewerState {
    /// Currently selected entry in list mode
    pub selected_index: usize,
    /// true = reading log content, false = browsing list
    pub viewing: bool,
    /// Scroll offset in reader mode
    pub scroll_offset: u16,
    /// Total height of rendered content (for scroll bounds)
    pub rendered_height: u16,
    /// Loaded file content (one file at a time)
    pub content: String,
}

/// State for the insights report viewer overlay
#[derive(Debug, Default)]
pub struct InsightsViewerState {
    /// Active section index
    pub active_section: usize,
    /// Scroll offset within the current section
    pub scroll_offset: u16,
}

/// Application state and logic
pub struct App {
    pub state: AppState,
    pub should_quit: bool,
    pub debug_mode: bool,
    pub show_help: bool,
    /// Dirty flag: true if UI needs re-render (render-on-change optimization)
    pub needs_render: bool,
    /// Current input mode
    pub input_mode: InputMode,
    /// Spawn dialog state
    pub spawn_state: SpawnState,
    /// Sprites API client (None if sprites not enabled)
    pub sprites_client: Option<SpritesClient>,
    /// Show task board overlay
    pub show_task_board: bool,
    /// Show plan viewer overlay
    pub show_plan_viewer: bool,
    /// Plan viewer state
    pub plan_viewer: PlanViewerState,
    /// Search query for agent filtering
    pub search_query: String,
    /// Show stats dashboard overlay
    pub show_stats_viewer: bool,
    /// Stats viewer state
    pub stats_viewer: StatsViewerState,
    /// Show history timeline overlay
    pub show_history_viewer: bool,
    /// History viewer state
    pub history_viewer: HistoryViewerState,
    /// Show debug log viewer overlay
    pub show_debug_viewer: bool,
    /// Debug viewer state
    pub debug_viewer: DebugViewerState,
    /// Show insights report overlay
    pub show_insights_viewer: bool,
    /// Insights viewer state
    pub insights_viewer: InsightsViewerState,
    /// hooks.log health checker
    health_checker: HealthChecker,
}

impl App {
    pub fn new(
        debug_mode: bool,
        sprites_client: Option<SpritesClient>,
        health_config: &HealthConfig,
        timeout_config: &TimeoutConfig,
    ) -> Self {
        Self {
            state: AppState::with_timeouts(
                timeout_config.idle_timeout_secs,
                timeout_config.stale_timeout_secs,
            ),
            should_quit: false,
            debug_mode,
            show_help: false,
            needs_render: true, // Always render first frame
            input_mode: InputMode::Normal,
            spawn_state: SpawnState::default(),
            sprites_client,
            show_task_board: false,
            show_plan_viewer: false,
            plan_viewer: PlanViewerState::default(),
            search_query: String::new(),
            show_stats_viewer: false,
            stats_viewer: StatsViewerState::default(),
            show_history_viewer: false,
            history_viewer: HistoryViewerState::default(),
            show_debug_viewer: false,
            debug_viewer: DebugViewerState::default(),
            show_insights_viewer: false,
            insights_viewer: InsightsViewerState::default(),
            health_checker: HealthChecker::new(health_config),
        }
    }

    /// Handle incoming events
    pub fn handle_event(&mut self, event: Event) {
        match event {
            Event::Hook(hook_event) => {
                let changed = self.state.process_event(*hook_event);
                self.needs_render = self.needs_render || changed;
            }
            Event::Key(key) => {
                self.handle_key(key);
                self.needs_render = true;
            }
            Event::RemoteHook { sprite_id, event } => {
                let mut hook_event = *event;
                hook_event.source = EventSource::Sprite {
                    sprite_id: sprite_id.clone(),
                };
                let changed = self.state.process_event(hook_event);
                self.needs_render = self.needs_render || changed;
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
        }
    }

    /// Tick for triggering re-renders
    ///
    /// Events update state, ticks trigger re-render only.
    pub fn tick(&mut self) {
        // Process timeout-based state transitions
        self.state.tick();

        // Scan ~/.claude/teams/ to enrich agents with team membership
        // Throttled internally: every 5s at startup, every 120s in steady-state
        self.state.refresh_team_metadata();

        // Scan ~/.claude/tasks/ for task board data
        // Throttled internally alongside team metadata refresh
        self.state.refresh_task_data();

        // Stats always refreshes (small file, 60s throttle)
        self.state.refresh_stats_data();

        // History and debug only refresh when their view is open
        if self.show_history_viewer {
            self.state.refresh_history_data();
        }
        if self.show_debug_viewer {
            self.state.refresh_debug_data();
        }
        if self.show_insights_viewer {
            self.state.refresh_insights_data();
        }

        // Run hooks.log health check (throttled to every 60s by default)
        if self.health_checker.should_run() {
            let modified = self.health_checker.check(&mut self.state);
            self.needs_render = self.needs_render || modified;
        }

        // Tick triggers re-render for elapsed time updates
        self.needs_render = true;
    }

    /// Called after render to reset dirty flag
    pub fn rendered(&mut self) {
        self.needs_render = false;
    }
}
