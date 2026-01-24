//! Rehoboam loop state management
//!
//! Implements proper Rehoboam loops with fresh sessions per iteration.
//! State persists in `.rehoboam/` directory, context stays fresh.
//!
//! **REQUIRES**: `CLAUDE_CODE_TASK_LIST_ID` environment variable for task management.
//! Agents use Claude Code native Tasks API (TaskCreate/TaskUpdate/TaskList/TaskGet).
//!
//! Files:
//! - anchor.md: Task spec, success criteria (read every iteration)
//! - guardrails.md: Learned constraints/signs (append-only)
//! - progress.md: What's done, what's next
//! - errors.log: What failed (append-only)
//! - activity.log: Timing/metrics per iteration
//! - session_history.log: State transitions for debugging
//! - state.json: Iteration counter, config
//!
//! NOTE: tasks.md is NO LONGER USED. Tasks are managed via Claude Code Tasks API.

mod activity;
mod git_checkpoint;
mod judge;
mod state;

// Re-export state types
#[allow(unused_imports)] // load_state is used in tests
pub use state::{
    check_max_iterations, increment_iteration, init_loop_dir, load_state, save_state, LoopState,
    RehoboamConfig,
};

// Re-export judge types and functions
pub use judge::{judge_completion, JudgeDecision};


// Re-export activity logging
pub use activity::{
    check_completion, get_iteration_duration, log_activity, log_session_transition,
    mark_iteration_start, track_error_pattern,
};

// Re-export git checkpoint
pub use git_checkpoint::create_git_checkpoint;

use color_eyre::eyre::Result;
use std::fs;
use std::path::Path;
use tracing::debug;

/// Build a simple iteration prompt from loop state files
///
/// Creates a minimal prompt file containing:
/// - Current iteration number
/// - anchor.md (task spec)
/// - progress.md (work done)
/// - guardrails.md (learned constraints)
///
/// Note: Role-specific prompts have been removed. TeammateTool
/// (via CLAUDE_CODE_AGENT_TYPE) now defines agent behavior.
pub fn build_iteration_prompt(loop_dir: &Path) -> Result<String> {
    let state = load_state(loop_dir)?;

    let anchor = fs::read_to_string(loop_dir.join("anchor.md")).unwrap_or_default();
    let progress = fs::read_to_string(loop_dir.join("progress.md")).unwrap_or_default();
    let guardrails = fs::read_to_string(loop_dir.join("guardrails.md")).unwrap_or_default();

    let prompt = format!(
        r#"# Rehoboam Loop - Iteration {iteration}

## Task (anchor.md)
{anchor}

## Progress (progress.md)
{progress}

## Guardrails
{guardrails}

## Instructions
- Continue working on the task from where progress.md left off
- Update progress.md with your work
- Add learned constraints to guardrails.md
- Write "{stop_word}" to progress.md when the task is complete
"#,
        iteration = state.iteration + 1,
        anchor = anchor.trim(),
        progress = progress.trim(),
        guardrails = guardrails.trim(),
        stop_word = state.stop_word,
    );

    // Write to temp file for piping to claude
    let prompt_file = loop_dir.join("_iteration_prompt.md");
    fs::write(&prompt_file, &prompt)?;

    debug!(
        iteration = state.iteration + 1,
        prompt_len = prompt.len(),
        "Built iteration prompt"
    );

    Ok(prompt_file.to_string_lossy().to_string())
}

/// Check if the Planner has completed planning
///
/// Returns true if progress.md contains "PLANNING COMPLETE" (case-insensitive)
pub fn is_planning_complete(loop_dir: &Path) -> bool {
    let progress = loop_dir.join("progress.md");
    if let Ok(content) = fs::read_to_string(&progress) {
        content.to_uppercase().contains("PLANNING COMPLETE")
    } else {
        false
    }
}


#[cfg(test)]
mod tests {
    use super::state::load_state;
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_init_loop_dir() {
        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig {
            max_iterations: 10,
            stop_word: "COMPLETE".to_string(),
            pane_id: "%42".to_string(),
        };

        let loop_dir = init_loop_dir(temp.path(), "Build a REST API", &config).unwrap();

        assert!(loop_dir.join("anchor.md").exists());
        assert!(loop_dir.join("guardrails.md").exists());
        assert!(loop_dir.join("progress.md").exists());
        assert!(loop_dir.join("errors.log").exists());
        assert!(loop_dir.join("state.json").exists());

        let state = load_state(&loop_dir).unwrap();
        assert_eq!(state.iteration, 0);
        assert_eq!(state.max_iterations, 10);
        assert_eq!(state.stop_word, "COMPLETE");
    }

