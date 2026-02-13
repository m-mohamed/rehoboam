//! Navigation operations: jump to agent, search

use crate::state::AppState;
use std::process::Command;

/// Jump to selected agent's tmux pane
pub fn jump_to_selected(state: &AppState) {
    let Some(agent) = state.selected_agent() else {
        return;
    };

    let pane_id = &agent.pane_id;
    tracing::debug!("Jumping to pane {}", pane_id);

    if pane_id.starts_with("team:") {
        tracing::debug!(pane_id = %pane_id, "Cannot jump to phantom agent");
        return;
    }
    if !pane_id.starts_with('%') {
        tracing::debug!(pane_id = %pane_id, "Cannot jump: not a tmux pane");
        return;
    }

    let result = Command::new("tmux")
        .args(["select-pane", "-t", pane_id])
        .output();

    match result {
        Ok(output) if !output.status.success() => {
            tracing::warn!(
                pane_id = %pane_id,
                "Failed to activate pane: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Err(e) => {
            tracing::warn!(pane_id = %pane_id, error = %e, "Terminal CLI not available for pane activation");
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
            state.selected_pane_id = Some(agent.pane_id.clone());
            tracing::debug!(
                project = %agent.project,
                pane_id = %agent.pane_id,
                "Jumping to search match"
            );
            break;
        }
    }
}
