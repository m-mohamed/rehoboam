//! Worker Pool for Auto-Spawning (Cursor Isolation Model)
//!
//! When a Planner writes "PLANNING COMPLETE" to progress.md, the TUI
//! automatically spawns Worker agents to execute tasks in parallel.
//!
//! Each Worker operates in complete isolation via git worktrees:
//! - Gets ONE pre-assigned task (via assigned_task.md)
//! - Works in its own git worktree (branch: worker/{task_id})
//! - Each worktree has its own .rehoboam/ directory
//! - No shared tasks.md - prevents race conditions
//! - Judge evaluates completion → next iteration or done
//!
//! Git worktrees provide:
//! - Full git capabilities per worker
//! - Workers can commit to their own branch
//! - Standard .rehoboam/ discovery (no REHOBOAM_LOOP_DIR needed)
//! - Clean isolation with easy cleanup

use crate::git::GitController;
use crate::rehoboam_loop::{self, LoopRole, LoopState, Task};
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
    /// Pre-assigned task ID
    pub task_id: String,
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
    /// Active workers: task_id -> WorkerInfo
    pub workers: HashMap<String, WorkerInfo>,
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

    /// Spawn workers for pending tasks
    ///
    /// Returns the pane IDs of spawned workers
    #[allow(dead_code)]
    pub fn spawn_workers(&mut self, tasks: &[Task]) -> Result<Vec<String>> {
        let to_spawn = tasks.len().min(self.max_workers);
        let mut pane_ids = Vec::new();

        info!(
            count = to_spawn,
            max = self.max_workers,
            pending = tasks.len(),
            "Spawning worker pool"
        );

        for (i, task) in tasks.iter().take(to_spawn).enumerate() {
            // Rate limit: delay between spawns (except first)
            if i > 0 {
                std::thread::sleep(Duration::from_millis(SPAWN_DELAY_MS));
            }

            match spawn_worker(&self.parent_loop_dir, task) {
                Ok(worker_info) => {
                    let pane_id = worker_info.pane_id.clone();
                    let task_id = worker_info.task_id.clone();
                    self.workers.insert(task_id.clone(), worker_info);
                    pane_ids.push(pane_id.clone());
                    info!(
                        pane_id = %pane_id,
                        task_id = %task_id,
                        worker_index = i,
                        "Worker spawned"
                    );
                }
                Err(e) => {
                    warn!(
                        task_id = %task.id,
                        error = %e,
                        "Failed to spawn worker"
                    );
                }
            }
        }

        Ok(pane_ids)
    }
}

/// Spawn a single isolated worker for a task using git worktrees
///
/// Creates:
/// - Git worktree with branch: worker/{task_id}
/// - .rehoboam/ directory inside the worktree
/// - assigned_task.md with only this worker's task
/// - state.json with role=Worker and assigned_task set
fn spawn_worker(parent_loop_dir: &Path, task: &Task) -> Result<WorkerInfo> {
    let project_dir = parent_loop_dir
        .parent()
        .ok_or_else(|| eyre!("Invalid parent loop directory"))?;

    // Create git worktree for this worker
    let git = GitController::new(project_dir.to_path_buf());

    // Branch name: worker/{task_id} (lowercase, sanitized)
    let branch_name = format!("worker/{}", task.id.to_lowercase());

    debug!(
        branch = %branch_name,
        task_id = %task.id,
        "Creating git worktree for worker"
    );

    // Create worktree (this creates a sibling directory)
    let worktree_path = git.create_worktree(&branch_name)?;

    // Worker's .rehoboam/ is inside the worktree (standard location)
    let worker_loop_dir = worktree_path.join(".rehoboam");

    debug!(
        worktree = %worktree_path.display(),
        loop_dir = %worker_loop_dir.display(),
        "Git worktree created for worker"
    );

    // Initialize worker state with pre-assigned task
    init_worker_dir(&worker_loop_dir, parent_loop_dir, &worktree_path, task)?;

    // Build iteration prompt for worker
    let prompt_file = rehoboam_loop::build_iteration_prompt(&worker_loop_dir)?;

    // Spawn tmux pane with Claude in the worktree directory
    // No REHOBOAM_LOOP_DIR needed - standard .rehoboam/ discovery works
    let worktree_str = worktree_path.to_string_lossy().to_string();
    let pane_id = TmuxController::respawn_claude(&worktree_str, &prompt_file)?;

    Ok(WorkerInfo {
        pane_id,
        task_id: task.id.clone(),
        worktree_path,
        worker_loop_dir,
        branch: branch_name,
        status: WorkerStatus::Provisioning,
    })
}

