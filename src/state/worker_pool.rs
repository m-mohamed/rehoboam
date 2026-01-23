//! Worker Pool for Auto-Spawning (Claude Code Tasks API Model)
//!
//! When a Planner writes "PLANNING COMPLETE" to progress.md, the TUI
//! automatically spawns Worker agents to execute tasks in parallel.
//!
//! Workers use Claude Code Tasks API for task coordination:
//! - Use `TaskList` to find pending tasks
//! - Use `TaskUpdate` to claim tasks (set status: in_progress)
//! - Use `TaskUpdate` to mark tasks completed
//! - No pre-assignment - workers self-organize via Tasks API
//!
//! Each Worker operates in complete isolation via git worktrees:
//! - Works in its own git worktree (branch: worker-{index})
//! - Each worktree has its own .rehoboam/ directory
//! - All workers share same CLAUDE_CODE_TASK_LIST_ID
//! - Judge evaluates completion via TaskList status
//!
//! **REQUIRES**: CLAUDE_CODE_TASK_LIST_ID environment variable

use crate::git::GitController;
use crate::rehoboam_loop::{self, LoopRole, LoopState};
use crate::tmux::TmuxController;
use color_eyre::eyre::{eyre, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{debug, info, warn};

/// Maximum concurrent workers (default for Claude Code Max)
#[allow(dead_code)]
pub const DEFAULT_MAX_WORKERS: usize = 3;

/// Delay between spawning workers (rate limiting)
const SPAWN_DELAY_MS: u64 = 1000;

/// Status of a spawned worker
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum WorkerStatus {
    /// Worker is being spawned
    Provisioning,
    /// Worker is actively working on task
    Working,
    /// Worker completed task successfully
    Complete,
    /// Worker failed or stalled
    Failed,
}

/// Information about a spawned worker
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WorkerInfo {
    /// Tmux pane ID for this worker
    pub pane_id: String,
    /// Worker index (1, 2, 3, ...)
    pub worker_index: usize,
    /// Worker's git worktree directory (contains .rehoboam/)
    pub worktree_path: PathBuf,
    /// Worker's loop directory (.rehoboam/ inside worktree)
    pub worker_loop_dir: PathBuf,
    /// Git branch name for this worker
    pub branch: String,
    /// Current status
    pub status: WorkerStatus,
}

/// Worker pool for managing auto-spawned workers
#[derive(Debug)]
#[allow(dead_code)]
pub struct WorkerPool {
    /// Maximum concurrent workers
    pub max_workers: usize,
    /// Active workers: worker_index -> WorkerInfo
    pub workers: HashMap<usize, WorkerInfo>,
    /// Parent Planner's loop directory
    pub parent_loop_dir: PathBuf,
    /// Project directory (same for all workers)
    pub project_dir: PathBuf,
}

impl WorkerPool {
    /// Create a new worker pool for a Planner's loop directory
    #[allow(dead_code)]
    pub fn new(parent_loop_dir: PathBuf, max_workers: usize) -> Self {
        let project_dir = parent_loop_dir
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));

        Self {
            max_workers,
            workers: HashMap::new(),
            parent_loop_dir,
            project_dir,
        }
    }

    /// Spawn workers (up to max_workers)
    ///
    /// Workers will use TaskList to find and claim tasks themselves.
    /// Returns the pane IDs of spawned workers.
    #[allow(dead_code)]
    pub fn spawn_workers(&mut self, count: usize) -> Result<Vec<String>> {
        let to_spawn = count.min(self.max_workers);
        let mut pane_ids = Vec::new();

        info!(
            count = to_spawn,
            max = self.max_workers,
            "Spawning worker pool (workers will claim tasks via TaskList)"
        );

        for i in 0..to_spawn {
            // Rate limit: delay between spawns (except first)
            if i > 0 {
                std::thread::sleep(Duration::from_millis(SPAWN_DELAY_MS));
            }

            let worker_index = i + 1;
            match spawn_worker(&self.parent_loop_dir, worker_index) {
                Ok(worker_info) => {
                    let pane_id = worker_info.pane_id.clone();
                    self.workers.insert(worker_index, worker_info);
                    pane_ids.push(pane_id.clone());
                    info!(
                        pane_id = %pane_id,
                        worker_index = worker_index,
                        "Worker spawned (will claim task via TaskList)"
                    );
                }
                Err(e) => {
                    warn!(
                        worker_index = worker_index,
                        error = %e,
                        "Failed to spawn worker"
                    );
                }
            }
        }

        Ok(pane_ids)
    }
}

