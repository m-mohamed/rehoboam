//! Git Checkpoint Management
//!
//! Creates git checkpoints between loop iterations for easy rollback.

use color_eyre::eyre::{eyre, Result};
use std::path::Path;
use std::process::Command;
use tracing::{debug, info, warn};

use super::state::{load_state, save_state};

/// Create a git checkpoint after an iteration completes
///
/// Commits all changes with a message indicating the Rehoboam iteration.
/// Returns the commit hash if successful.
pub fn create_git_checkpoint(loop_dir: &Path) -> Result<Option<String>> {
    let state = load_state(loop_dir)?;
    let project_dir = loop_dir
        .parent()
        .ok_or_else(|| eyre!("Invalid rehoboam dir"))?;

    // Check if we're in a git repo
    let git_dir = project_dir.join(".git");
    if !git_dir.exists() {
        debug!("Not a git repository, skipping checkpoint");
        return Ok(None);
    }

    // Stage all changes
    let add_output = Command::new("git")
        .args(["add", "-A"])
        .current_dir(project_dir)
        .output();

    if let Err(e) = add_output {
        warn!("Failed to stage changes: {}", e);
        return Ok(None);
    }

    // Check if there are changes to commit
    let status_output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(project_dir)
        .output()
        .map_err(|e| eyre!("Failed to check git status: {}", e))?;

    let status = String::from_utf8_lossy(&status_output.stdout);
    if status.trim().is_empty() {
        debug!("No changes to commit");
        return Ok(state.last_commit.clone());
    }

    // Commit with Rehoboam iteration message
    let commit_msg = format!(
        "rehoboam: iteration {} checkpoint\n\nAutomated checkpoint from Rehoboam loop.",
        state.iteration
    );

    let commit_output = Command::new("git")
        .args(["commit", "-m", &commit_msg])
        .current_dir(project_dir)
        .output();

    match commit_output {
        Ok(output) if output.status.success() => {
            // Get the commit hash
            let hash_output = Command::new("git")
                .args(["rev-parse", "HEAD"])
                .current_dir(project_dir)
                .output()
                .map_err(|e| eyre!("Failed to get commit hash: {}", e))?;

            let hash = String::from_utf8_lossy(&hash_output.stdout)
                .trim()
                .to_string();

            // Update state with new commit hash
            let mut new_state = state;
            new_state.last_commit = Some(hash.clone());
            save_state(loop_dir, &new_state)?;

            info!("Created git checkpoint: {}", &hash[..8.min(hash.len())]);
            Ok(Some(hash))
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Git commit failed: {}", stderr);
            Ok(None)
        }
        Err(e) => {
            warn!("Failed to run git commit: {}", e);
            Ok(None)
        }
    }
}