/// Initialize a worker's isolated loop directory inside a git worktree
///
/// Copies context from parent Planner but creates isolated assigned_task.md
///
/// # Arguments
/// * `worker_loop_dir` - The .rehoboam/ directory inside the worker's worktree
/// * `parent_loop_dir` - The parent Planner's .rehoboam/ directory
/// * `worktree_path` - The worker's git worktree directory (project_dir for worker)
/// * `assigned_task` - The task assigned to this worker
fn init_worker_dir(
    worker_loop_dir: &Path,
    parent_loop_dir: &Path,
    worktree_path: &Path,
    assigned_task: &Task,
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

    // Create worker-specific assigned_task.md (NOT shared tasks.md)
    // Worker only sees their ONE task - Cursor isolation model
    let task_content = format!(
        r#"# Assigned Task

## Your Task
- [ ] [{}] {}

## Instructions
Complete this task, then write DONE to progress.md.
Do NOT explore beyond what's needed for this task.
Do NOT coordinate with other workers.
"#,
        assigned_task.id, assigned_task.description
    );
    fs::write(worker_loop_dir.join("assigned_task.md"), task_content)?;

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
        assigned_task: Some(assigned_task.id.clone()),
    };
    rehoboam_loop::save_state(worker_loop_dir, &state)?;

    // Create empty progress.md
    let progress_content = format!(
        r#"# Worker Progress

## Working on: [{}] {}

## Status
Starting...
"#,
        assigned_task.id, assigned_task.description
    );
    fs::write(worker_loop_dir.join("progress.md"), progress_content)?;

    // Create empty logs
    fs::write(worker_loop_dir.join("errors.log"), "")?;
    fs::write(worker_loop_dir.join("activity.log"), "")?;
    fs::write(worker_loop_dir.join("session_history.log"), "")?;

    debug!(
        worker_loop_dir = %worker_loop_dir.display(),
        worktree = %worktree_path.display(),
        task_id = %assigned_task.id,
        "Worker directory initialized in git worktree"
    );

    Ok(())
}

/// Spawn worker pool for a completed Planner
///
/// This is the main entry point called from event_processing.rs.
/// Returns WorkerInfo for each spawned worker so they can be registered with the TUI.
pub fn spawn_worker_pool_for_planner(
    parent_loop_dir: &Path,
    max_workers: usize,
) -> Result<Vec<WorkerInfo>> {
    // Read pending tasks from Planner's tasks.md
    let pending_tasks = rehoboam_loop::read_pending_tasks(parent_loop_dir)?;

    if pending_tasks.is_empty() {
        info!("No pending tasks to spawn workers for");
        return Ok(Vec::new());
    }

    info!(
        pending = pending_tasks.len(),
        max_workers = max_workers,
        "Planning complete, spawning workers"
    );

    let to_spawn = pending_tasks.len().min(max_workers);
    let mut workers = Vec::new();

    for (i, task) in pending_tasks.iter().take(to_spawn).enumerate() {
        // Rate limit: delay between spawns (except first)
        if i > 0 {
            std::thread::sleep(Duration::from_millis(SPAWN_DELAY_MS));
        }

        match spawn_worker(parent_loop_dir, task) {
            Ok(worker_info) => {
                info!(
                    pane_id = %worker_info.pane_id,
                    task_id = %worker_info.task_id,
                    worker_index = i,
                    "Worker spawned"
                );
                workers.push(worker_info);
            }
            Err(e) => {
                warn!(
                    task_id = %task.id,
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
        let worktree_path = temp.path().join("project-worker-task-001");
        fs::create_dir_all(&worktree_path).unwrap();

        // Worker's .rehoboam/ inside the worktree
        let worker_loop_dir = worktree_path.join(".rehoboam");

        let task = Task {
            id: "TASK-001".to_string(),
            description: "Implement auth endpoint".to_string(),
            worker: None,
        };

        init_worker_dir(&worker_loop_dir, &parent_loop_dir, &worktree_path, &task).unwrap();

        // Check files created in worker's .rehoboam/
        assert!(worker_loop_dir.join("anchor.md").exists());
        assert!(worker_loop_dir.join("guardrails.md").exists());
        assert!(worker_loop_dir.join("assigned_task.md").exists());
        assert!(worker_loop_dir.join("state.json").exists());
        assert!(worker_loop_dir.join("progress.md").exists());

        // Check assigned_task.md content
        let content = fs::read_to_string(worker_loop_dir.join("assigned_task.md")).unwrap();
        assert!(content.contains("TASK-001"));
        assert!(content.contains("Implement auth endpoint"));

        // Check state.json
        let state = rehoboam_loop::load_state(&worker_loop_dir).unwrap();
        assert_eq!(state.role, LoopRole::Worker);
        assert_eq!(state.assigned_task, Some("TASK-001".to_string()));
        assert_eq!(state.max_iterations, 10);
        // Verify project_dir points to the worktree
        assert_eq!(state.project_dir, worktree_path);
    }
}