/// Spawn a single isolated worker using git worktrees
///
/// Workers use Claude Code Tasks API to claim their own tasks.
/// Creates:
/// - Git worktree with branch: worker-{index}
/// - .rehoboam/ directory inside the worktree
/// - state.json with role=Worker
/// - CLAUDE_CODE_TASK_LIST_ID inherited from parent
fn spawn_worker(parent_loop_dir: &Path, worker_index: usize) -> Result<WorkerInfo> {
    // Verify Tasks API is configured
    let task_list_id = std::env::var("CLAUDE_CODE_TASK_LIST_ID").map_err(|_| {
        eyre!("CLAUDE_CODE_TASK_LIST_ID not set. Workers require Claude Code Tasks API.")
    })?;

    let project_dir = parent_loop_dir
        .parent()
        .ok_or_else(|| eyre!("Invalid parent loop directory"))?;

    // Create git worktree for this worker
    let git = GitController::new(project_dir.to_path_buf());

    // Branch name: worker-{index}
    let branch_name = format!("worker-{}", worker_index);

    debug!(
        branch = %branch_name,
        worker_index = worker_index,
        "Creating git worktree for worker"
    );

    // Create worktree (this creates a sibling directory)
    let worktree_path = git.create_worktree(&branch_name)?;

    // Worker's .rehoboam/ is inside the worktree (standard location)
    let worker_loop_dir = worktree_path.join(".rehoboam");

    debug!(
        worktree = %worktree_path.display(),
        loop_dir = %worker_loop_dir.display(),
        task_list_id = %task_list_id,
        "Git worktree created for worker"
    );

    // Initialize worker state (no pre-assigned task - worker will use TaskList)
    init_worker_dir(
        &worker_loop_dir,
        parent_loop_dir,
        &worktree_path,
        worker_index,
    )?;

    // Build iteration prompt for worker
    let prompt_file = rehoboam_loop::build_iteration_prompt(&worker_loop_dir)?;

    // Spawn tmux pane with Claude in the worktree directory
    // Pass CLAUDE_CODE_TASK_LIST_ID so worker can access shared task list
    let worktree_str = worktree_path.to_string_lossy().to_string();
    let env_vars = vec![("CLAUDE_CODE_TASK_LIST_ID", task_list_id.as_str())];
    let pane_id = TmuxController::respawn_claude_with_env(&worktree_str, &prompt_file, &env_vars)?;

    Ok(WorkerInfo {
        pane_id,
        worker_index,
        worktree_path,
        worker_loop_dir,
        branch: branch_name,
        status: WorkerStatus::Provisioning,
    })
}

/// Initialize a worker's isolated loop directory inside a git worktree
///
/// Workers use Claude Code Tasks API to claim their own tasks.
/// No pre-assigned task - worker will use TaskList/TaskUpdate.
///
/// # Arguments
/// * `worker_loop_dir` - The .rehoboam/ directory inside the worker's worktree
/// * `parent_loop_dir` - The parent Planner's .rehoboam/ directory
/// * `worktree_path` - The worker's git worktree directory (project_dir for worker)
/// * `worker_index` - The worker's index (1, 2, 3, ...)
fn init_worker_dir(
    worker_loop_dir: &Path,
    parent_loop_dir: &Path,
    worktree_path: &Path,
    worker_index: usize,
) -> Result<()> {
    // Create .rehoboam/ inside the worktree
    fs::create_dir_all(worker_loop_dir)?;

    // Copy shared context (read-only for worker)
    let anchor_src = parent_loop_dir.join("anchor.md");
    let guardrails_src = parent_loop_dir.join("guardrails.md");

    if anchor_src.exists() {
        fs::copy(&anchor_src, worker_loop_dir.join("anchor.md"))?;
    }
    if guardrails_src.exists() {
        fs::copy(&guardrails_src, worker_loop_dir.join("guardrails.md"))?;
    }

    // NOTE: No assigned_task.md - worker will use TaskList to claim tasks
    // This is the Claude Code Tasks API model

    // Create worker state.json with worktree as project_dir
    // This enables git operations in the worker's worktree
    let state = LoopState {
        iteration: 0,
        max_iterations: 10, // Workers should finish in fewer iterations
        stop_word: "DONE".to_string(),
        started_at: chrono::Utc::now(),
        pane_id: String::new(),                   // Will be set when we spawn
        project_dir: worktree_path.to_path_buf(), // Worker's git worktree
        iteration_started_at: Some(chrono::Utc::now()),
        error_counts: std::collections::HashMap::new(),
        last_commit: None,
        role: LoopRole::Worker,
        assigned_task: None, // Workers claim tasks via TaskList/TaskUpdate
    };
    rehoboam_loop::save_state(worker_loop_dir, &state)?;

    // Create progress.md (worker will update after claiming a task)
    let progress_content = format!(
        r#"# Worker {} Progress

## Status
Starting... Use TaskList to find a pending task, then TaskUpdate to claim it.
"#,
        worker_index
    );
    fs::write(worker_loop_dir.join("progress.md"), progress_content)?;

    // Create empty logs
    fs::write(worker_loop_dir.join("errors.log"), "")?;
    fs::write(worker_loop_dir.join("activity.log"), "")?;
    fs::write(worker_loop_dir.join("session_history.log"), "")?;

    debug!(
        worker_loop_dir = %worker_loop_dir.display(),
        worktree = %worktree_path.display(),
        worker_index = worker_index,
        "Worker directory initialized in git worktree (will claim task via TaskList)"
    );

    Ok(())
}

