//! Navigation operations: jump to agent, search, capture output

use crate::state::{AppState, Status};
use crate::tmux::TmuxController;
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
                Status::Idle => 3,
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

/// Capture output from selected agent's pane
///
/// Uses TmuxController::capture_pane for local agents.
/// Returns the captured output string.
pub fn capture_selected_output(state: &AppState) -> String {
    let Some(agent) = state.selected_agent() else {
        return "No agent selected.\n\nUse j/k to select an agent.".to_string();
    };

    let pane_id = &agent.pane_id;

    if agent.is_sprite {
        // Sprite agents: would need async capture via sprite.get_output()
        format!(
            "☁ Sprite Agent: {}\n\
             Project: {}\n\
             Status: {:?}\n\
             \n\
             [Live output from sprites requires async capture - coming soon]\n",
            pane_id, agent.project, agent.status,
        )
    } else if pane_id.starts_with('%') {
        // Tmux panes: capture directly
        match TmuxController::capture_pane(pane_id) {
            Ok(output) => output,
            Err(e) => {
                format!(
                    "Error capturing pane {}: {}\n\n\
                     The pane may have closed or be unavailable.",
                    pane_id, e
                )
            }
        }
    } else {
        format!(
            "Pane {} (non-tmux)\n\
             Project: {}\n\
             Status: {:?}\n\
             \n\
             [Live output requires tmux pane]",
            pane_id, agent.project, agent.status,
        )
    }
}
