//! Git operations and checkpoint management

use crate::event::Event;
use crate::git::GitController;
use crate::sprite::CheckpointRecord;
use crate::state::AppState;
use sprites::SpritesClient;
use tokio::sync::mpsc;

/// Git commit on selected agent's worktree
///
/// Stages all changes and creates a checkpoint commit.
pub fn git_commit_selected(state: &AppState) {
    let Some(agent) = state.selected_agent() else {
        tracing::warn!("No agent selected for git commit");
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

    // Check for changes first
    match git.has_changes() {
        Ok(false) => {
            tracing::info!(
                pane_id = %agent.pane_id,
                project = %agent.project,
                "No changes to commit"
            );
            return;
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                pane_id = %agent.pane_id,
                "Failed to check for changes"
            );
            return;
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
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                pane_id = %agent.pane_id,
                project = %agent.project,
                "Git commit failed"
            );
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
/// Returns None if diff should not be shown, Some(content) otherwise.
pub fn get_diff_content(state: &AppState) -> Option<String> {
    let agent = state.selected_agent()?;

    let working_dir = agent.working_dir.as_ref()?;

    let git = GitController::new(working_dir.clone());

    match git.diff_full() {
        Ok(diff) => {
            if diff.is_empty() {
                tracing::info!(
                    pane_id = %agent.pane_id,
                    project = %agent.project,
                    "No changes to display"
                );
                Some("No uncommitted changes.".to_string())
            } else {
                tracing::debug!(
                    pane_id = %agent.pane_id,
                    project = %agent.project,
                    "Showing diff view"
                );
                Some(diff)
            }
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                pane_id = %agent.pane_id,
                project = %agent.project,
                "Failed to get diff"
            );
            None
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
