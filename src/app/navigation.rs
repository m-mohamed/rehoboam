//! Navigation operations: jump to agent, search

use crate::state::{AppState, Status};
use std::process::Command;

/// Jump to selected agent using terminal-appropriate CLI
///
/// Detects terminal type from pane_id format and environment:
/// - Tmux: %0, %1, etc. (starts with %)
/// - Kitty: detected via KITTY_WINDOW_ID env var
/// - iTerm2: detected via ITERM_SESSION_ID env var
/// - WezTerm: numeric ID (fallback for non-tmux panes)
pub fn jump_to_selected(state: &AppState) {
    let Some(agent) = state.selected_agent() else {
        return;
    };

    let pane_id = &agent.pane_id;
    tracing::debug!("Jumping to pane {}", pane_id);

    let result = if pane_id.starts_with("team:") {
        // Phantom agent (team member without tmux pane) — cannot jump
        tracing::debug!(pane_id = %pane_id, "Cannot jump to phantom agent (no tmux pane)");
        return;
    } else if pane_id.starts_with('%') {
        // Tmux pane format: %0, %1, etc.
        // Use select-pane for better cross-window handling
        Command::new("tmux")
            .args(["select-pane", "-t", pane_id])
            .output()
    } else if std::env::var("KITTY_WINDOW_ID").is_ok() {
        // Kitty terminal — focus window by ID
        Command::new("kitty")
            .args(["@", "focus-window", "--match", &format!("id:{}", pane_id)])
            .output()
    } else if std::env::var("ITERM_SESSION_ID").is_ok() {
        // iTerm2 — use AppleScript to activate session
        Command::new("osascript")
            .args([
                "-e",
                &format!(
                    "tell application \"iTerm2\" to tell current window to \
                     select (first session of every tab whose unique ID is \"{}\")",
                    pane_id
                ),
            ])
            .output()
    } else {
        // WezTerm pane (numeric ID) — default fallback
        Command::new("wezterm")
            .args(["cli", "activate-pane", "--pane-id", pane_id])
            .output()
    };

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
