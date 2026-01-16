//! Sprite checkpoint and pool management
//!
//! v1.5: Adds distributed sprite swarms for parallel task execution.
//! Supports hybrid mode with local planner and remote sprite workers.
//!
//! Note: Pool management APIs are prepared for future integration.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;

/// A checkpoint in the timeline
#[derive(Debug, Clone)]
pub struct CheckpointRecord {
    /// Checkpoint ID from Sprites API
    pub id: String,

    /// User-provided comment/description
    pub comment: String,

    /// When the checkpoint was created (Unix timestamp in seconds)
    pub created_at: i64,

    /// Loop iteration at time of checkpoint (0 if not in loop mode)
    pub iteration: u32,
}

impl From<sprites::Checkpoint> for CheckpointRecord {
    fn from(cp: sprites::Checkpoint) -> Self {
        Self {
            id: cp.id,
            comment: cp.comment.unwrap_or_default(),
            created_at: cp.created_at.map(|dt| dt.timestamp()).unwrap_or(0),
            iteration: 0,
        }
    }
}

// ============================================================================
// v1.5: Sprite Pool Management
// ============================================================================

/// Status of a sprite worker
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpriteWorkerStatus {
    /// Worker is being created
    Provisioning,
    /// Worker is ready and waiting for tasks
    Idle,
    /// Worker is actively processing a task
    Working,
    /// Worker is creating a checkpoint
    Checkpointing,
    /// Worker has completed its task
    Completed,
    /// Worker encountered an error
    Failed,
    /// Worker is being destroyed
    Terminating,
}

/// A sprite worker in the pool
#[derive(Debug, Clone)]
pub struct SpriteWorker {
    /// Unique worker ID (e.g., "worker-1", "worker-abc123")
    pub id: String,

    /// Sprite name in Fly.io
    pub sprite_name: String,

    /// Current status
    pub status: SpriteWorkerStatus,

    /// Task description (if any)
    pub task_description: Option<String>,

    /// Ralph directory for coordination
    pub ralph_dir: Option<PathBuf>,

    /// Loop iteration counter
    pub iteration: u32,

    /// Last checkpoint ID (if any)
    pub last_checkpoint: Option<String>,

    /// Created timestamp (Unix seconds)
    pub created_at: i64,

    /// Last activity timestamp (Unix seconds)
    pub last_activity: i64,
}

impl SpriteWorker {
    /// Create a new sprite worker
    pub fn new(id: String, sprite_name: String) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id,
            sprite_name,
            status: SpriteWorkerStatus::Provisioning,
            task_description: None,
            ralph_dir: None,
            iteration: 0,
            last_checkpoint: None,
            created_at: now,
            last_activity: now,
        }
    }

    /// Mark worker as ready
    pub fn mark_ready(&mut self) {
        self.status = SpriteWorkerStatus::Idle;
        self.last_activity = chrono::Utc::now().timestamp();
    }

    /// Start working on a task
    pub fn start_task(&mut self, description: &str, ralph_dir: Option<PathBuf>) {
        self.status = SpriteWorkerStatus::Working;
        self.task_description = Some(description.to_string());
        self.ralph_dir = ralph_dir;
        self.last_activity = chrono::Utc::now().timestamp();
    }

    /// Complete current task
    pub fn complete_task(&mut self) {
        self.status = SpriteWorkerStatus::Completed;
        self.last_activity = chrono::Utc::now().timestamp();
    }

    /// Mark as failed
    pub fn fail(&mut self, reason: &str) {
        self.status = SpriteWorkerStatus::Failed;
        self.task_description = Some(format!("Failed: {}", reason));
        self.last_activity = chrono::Utc::now().timestamp();
    }
}

/// Configuration for a sprite pool
#[derive(Debug, Clone)]
pub struct SpritePoolConfig {
    /// Maximum number of concurrent workers
    pub max_workers: usize,

    /// RAM allocation per worker (MB)
    pub ram_mb: u32,

    /// CPU allocation per worker
    pub cpus: u32,

    /// Whether to use checkpoints for resumption
    pub use_checkpoints: bool,

    /// Base name prefix for workers
    pub name_prefix: String,
}

impl Default for SpritePoolConfig {
    fn default() -> Self {
        Self {
            max_workers: 4,
            ram_mb: 2048,
            cpus: 2,
            use_checkpoints: true,
            name_prefix: "rehoboam".to_string(),
        }
    }
}

/// A pool of sprite workers for parallel task execution
///
/// This manages a fleet of sprites for distributed processing.
/// Workers can share state via the coordination.md file.
#[derive(Debug, Clone)]
pub struct SpritePool {
    /// Pool configuration
    pub config: SpritePoolConfig,

    /// Active workers in the pool
    pub workers: HashMap<String, SpriteWorker>,

    /// Ralph directory for shared coordination (if any)
    pub shared_ralph_dir: Option<PathBuf>,

    /// Whether the pool is in hybrid mode (local planner + remote workers)
    pub hybrid_mode: bool,

    /// Local planner pane ID (for hybrid mode)
    pub local_planner_pane: Option<String>,
}

impl SpritePool {
    /// Create a new sprite pool
    pub fn new(config: SpritePoolConfig) -> Self {
        Self {
            config,
            workers: HashMap::new(),
            shared_ralph_dir: None,
            hybrid_mode: false,
            local_planner_pane: None,
        }
    }

