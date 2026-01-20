//! View rendering modules
//!
//! Contains the main view rendering functions for different view modes.

mod kanban;
mod project;
mod split;

pub use kanban::render_agent_columns;
pub use project::render_project_view;
pub use split::render_split_view;
