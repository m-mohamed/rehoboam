//! Git operations and checkpoint management
//!
//! Provides git-based workflow operations for agents running in worktrees.
//!
//! # Available Operations
//!
//! ## Checkpoint Operations (`g` key)
//! - Stages all changes in agent's worktree
//! - Creates timestamped commit: "Checkpoint from Rehoboam ({timestamp})"
//! - Safe to call repeatedly (no-op if no changes)
//!
//! ## Push Operations (`G` key)
//! - Pushes current branch to remote origin
//! - Sets upstream tracking if not already set
//!
//! ## Diff Viewing (`D` key)
//! - Shows uncommitted changes as structured diff
//! - Supports session-scoped diffs (changes since session start)
//! - Collapsible hunks for large diffs
//!
//! ## Checkpoint Timeline (Sprite agents only)
//! - Fetches checkpoint history from sprite API
//! - Displays timeline with restore capability
//!
//! # Worktree Context
//!
//! Operations require `agent.working_dir` to be set. This is automatically
//! configured when spawning agents with the worktree option enabled.

use crate::diff::{parse_diff, ParsedDiff};
use crate::event::Event;
use crate::git::GitController;
use crate::sprite::CheckpointRecord;
use crate::state::AppState;
use sprites::SpritesClient;
use tokio::sync::mpsc;

/// Git commit on selected agent's worktree
///
/// Stages all changes and creates a checkpoint commit.
/// Returns a status message for UI display.
pub fn git_commit_selected(state: &AppState) -> Option<String> {
    let Some(agent) = state.selected_agent() else {
        tracing::warn!("No agent selected for git commit");
        return Some("No agent selected".to_string());
    };

    let Some(ref working_dir) = agent.working_dir else {
        tracing::warn!(
            pane_id = %agent.pane_id,
            project = %agent.project,
            "No working directory set for agent"
        );
        return Some("No working directory set".to_string());
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
            return Some("No changes to commit".to_string());
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                pane_id = %agent.pane_id,
                "Failed to check for changes"
            );
            return Some(format!("Git error: {}", e));
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
            None // Success, no error to show
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                pane_id = %agent.pane_id,
                project = %agent.project,
                "Git commit failed"
            );
            Some(format!("Commit failed: {}", e))
        }
    }
}

/// Git push on selected agent's worktree
pub fn git_push_selected(state: &AppState) {
    let Some(agent) = state.selected_agent() else {
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

/// Get diff content for selected agent
///
/// Returns Ok((raw, parsed)) on success, Err(message) with user-friendly error otherwise.
/// Uses session-scoped diff if session_start_commit is available (shows only
/// changes made during this session), otherwise shows full diff.
pub fn get_diff_content(state: &AppState) -> Result<(String, ParsedDiff), String> {
    let agent = state.selected_agent().ok_or("No agent selected")?;

    let working_dir = agent
        .working_dir
        .as_ref()
        .ok_or("Agent has no working directory")?;

    let git = GitController::new(working_dir.clone());

    // Check if it's actually a git repository
    if !git.is_git_repo() {
        return Err("Not a git repository".into());
    }

    // Try session-scoped diff first (v2.0)
    let diff_result = if let Some(ref commit) = agent.session_start_commit {
        tracing::debug!(
            pane_id = %agent.pane_id,
            commit = %commit,
            "Using session-scoped diff since commit"
        );
        git.diff_since(commit)
    } else {
        // Fallback to full diff
        git.diff_full()
    };

    match diff_result {
        Ok(diff) => {
            if diff.is_empty() {
                tracing::info!(
                    pane_id = %agent.pane_id,
                    project = %agent.project,
                    "No changes to display"
                );
                Ok(("No uncommitted changes.".to_string(), ParsedDiff::empty()))
            } else {
                tracing::debug!(
                    pane_id = %agent.pane_id,
                    project = %agent.project,
                    files_modified = agent.modified_files.len(),
                    "Showing diff view"
                );
                let parsed = parse_diff(&diff);
                Ok((diff, parsed))
            }
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                pane_id = %agent.pane_id,
                project = %agent.project,
                "Failed to get diff"
            );
            Err(format!("Git error: {}", e))
        }
    }
}

/// Fetch checkpoints for selected sprite agent
///
/// Spawns an async task to fetch checkpoints from the API.
/// Returns the sprite_id if checkpoints should be shown.
pub fn fetch_checkpoints(
    state: &AppState,
    sprites_client: Option<&SpritesClient>,
    event_tx: Option<&mpsc::Sender<Event>>,
) -> Option<String> {
    let agent = state.selected_agent()?;

    // Only sprites have checkpoints
    if !agent.is_sprite {
        tracing::info!(
            pane_id = %agent.pane_id,
            project = %agent.project,
            "Checkpoint timeline only available for sprite agents"
        );
        return None;
    }

    let sprite_id = agent.sprite_id.clone()?;

    // Spawn async task to fetch checkpoints
    if let (Some(client), Some(tx)) = (sprites_client, event_tx) {
        let client = client.clone();
        let tx = tx.clone();
        let sprite_id_clone = sprite_id.clone();

        tokio::spawn(async move {
            let sprite = client.sprite(&sprite_id_clone);
            match sprite.list_checkpoints().await {
                Ok(checkpoints) => {
                    tracing::debug!(
                        sprite_id = %sprite_id_clone,
                        count = checkpoints.len(),
                        "Fetched checkpoints from API"
                    );
                    if let Err(e) = tx
                        .send(Event::CheckpointData {
                            sprite_id: sprite_id_clone,
                            checkpoints,
                        })
                        .await
                    {
                        tracing::error!("Failed to send checkpoint data: {}", e);
                    }
                }
                Err(e) => {
                    tracing::error!(
                        sprite_id = %sprite_id_clone,
                        error = %e,
                        "Failed to fetch checkpoints"
                    );
                }
            }
        });
    }

    tracing::debug!(
        pane_id = %agent.pane_id,
        project = %agent.project,
        sprite_id = %sprite_id,
        "Showing checkpoint timeline (fetching data...)"
    );

    Some(sprite_id)
}

/// Restore sprite to a checkpoint
pub fn restore_checkpoint(state: &AppState, checkpoint: &CheckpointRecord) {
    let Some(agent) = state.selected_agent() else {
        return;
    };

    tracing::info!(
        pane_id = %agent.pane_id,
        checkpoint_id = %checkpoint.id,
        "Restoring to checkpoint"
    );

    // Note: Actual restore would be async through SpriteManager
    // This would be wired through an event system in a full implementation
}
