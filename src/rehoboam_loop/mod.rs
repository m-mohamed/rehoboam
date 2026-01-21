//! Rehoboam loop state management
//!
//! Implements proper Rehoboam loops with fresh sessions per iteration.
//! State persists in `.rehoboam/` directory, context stays fresh.
//!
//! Files:
//! - anchor.md: Task spec, success criteria (read every iteration)
//! - guardrails.md: Learned constraints/signs (append-only)
//! - progress.md: What's done, what's next
//! - errors.log: What failed (append-only)
//! - activity.log: Timing/metrics per iteration
//! - session_history.log: State transitions for debugging
//! - state.json: Iteration counter, config

mod activity;
mod coordination;
mod git_checkpoint;
mod judge;
mod permissions;
mod prompts;
mod state;
mod tasks;

// Re-export state types
#[allow(unused_imports)] // load_state is used in tests and worker_pool module
pub use state::{
    check_max_iterations, find_rehoboam_dir, increment_iteration, init_loop_dir, load_state,
    save_state, LoopRole, LoopState, RehoboamConfig,
};

// Re-export task types and functions
pub use tasks::{read_pending_tasks, Task};

// Re-export judge types and functions
pub use judge::{judge_completion, JudgeDecision};

// Re-export prompt building
pub use prompts::{build_iteration_prompt, build_loop_context};

// Re-export activity logging
pub use activity::{
    check_completion, get_iteration_duration, log_activity, log_session_transition,
    mark_iteration_start, track_error_pattern,
};

// Re-export git checkpoint
pub use git_checkpoint::create_git_checkpoint;

use std::fs;
use std::path::Path;

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

// Re-export permission policy types and functions
pub use permissions::evaluate_permission;

