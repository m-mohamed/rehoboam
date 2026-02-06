//! Navigation operations: jump to agent, search

use crate::state::{AppState, Status};
use std::process::Command;

/// Jump to selected agent using terminal-appropriate CLI
///
/// Detects terminal type from pane_id format:
/// - Tmux: %0, %1, etc. (starts with %)
/// - WezTerm: numeric ID
pub fn jump_to_selected(state: &AppState) {
    let Some(agent) = state.selected_agent() else {
        return;
    };

    let pane_id = &agent.pane_id;
    tracing::debug!("Jumping to pane {}", pane_id);

    let result = if pane_id.starts_with('%') {
        // Tmux pane format: %0, %1, etc.
        // Use switch-client instead of select-pane to work across sessions
        Command::new("tmux")
            .args(["switch-client", "-t", pane_id])
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

/// Jump to the first agent matching the search query
pub fn jump_to_search_match(state: &mut AppState, query: &str) {
    let query = query.to_lowercase();
    if query.is_empty() {
        return;
    }

    // Find first matching agent
    for agent in state.agents.values() {
        let project_lower = agent.project.to_lowercase();
        let pane_lower = agent.pane_id.to_lowercase();

        if project_lower.contains(&query) || pane_lower.contains(&query) {
            // Set the selected column to the agent's status column
            state.selected_column = match agent.status {
                Status::Attention(_) => 0,
                Status::Working => 1,
                Status::Compacting => 2,
            };
            tracing::debug!(
                project = %agent.project,
                pane_id = %agent.pane_id,
                "Jumping to search match"
            );
            break;
        }
    }
}