/// Spawn worker pool for a completed Planner
///
/// This is the main entry point called from event_processing.rs.
/// Workers will use Claude Code Tasks API to claim their own tasks.
/// Returns WorkerInfo for each spawned worker so they can be registered with the TUI.
///
/// **REQUIRES**: CLAUDE_CODE_TASK_LIST_ID environment variable
pub fn spawn_worker_pool_for_planner(
    parent_loop_dir: &Path,
    max_workers: usize,
) -> Result<Vec<WorkerInfo>> {
    // Verify Tasks API is configured
    if std::env::var("CLAUDE_CODE_TASK_LIST_ID").is_err() {
        return Err(eyre!(
            "CLAUDE_CODE_TASK_LIST_ID not set. Worker spawning requires Claude Code Tasks API."
        ));
    }

    info!(
        max_workers = max_workers,
        "Planning complete, spawning workers (will claim tasks via TaskList)"
    );

    let mut workers = Vec::new();

    for i in 0..max_workers {
        // Rate limit: delay between spawns (except first)
        if i > 0 {
            std::thread::sleep(Duration::from_millis(SPAWN_DELAY_MS));
        }

        let worker_index = i + 1;
        match spawn_worker(parent_loop_dir, worker_index) {
            Ok(worker_info) => {
                info!(
                    pane_id = %worker_info.pane_id,
                    worker_index = worker_index,
                    "Worker spawned (will claim task via TaskList)"
                );
                workers.push(worker_info);
            }
            Err(e) => {
                warn!(
                    worker_index = worker_index,
                    error = %e,
                    "Failed to spawn worker"
                );
            }
        }
    }

    Ok(workers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_worker_pool_new() {
        let temp = TempDir::new().unwrap();
        let pool = WorkerPool::new(temp.path().to_path_buf(), 3);

        assert_eq!(pool.max_workers, 3);
        assert!(pool.workers.is_empty());
    }

    #[test]
    fn test_init_worker_dir() {
        let temp = TempDir::new().unwrap();

        // Simulate parent Planner's .rehoboam/ directory
        let parent_loop_dir = temp.path().join(".rehoboam");
        fs::create_dir_all(&parent_loop_dir).unwrap();

        // Create parent files
        fs::write(parent_loop_dir.join("anchor.md"), "# Task\nBuild API").unwrap();
        fs::write(parent_loop_dir.join("guardrails.md"), "# Guardrails").unwrap();

        // Simulate worker's worktree directory (sibling to main project)
        let worktree_path = temp.path().join("project-worker-1");
        fs::create_dir_all(&worktree_path).unwrap();

        // Worker's .rehoboam/ inside the worktree
        let worker_loop_dir = worktree_path.join(".rehoboam");

        init_worker_dir(&worker_loop_dir, &parent_loop_dir, &worktree_path, 1).unwrap();

        // Check files created in worker's .rehoboam/
        assert!(worker_loop_dir.join("anchor.md").exists());
        assert!(worker_loop_dir.join("guardrails.md").exists());
        assert!(worker_loop_dir.join("state.json").exists());
        assert!(worker_loop_dir.join("progress.md").exists());

        // NOTE: No assigned_task.md - workers use TaskList to claim tasks

        // Check state.json
        let state = rehoboam_loop::load_state(&worker_loop_dir).unwrap();
        assert_eq!(state.role, LoopRole::Worker);
        assert_eq!(state.assigned_task, None); // Workers claim via TaskList
        assert_eq!(state.max_iterations, 10);
        // Verify project_dir points to the worktree
        assert_eq!(state.project_dir, worktree_path);
    }
}