#[cfg(test)]
mod tests {
    use super::coordination::{
        broadcast, join_existing_loop, list_workers, read_broadcasts, register_worker,
    };
    use super::permissions::{check_approval_memory, record_approval, PermissionDecision};
    use super::state::load_state;
    use super::tasks::{add_task, claim_task, complete_task, read_next_task, read_pending_tasks};
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_init_loop_dir() {
        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig {
            max_iterations: 10,
            stop_word: "COMPLETE".to_string(),
            pane_id: "%42".to_string(),
            role: LoopRole::Auto,
            enable_coordination: false,
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

    // Multi-Agent Coordination tests

    #[test]
    fn test_coordination_file_created() {
        let temp = TempDir::new().unwrap();
        // Coordination is opt-in per Cursor guidance
        let config = RehoboamConfig {
            enable_coordination: true,
            ..Default::default()
        };
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        assert!(loop_dir.join("coordination.md").exists());
    }

    #[test]
    fn test_coordination_file_not_created_by_default() {
        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        // Coordination is opt-in, so it shouldn't exist by default
        assert!(!loop_dir.join("coordination.md").exists());
    }

    #[test]
    fn test_broadcast() {
        use std::fs;

        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig {
            enable_coordination: true,
            ..Default::default()
        };
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        broadcast(&loop_dir, "worker-1", "Found API schema at /api/v1").unwrap();
        broadcast(&loop_dir, "worker-2", "Database migration complete").unwrap();

        let content = fs::read_to_string(loop_dir.join("coordination.md")).unwrap();
        assert!(content.contains("[worker-1]: Found API schema at /api/v1"));
        assert!(content.contains("[worker-2]: Database migration complete"));
    }

    #[test]
    fn test_read_broadcasts() {
        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        broadcast(&loop_dir, "agent-a", "Test message 1").unwrap();
        broadcast(&loop_dir, "agent-b", "Test message 2").unwrap();

        let broadcasts = read_broadcasts(&loop_dir, Some(60)).unwrap();
        assert_eq!(broadcasts.len(), 2);
    }

    #[test]
    fn test_register_worker() {
        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        register_worker(&loop_dir, "worker-1", "API endpoint worker").unwrap();

        // Check worker file created
        let worker_file = loop_dir.join("workers/worker-1.md");
        assert!(worker_file.exists());

        let content = std::fs::read_to_string(&worker_file).unwrap();
        assert!(content.contains("API endpoint worker"));

        // Check broadcast was sent
        let broadcasts = read_broadcasts(&loop_dir, Some(60)).unwrap();
        assert!(!broadcasts.is_empty());
    }

    #[test]
    fn test_list_workers() {
        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        register_worker(&loop_dir, "worker-a", "First worker").unwrap();
        register_worker(&loop_dir, "worker-b", "Second worker").unwrap();

        let workers = list_workers(&loop_dir).unwrap();
        assert_eq!(workers.len(), 2);
        assert!(workers.contains(&"worker-a".to_string()));
        assert!(workers.contains(&"worker-b".to_string()));
    }

    #[test]
    fn test_join_existing_loop() {
        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let _loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        // Should be able to join existing loop
        let joined_dir = join_existing_loop(temp.path()).unwrap();
        assert!(joined_dir.exists());

        // Should fail for non-existent loop
        let other_temp = TempDir::new().unwrap();
        assert!(join_existing_loop(other_temp.path()).is_err());
    }

    // Task Queue tests

    #[test]
    fn test_tasks_file_created() {
        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        assert!(loop_dir.join("tasks.md").exists());
    }

    #[test]
    fn test_add_and_read_tasks() {
        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        // Add tasks (newest first - LIFO order)
        add_task(&loop_dir, "TASK-001", "Implement user auth").unwrap();
        add_task(&loop_dir, "TASK-002", "Add API validation").unwrap();

        // Read pending tasks - newest task is at top
        let tasks = read_pending_tasks(&loop_dir).unwrap();
        assert_eq!(tasks.len(), 2);
        // Most recently added task is first
        assert_eq!(tasks[0].id, "TASK-002");
        assert_eq!(tasks[0].description, "Add API validation");
        assert_eq!(tasks[1].id, "TASK-001");
    }

    #[test]
    fn test_read_next_task() {
        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        // No tasks initially
        let task = read_next_task(&loop_dir).unwrap();
        assert!(task.is_none());

        // Add a task
        add_task(&loop_dir, "TASK-001", "First task").unwrap();

        // Should return the first task
        let task = read_next_task(&loop_dir).unwrap();
        assert!(task.is_some());
        assert_eq!(task.unwrap().id, "TASK-001");
    }

    #[test]
    fn test_claim_task() {
        use std::fs;

        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        // Add a task
        add_task(&loop_dir, "TASK-001", "Auth endpoint").unwrap();

        // Claim it
        claim_task(&loop_dir, "TASK-001", "%42").unwrap();

        // Should no longer be in pending
        let pending = read_pending_tasks(&loop_dir).unwrap();
        assert!(pending.is_empty());

        // Check the file content
        let content = fs::read_to_string(loop_dir.join("tasks.md")).unwrap();
        assert!(content.contains("In Progress"));
        assert!(content.contains("[TASK-001]"));
        assert!(content.contains("worker: %42"));
    }

    #[test]
    fn test_complete_task() {
        use std::fs;

        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        // Add and claim a task
        add_task(&loop_dir, "TASK-001", "Auth endpoint").unwrap();
        claim_task(&loop_dir, "TASK-001", "%42").unwrap();

        // Complete it
        complete_task(&loop_dir, "TASK-001").unwrap();

        // Check the file content
        let content = fs::read_to_string(loop_dir.join("tasks.md")).unwrap();
        assert!(content.contains("Completed"));
        assert!(content.contains("[x] [TASK-001]"));
    }

    #[test]
    fn test_role_specific_prompts() {
        use std::fs;

        let temp = TempDir::new().unwrap();

        // Test Planner role
        let planner_config = RehoboamConfig {
            role: LoopRole::Planner,
            ..Default::default()
        };
        let loop_dir = init_loop_dir(temp.path(), "Build API", &planner_config).unwrap();
        let prompt_file = build_iteration_prompt(&loop_dir).unwrap();
        let prompt = fs::read_to_string(&prompt_file).unwrap();
        assert!(prompt.contains("PLANNER"));
        assert!(prompt.contains("Do NOT implement anything yourself"));

        // Test Worker role
        let temp2 = TempDir::new().unwrap();
        let worker_config = RehoboamConfig {
            role: LoopRole::Worker,
            ..Default::default()
        };
        let loop_dir2 = init_loop_dir(temp2.path(), "Build API", &worker_config).unwrap();
        add_task(&loop_dir2, "TASK-001", "Build auth").unwrap();
        let prompt_file2 = build_iteration_prompt(&loop_dir2).unwrap();
        let prompt2 = fs::read_to_string(&prompt_file2).unwrap();
        assert!(prompt2.contains("WORKER"));
        assert!(prompt2.contains("TASK-001"));

        // Test Auto role (backward compatible)
        let temp3 = TempDir::new().unwrap();
        let auto_config = RehoboamConfig::default();
        let loop_dir3 = init_loop_dir(temp3.path(), "Build API", &auto_config).unwrap();
        let prompt_file3 = build_iteration_prompt(&loop_dir3).unwrap();
        let prompt3 = fs::read_to_string(&prompt_file3).unwrap();
        assert!(prompt3.contains("Rehoboam Loop - Iteration"));
        assert!(!prompt3.contains("PLANNER"));
        assert!(!prompt3.contains("WORKER"));
    }

    // Permission Policy Tests

    #[test]
    fn test_permission_policy_default_read_only_tools() {
        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        // Read-only tools should be auto-approved
        let read_only_tools = ["Read", "Glob", "Grep", "WebFetch", "WebSearch", "Task"];
        for tool in &read_only_tools {
            let decision = evaluate_permission(&loop_dir, tool, None, None);
            assert_eq!(
                decision,
                PermissionDecision::Approve,
                "{} should be auto-approved",
                tool
            );
        }
    }

    #[test]
    fn test_permission_policy_bash_allow_patterns() {
        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        // Table-driven test for bash allow patterns
        let cases = vec![
            ("git status", true, "git status should be allowed"),
            ("git diff --cached", true, "git diff should be allowed"),
            ("cargo test --release", true, "cargo test should be allowed"),
            ("cargo clippy", true, "cargo clippy should be allowed"),
            ("ls -la", true, "ls should be allowed"),
            ("cat README.md", true, "cat should be allowed"),
            ("my-command --help", true, "--help should be allowed"),
            ("npm test", true, "npm test should be allowed"),
        ];

        for (command, should_approve, desc) in cases {
            let input = serde_json::json!({ "command": command });
            let decision = evaluate_permission(&loop_dir, "Bash", Some(&input), None);

            if should_approve {
                assert_eq!(decision, PermissionDecision::Approve, "{}", desc);
            }
        }
    }

    #[test]
    fn test_permission_policy_bash_deny_patterns() {
        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        // Dangerous commands should be denied
        let deny_cases = vec![
            ("rm -rf /", "rm -rf should be denied"),
            ("sudo apt install foo", "sudo should be denied"),
            (
                "git push --force origin main",
                "force push should be denied",
            ),
            ("chmod 777 /etc/passwd", "chmod 777 should be denied"),
        ];

        for (command, desc) in deny_cases {
            let input = serde_json::json!({ "command": command });
            let decision = evaluate_permission(&loop_dir, "Bash", Some(&input), None);
            assert_eq!(decision, PermissionDecision::Deny, "{}", desc);
        }
    }

    #[test]
    fn test_permission_policy_unknown_tool_defers() {
        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        // Unknown tools should defer to user
        let decision = evaluate_permission(&loop_dir, "UnknownTool", None, None);
        assert_eq!(
            decision,
            PermissionDecision::Defer,
            "Unknown tool should defer"
        );

        // Edit/Write without approval memory should defer
        let edit_input = serde_json::json!({ "file_path": "/some/path.rs" });
        let decision = evaluate_permission(&loop_dir, "Edit", Some(&edit_input), None);
        assert_eq!(
            decision,
            PermissionDecision::Defer,
            "Edit should defer without approval memory"
        );
    }

    #[test]
    fn test_permission_policy_custom_policy() {
        use std::fs;

        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        // Create a custom policy that allows Edit
        let policy_content = r#"
[auto_approve]
always = ["Edit", "Write"]
bash_allow = []
bash_deny = []
"#;
        fs::write(loop_dir.join("policy.toml"), policy_content).unwrap();

        // Edit should now be auto-approved
        let decision = evaluate_permission(&loop_dir, "Edit", None, None);
        assert_eq!(
            decision,
            PermissionDecision::Approve,
            "Edit should be approved with custom policy"
        );
    }

    #[test]
    fn test_approval_memory() {
        let temp = TempDir::new().unwrap();
        let config = RehoboamConfig::default();
        let loop_dir = init_loop_dir(temp.path(), "Test", &config).unwrap();

        // Record an approval
        record_approval(&loop_dir, "Edit", Some("/path/to/file.rs")).unwrap();

        // Check that the approval is remembered
        let found = check_approval_memory(&loop_dir, "Edit", Some("/path/to/file.rs"), 24);
        assert!(found, "Should find recent approval");

        // Different path should not be found
        let not_found = check_approval_memory(&loop_dir, "Edit", Some("/different/path.rs"), 24);
        assert!(!not_found, "Should not find approval for different path");
    }
}
