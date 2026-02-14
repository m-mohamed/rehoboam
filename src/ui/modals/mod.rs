//! Modal dialogs and overlays
//!
//! This module contains all modal/popup rendering functions.

mod debug;
mod event_log;
mod help;
mod history;
mod insights;
mod plans;
mod spawn;
mod stats;
pub use debug::render_debug_viewer;
pub use event_log::render_event_log;
pub use help::render_help;
pub use history::render_history_viewer;
pub use insights::render_insights_viewer;
pub use plans::render_plan_viewer;
pub use spawn::render_spawn_dialog;
pub use stats::render_stats_viewer;
