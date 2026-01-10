//! Application actions and messages
//!
//! Defines the actions that can be triggered in the TUI, following
//! the message-passing pattern common in ratatui applications.

use crate::event::HookEvent;

/// Actions that can be triggered in the TUI
///
/// These represent all possible user interactions and system events
/// that the application can respond to.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum Action {
    /// Periodic tick for UI updates (e.g., elapsed time)
    Tick,

    /// Quit the application
    Quit,

    /// Navigate to next agent in the list
    NextAgent,

    /// Navigate to previous agent in the list
    PreviousAgent,

    /// Jump to the selected agent's WezTerm pane
    JumpToAgent,

    /// Toggle debug mode (shows event log)
    ToggleDebug,

    /// Toggle help overlay
    ToggleHelp,

    /// Process an incoming hook event from Claude Code (boxed to reduce enum size)
    HookEvent(Box<HookEvent>),

    // === Phase 1.1: Permission shortcuts ===
    /// Approve permission request (sends "y" + Enter to selected agent's pane)
    Approve,

    /// Reject permission request (sends "n" + Enter to selected agent's pane)
    Reject,

    // === Phase 2: View navigation ===
    /// Move to next column (h/l vim keys)
    NextColumn,

    /// Move to previous column
    PreviousColumn,

    /// Toggle between Kanban and Project view
    ToggleProjectView,

    // === Phase 3: Agent spawning ===
    /// Open spawn dialog
    OpenSpawnDialog,

    /// Toggle auto-accept mode
    ToggleAutoAccept,

    // === Phase 3.2: Bulk operations ===
    /// Toggle selection of current agent
    ToggleSelection,

    /// Approve all selected agents
    ApproveSelected,

    /// Reject all selected agents
    RejectSelected,

    /// Kill all selected agents
    KillSelected,

    /// No action (used for unhandled inputs)
    None,
}

impl Action {
    /// Check if this action should trigger a re-render
    #[allow(dead_code)]
    pub fn should_render(&self) -> bool {
        !matches!(self, Action::None)
    }
}
