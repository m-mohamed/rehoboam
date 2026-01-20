//! Loop handling logic for Rehoboam state management
//!
//! This module contains the loop continuation logic including:
//! - Stall detection
//! - Fresh session spawning for Rehoboam loops

use super::{Agent, LoopMode};
use crate::notify;
use crate::rehoboam_loop;
use crate::tmux::TmuxController;
use std::collections::VecDeque;

/// Check if loop is stalled (5+ consecutive identical stop reasons)
///
/// Stall detection prevents infinite loops on repeating errors.
/// If the last 5 stop reasons are identical, the agent is stuck.
pub fn is_stalled(reasons: &VecDeque<String>) -> bool {
    if reasons.len() < 5 {
        return false;
    }
    if let Some(last) = reasons.back() {
        reasons.iter().rev().take(5).all(|r| r == last)
    } else {
        false
    }
}

/// Spawn a fresh Rehoboam session in the given pane
///
/// This is the core of proper Rehoboam loops:
/// 1. Increment iteration counter in state.json
/// 2. Check stop word in progress.md
/// 3. Build iteration prompt with current state
/// 4. Kill old pane, spawn fresh Claude session
///
/// Returns the new pane_id (may be different from old one)
pub fn spawn_fresh_rehoboam_session(
    pane_id: &str,
    loop_dir: &std::path::Path,
    agent: &mut Agent,
) -> color_eyre::eyre::Result<String> {
    use color_eyre::eyre::WrapErr;

    // Log session transition: iteration ending
    let _ = rehoboam_loop::log_session_transition(
        loop_dir,
        "working",
        "stopping",
        Some("iteration ending"),
    );

    // Get iteration duration before incrementing
    let duration = rehoboam_loop::get_iteration_duration(loop_dir);

    // 1. Increment iteration counter
    let new_iteration = rehoboam_loop::increment_iteration(loop_dir)
        .wrap_err("Failed to increment Rehoboam iteration")?;
    agent.loop_iteration = new_iteration;

    // 2. Check completion (stop word OR promise tag)
    let (is_complete, completion_reason) =
        rehoboam_loop::check_completion(loop_dir, &agent.loop_stop_word)
            .wrap_err("Failed to check completion")?;

    if is_complete {
        // Log activity for completed iteration
        let _ = rehoboam_loop::log_activity(
            loop_dir,
            new_iteration,
            duration,
            None,
            &format!("complete:{}", completion_reason),
        );

        // Create final git checkpoint
        let _ = rehoboam_loop::create_git_checkpoint(loop_dir);
        let _ = rehoboam_loop::log_session_transition(
            loop_dir,
            "stopping",
            "complete",
            Some(&completion_reason),
        );

        agent.loop_mode = LoopMode::Complete;
        tracing::info!(
            pane_id = %pane_id,
            iteration = new_iteration,
            reason = %completion_reason,
            "Rehoboam loop complete"
        );
        notify::send(
            "Rehoboam Complete",
            &format!(
                "{}: {} iterations ({})",
                agent.project, new_iteration, completion_reason
            ),
            Some("Glass"),
        );
        return Ok(pane_id.to_string());
    }

    // 3. Check max iterations
    if rehoboam_loop::check_max_iterations(loop_dir).wrap_err("Failed to check max iterations")? {
        // Log activity
        let _ =
            rehoboam_loop::log_activity(loop_dir, new_iteration, duration, None, "max_iterations");

        // Create git checkpoint
        let _ = rehoboam_loop::create_git_checkpoint(loop_dir);
        let _ = rehoboam_loop::log_session_transition(loop_dir, "stopping", "max_reached", None);

        agent.loop_mode = LoopMode::Complete;
        tracing::info!(
            pane_id = %pane_id,
            iteration = new_iteration,
            max = agent.loop_max,
            "Rehoboam loop complete: max iterations reached"
        );
        notify::send(
            "Rehoboam Max Reached",
            &format!("{}: {} iterations", agent.project, new_iteration),
            Some("Basso"),
        );
        return Ok(pane_id.to_string());
    }

    // Log activity for continuing iteration
    let _ = rehoboam_loop::log_activity(loop_dir, new_iteration, duration, None, "continuing");

    // 4. Create git checkpoint before respawning
    let _ = rehoboam_loop::create_git_checkpoint(loop_dir);

    // 5. Build iteration prompt
    let prompt_file = rehoboam_loop::build_iteration_prompt(loop_dir)
        .wrap_err("Failed to build iteration prompt")?;

    // 6. Get project directory for respawn
    let project_dir = loop_dir
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());

    // Log session transition: respawning
    let _ = rehoboam_loop::log_session_transition(
        loop_dir,
        "stopping",
        "respawning",
        Some(&format!("iteration {}", new_iteration + 1)),
    );

    // 7. Send Ctrl+C to ensure clean shutdown, then kill pane
    let _ = TmuxController::send_interrupt(pane_id);
    std::thread::sleep(std::time::Duration::from_millis(100));

    if let Err(e) = TmuxController::kill_pane(pane_id) {
        tracing::warn!(
            pane_id = %pane_id,
            error = %e,
            "Failed to kill old pane (may already be gone)"
        );
    }

    // 8. Respawn fresh Claude session
    let new_pane_id = TmuxController::respawn_claude(&project_dir, &prompt_file)
        .wrap_err("Failed to respawn Claude session")?;

    // 9. Mark iteration start time for next iteration
    let _ = rehoboam_loop::mark_iteration_start(loop_dir);
    let _ = rehoboam_loop::log_session_transition(
        loop_dir,
        "respawning",
        "working",
        Some(&new_pane_id),
    );

    tracing::info!(
        old_pane = %pane_id,
        new_pane = %new_pane_id,
        iteration = new_iteration,
        prompt_file = %prompt_file,
        "Spawned fresh Rehoboam session"
    );

    Ok(new_pane_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_stalled_returns_false() {
        // Table-driven test for cases that should NOT stall
        let cases: Vec<(Vec<&str>, &str)> = vec![
            (vec![], "empty"),
            (vec!["a", "b", "c", "d"], "less than 5"),
            (vec!["a", "b", "c", "d", "e"], "5 different"),
        ];
        for (reasons_data, desc) in cases {
            let reasons: VecDeque<String> = reasons_data.into_iter().map(String::from).collect();
            assert!(!is_stalled(&reasons), "should not stall: {}", desc);
        }
    }

    #[test]
    fn test_is_stalled_returns_true() {
        // Table-driven test for cases that SHOULD stall
        let cases: Vec<(Vec<&str>, &str)> = vec![
            (vec!["same"; 5], "5 identical"),
            (
                vec!["different", "same", "same", "same", "same", "same"],
                "last 5 identical",
            ),
        ];
        for (reasons_data, desc) in cases {
            let reasons: VecDeque<String> = reasons_data.into_iter().map(String::from).collect();
            assert!(is_stalled(&reasons), "should stall: {}", desc);
        }
    }
}