    #[test]
    fn test_increment_iteration() {
        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        let iter1 = increment_iteration(&loop_dir).unwrap();
        assert_eq!(iter1, 1);

        let iter2 = increment_iteration(&loop_dir).unwrap();
        assert_eq!(iter2, 2);
    }

    #[test]
    fn test_check_max_iterations() {
        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig {
            max_iterations: 3,
            ..Default::default()
        };
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        assert!(!check_max_iterations(&loop_dir).unwrap());

        increment_iteration(&loop_dir).unwrap();
        increment_iteration(&loop_dir).unwrap();
        assert!(!check_max_iterations(&loop_dir).unwrap());

        increment_iteration(&loop_dir).unwrap();
        assert!(check_max_iterations(&loop_dir).unwrap());
    }

    #[test]
    fn test_check_completion() {
        use std::fs;

        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig {
            stop_word: "DONE".to_string(),
            ..Default::default()
        };
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        // Initially not complete
        let (complete, _) = check_completion(&loop_dir, "DONE").unwrap();
        assert!(!complete);

        // Stop word triggers completion
        fs::write(loop_dir.join("progress.md"), "Task DONE").unwrap();
        let (complete, reason) = check_completion(&loop_dir, "DONE").unwrap();
        assert!(complete);
        assert_eq!(reason, "stop_word");

        // Promise tag also triggers completion (takes precedence)
        fs::write(loop_dir.join("progress.md"), "<promise>COMPLETE</promise>").unwrap();
        let (complete, reason) = check_completion(&loop_dir, "DONE").unwrap();
        assert!(complete);
        assert_eq!(reason, "promise_tag");
    }

    #[test]
    fn test_log_activity() {
        use std::fs;

        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        log_activity(&loop_dir, 1, Some(120), Some(50), "continuing").unwrap();
        log_activity(&loop_dir, 2, Some(90), None, "complete:stop_word").unwrap();

        let content = fs::read_to_string(loop_dir.join("activity.log")).unwrap();
        assert!(content.contains("Iteration 1"));
        assert!(content.contains("2m 0s"));
        assert!(content.contains("Iteration 2"));
        assert!(content.contains("complete:stop_word"));
    }

    #[test]
    fn test_log_session_transition() {
        use std::fs;

        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        log_session_transition(&loop_dir, "init", "starting", Some("%42")).unwrap();
        log_session_transition(&loop_dir, "working", "stopping", None).unwrap();
        log_session_transition(&loop_dir, "stopping", "respawning", Some("iteration 2")).unwrap();

        let content = fs::read_to_string(loop_dir.join("session_history.log")).unwrap();
        assert!(content.contains("init -> starting"));
        assert!(content.contains("working -> stopping"));
        assert!(content.contains("respawning"));
    }

    #[test]
    fn test_track_error_pattern() {
        use std::fs;

        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        // First two occurrences don't trigger guardrail
        assert!(!track_error_pattern(&loop_dir, "API rate limit exceeded").unwrap());
        assert!(!track_error_pattern(&loop_dir, "API rate limit exceeded").unwrap());

        // Third occurrence triggers auto-guardrail
        assert!(track_error_pattern(&loop_dir, "API rate limit exceeded").unwrap());

        // Check guardrail was added
        let guardrails = fs::read_to_string(loop_dir.join("guardrails.md")).unwrap();
        assert!(guardrails.contains("Auto-detected"));
        assert!(guardrails.contains("occurred 3 times"));
    }

    #[test]
    fn test_activity_log_created() {
        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        assert!(loop_dir.join("activity.log").exists());
        assert!(loop_dir.join("session_history.log").exists());
    }

    // Judge mode tests
    //
    // Note: judge_completion() now always uses Claude Code (no heuristics).
    // Unit testing the full function would require Claude Code to be installed.
    // The JudgeDecision enum is still tested via other integration tests.

    // NOTE: Coordination and permission tests removed - modules deleted
    // TeammateTool now handles agent coordination and permissions
}