    /// Enable hybrid mode with local planner
    pub fn set_hybrid_mode(&mut self, planner_pane: &str, ralph_dir: PathBuf) {
        self.hybrid_mode = true;
        self.local_planner_pane = Some(planner_pane.to_string());
        self.shared_ralph_dir = Some(ralph_dir);
    }

    /// Add a worker to the pool
    pub fn add_worker(&mut self, worker: SpriteWorker) {
        self.workers.insert(worker.id.clone(), worker);
    }

    /// Remove a worker from the pool
    pub fn remove_worker(&mut self, worker_id: &str) -> Option<SpriteWorker> {
        self.workers.remove(worker_id)
    }

    /// Get a worker by ID
    pub fn get_worker(&self, worker_id: &str) -> Option<&SpriteWorker> {
        self.workers.get(worker_id)
    }

    /// Get a mutable worker by ID
    pub fn get_worker_mut(&mut self, worker_id: &str) -> Option<&mut SpriteWorker> {
        self.workers.get_mut(worker_id)
    }

    /// Count workers by status
    pub fn count_by_status(&self, status: SpriteWorkerStatus) -> usize {
        self.workers.values().filter(|w| w.status == status).count()
    }

    /// Get idle workers
    pub fn idle_workers(&self) -> Vec<&SpriteWorker> {
        self.workers
            .values()
            .filter(|w| w.status == SpriteWorkerStatus::Idle)
            .collect()
    }

    /// Get working workers
    pub fn working_workers(&self) -> Vec<&SpriteWorker> {
        self.workers
            .values()
            .filter(|w| w.status == SpriteWorkerStatus::Working)
            .collect()
    }

    /// Check if pool has capacity for more workers
    pub fn has_capacity(&self) -> bool {
        self.workers.len() < self.config.max_workers
    }

    /// Generate a unique worker ID
    pub fn next_worker_id(&self) -> String {
        let count = self.workers.len();
        format!("{}-worker-{}", self.config.name_prefix, count + 1)
    }

    /// Generate sprite name for worker
    pub fn sprite_name_for_worker(&self, worker_id: &str) -> String {
        format!("sprite-{}", worker_id.replace('.', "-"))
    }
}

// ============================================================================
// Hybrid Mode Support
// ============================================================================

/// Configuration for hybrid mode (local planner + remote workers)
#[derive(Debug, Clone)]
pub struct HybridConfig {
    /// Pane ID for local planner
    pub planner_pane: String,

    /// Ralph directory for shared coordination
    pub ralph_dir: PathBuf,

    /// Number of remote sprite workers
    pub num_workers: usize,

    /// Sprite pool configuration
    pub pool_config: SpritePoolConfig,
}

/// Status of a hybrid swarm
#[derive(Debug, Clone)]
pub struct HybridSwarmStatus {
    /// Local planner status
    pub planner_active: bool,

    /// Number of active remote workers
    pub active_workers: usize,

    /// Total tasks completed
    pub tasks_completed: u32,

    /// Current shared iteration
    pub iteration: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sprite_worker_lifecycle() {
        let mut worker = SpriteWorker::new("worker-1".to_string(), "sprite-worker-1".to_string());

        assert_eq!(worker.status, SpriteWorkerStatus::Provisioning);

        worker.mark_ready();
        assert_eq!(worker.status, SpriteWorkerStatus::Idle);

        worker.start_task("Build API", None);
        assert_eq!(worker.status, SpriteWorkerStatus::Working);
        assert_eq!(worker.task_description, Some("Build API".to_string()));

        worker.complete_task();
        assert_eq!(worker.status, SpriteWorkerStatus::Completed);
    }

    #[test]
    fn test_sprite_pool_capacity() {
        let config = SpritePoolConfig {
            max_workers: 2,
            ..Default::default()
        };
        let mut pool = SpritePool::new(config);

        assert!(pool.has_capacity());

        let worker1 = SpriteWorker::new("worker-1".to_string(), "sprite-1".to_string());
        pool.add_worker(worker1);
        assert!(pool.has_capacity());

        let worker2 = SpriteWorker::new("worker-2".to_string(), "sprite-2".to_string());
        pool.add_worker(worker2);
        assert!(!pool.has_capacity());
    }

    #[test]
    fn test_sprite_pool_worker_queries() {
        let config = SpritePoolConfig::default();
        let mut pool = SpritePool::new(config);

        let mut worker1 = SpriteWorker::new("worker-1".to_string(), "sprite-1".to_string());
        worker1.mark_ready();
        pool.add_worker(worker1);

        let mut worker2 = SpriteWorker::new("worker-2".to_string(), "sprite-2".to_string());
        worker2.mark_ready();
        worker2.start_task("Task A", None);
        pool.add_worker(worker2);

        assert_eq!(pool.count_by_status(SpriteWorkerStatus::Idle), 1);
        assert_eq!(pool.count_by_status(SpriteWorkerStatus::Working), 1);
        assert_eq!(pool.idle_workers().len(), 1);
        assert_eq!(pool.working_workers().len(), 1);
    }

    #[test]
    fn test_hybrid_mode() {
        let config = SpritePoolConfig::default();
        let mut pool = SpritePool::new(config);

        assert!(!pool.hybrid_mode);

        pool.set_hybrid_mode("%42", PathBuf::from("/tmp/.ralph"));
        assert!(pool.hybrid_mode);
        assert_eq!(pool.local_planner_pane, Some("%42".to_string()));
        assert_eq!(pool.shared_ralph_dir, Some(PathBuf::from("/tmp/.ralph")));
    }
}
