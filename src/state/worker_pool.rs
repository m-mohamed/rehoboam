//! Worker Pool for Auto-Spawning (Cursor Isolation Model)
//!
//! When a Planner writes "PLANNING COMPLETE" to progress.md, the TUI
//! automatically spawns Worker agents to execute tasks in parallel.
//!
//! Each Worker operates in complete isolation:
//! - Gets ONE pre-assigned task (via assigned_task.md)
//! - Works in its own loop directory (.rehoboam-worker-{task_id}/)
//! - No shared tasks.md - prevents race conditions
//! - Judge evaluates completion → next iteration or done

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
    /// Worker's isolated loop directory
    pub worker_loop_dir: PathBuf,
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

/// Spawn a single isolated worker for a task
///
/// Creates:
/// - Worker-specific loop directory: .rehoboam-worker-{task_id}/
/// - assigned_task.md with only this worker's task
/// - state.json with role=Worker and assigned_task set
fn spawn_worker(parent_loop_dir: &Path, task: &Task) -> Result<WorkerInfo> {
    let project_dir = parent_loop_dir
        .parent()
        .ok_or_else(|| eyre!("Invalid parent loop directory"))?;

    // Create worker-specific loop directory
    let worker_dir = project_dir.join(format!(".rehoboam-worker-{}", task.id.to_lowercase()));

    debug!(
        worker_dir = %worker_dir.display(),
        task_id = %task.id,
        "Initializing worker directory"
    );

    // Initialize worker state with pre-assigned task
    init_worker_dir(&worker_dir, parent_loop_dir, task)?;

    // Build iteration prompt for worker
    let prompt_file = rehoboam_loop::build_iteration_prompt(&worker_dir)?;

    // Spawn tmux pane with Claude, setting REHOBOAM_LOOP_DIR for worker isolation
    let project_dir_str = project_dir.to_string_lossy().to_string();
    let pane_id = TmuxController::respawn_claude_with_loop_dir(
        &project_dir_str,
        &prompt_file,
        Some(&worker_dir),
    )?;

    Ok(WorkerInfo {
        pane_id,
        task_id: task.id.clone(),
        worker_loop_dir: worker_dir,
        status: WorkerStatus::Provisioning,
    })
}

/// Initialize a worker's isolated loop directory
///
/// Copies context from parent Planner but creates isolated assigned_task.md
fn init_worker_dir(worker_dir: &Path, parent_dir: &Path, assigned_task: &Task) -> Result<()> {
    // Create directory (may already exist from previous run)
    fs::create_dir_all(worker_dir)?;

    // Copy shared context (read-only for worker)
    let anchor_src = parent_dir.join("anchor.md");
    let guardrails_src = parent_dir.join("guardrails.md");

    if anchor_src.exists() {
        fs::copy(&anchor_src, worker_dir.join("anchor.md"))?;
    }
    if guardrails_src.exists() {
        fs::copy(&guardrails_src, worker_dir.join("guardrails.md"))?;
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
    fs::write(worker_dir.join("assigned_task.md"), task_content)?;

    // Create worker state.json with assigned task ID
    let project_dir = parent_dir
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    let state = LoopState {
        iteration: 0,
        max_iterations: 10, // Workers should finish in fewer iterations
        stop_word: "DONE".to_string(),
        started_at: chrono::Utc::now(),
        pane_id: String::new(), // Will be set when we spawn
        project_dir,
        iteration_started_at: Some(chrono::Utc::now()),
        error_counts: std::collections::HashMap::new(),
        last_commit: None,
        role: LoopRole::Worker,
        assigned_task: Some(assigned_task.id.clone()),
    };
    rehoboam_loop::save_state(worker_dir, &state)?;

    // Create empty progress.md
    let progress_content = format!(
        r#"# Worker Progress

## Working on: [{}] {}

## Status
Starting...
"#,
        assigned_task.id, assigned_task.description
    );
    fs::write(worker_dir.join("progress.md"), progress_content)?;

    // Create empty logs
    fs::write(worker_dir.join("errors.log"), "")?;
    fs::write(worker_dir.join("activity.log"), "")?;
    fs::write(worker_dir.join("session_history.log"), "")?;

    debug!(
        worker_dir = %worker_dir.display(),
        task_id = %assigned_task.id,
        "Worker directory initialized"
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
        let parent_dir = temp.path().join(".rehoboam");
        fs::create_dir_all(&parent_dir).unwrap();

        // Create parent files
        fs::write(parent_dir.join("anchor.md"), "# Task\nBuild API").unwrap();
        fs::write(parent_dir.join("guardrails.md"), "# Guardrails").unwrap();

        let worker_dir = temp.path().join(".rehoboam-worker-task-001");
        let task = Task {
            id: "TASK-001".to_string(),
            description: "Implement auth endpoint".to_string(),
            worker: None,
        };

        init_worker_dir(&worker_dir, &parent_dir, &task).unwrap();

        // Check files created
        assert!(worker_dir.join("anchor.md").exists());
        assert!(worker_dir.join("guardrails.md").exists());
        assert!(worker_dir.join("assigned_task.md").exists());
        assert!(worker_dir.join("state.json").exists());
        assert!(worker_dir.join("progress.md").exists());

        // Check assigned_task.md content
        let content = fs::read_to_string(worker_dir.join("assigned_task.md")).unwrap();
        assert!(content.contains("TASK-001"));
        assert!(content.contains("Implement auth endpoint"));

        // Check state.json
        let state = rehoboam_loop::load_state(&worker_dir).unwrap();
        assert_eq!(state.role, LoopRole::Worker);
        assert_eq!(state.assigned_task, Some("TASK-001".to_string()));
        assert_eq!(state.max_iterations, 10);
    }
}
