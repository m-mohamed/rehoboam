//! Ralph loop state management
//!
//! Implements proper Ralph loops with fresh sessions per iteration.
//! State persists in `.ralph/` directory, context stays fresh.
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
//! Note: Some functions are public APIs for future integration and may appear unused.
#![allow(dead_code)]

use chrono::{DateTime, Utc};
use color_eyre::eyre::{eyre, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info, warn};

/// Promise tag for explicit completion signal
pub const PROMISE_COMPLETE_TAG: &str = "<promise>COMPLETE</promise>";

/// Max session history entries to keep
const MAX_SESSION_HISTORY: usize = 50;

/// Ralph loop state persisted to state.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RalphState {
    /// Current iteration (0-indexed, incremented after each Stop)
    pub iteration: u32,

    /// Maximum iterations before auto-complete
    pub max_iterations: u32,

    /// Stop word to detect completion
    pub stop_word: String,

    /// When the loop started
    pub started_at: DateTime<Utc>,

    /// Tmux pane ID for respawning
    pub pane_id: String,

    /// Project directory
    pub project_dir: PathBuf,

    /// When the current iteration started (for timing)
    #[serde(default)]
    pub iteration_started_at: Option<DateTime<Utc>>,

    /// Error patterns seen (error_hash -> count)
    #[serde(default)]
    pub error_counts: HashMap<String, u32>,

    /// Last git commit hash (for checkpoint tracking)
    #[serde(default)]
    pub last_commit: Option<String>,

    /// Role for the agent (Planner, Worker, or Auto)
    #[serde(default)]
    pub role: LoopRole,
}

// ============================================================================
// v1.6: Loop Role - Cursor-aligned behavioral patterns
// ============================================================================

/// Role for a loop agent (Cursor-aligned)
///
/// Different roles get different prompts and behaviors:
/// - Planner: Explores, decomposes tasks, writes to tasks.md
/// - Worker: Picks task from queue, works in isolation, marks complete
/// - Auto: Legacy behavior, generic prompt
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LoopRole {
    /// Planner: Explores and creates tasks (doesn't implement)
    Planner,
    /// Worker: Executes single task in isolation
    Worker,
    /// Auto: Legacy behavior with generic prompt
    #[default]
    Auto,
}

impl std::fmt::Display for LoopRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoopRole::Planner => write!(f, "Planner"),
            LoopRole::Worker => write!(f, "Worker"),
            LoopRole::Auto => write!(f, "Auto"),
        }
    }
}

impl LoopRole {
    /// Cycle to next role
    pub fn next(self) -> Self {
        match self {
            LoopRole::Auto => LoopRole::Planner,
            LoopRole::Planner => LoopRole::Worker,
            LoopRole::Worker => LoopRole::Auto,
        }
    }

    /// Cycle to previous role
    pub fn prev(self) -> Self {
        match self {
            LoopRole::Auto => LoopRole::Worker,
            LoopRole::Planner => LoopRole::Auto,
            LoopRole::Worker => LoopRole::Planner,
        }
    }

    /// Get display name for UI
    pub fn display(&self) -> &'static str {
        match self {
            LoopRole::Auto => "Auto (generic)",
            LoopRole::Planner => "Planner (explores, creates tasks)",
            LoopRole::Worker => "Worker (executes task in isolation)",
        }
    }
}

/// Configuration for starting a Ralph loop
#[derive(Debug, Clone)]
pub struct RalphConfig {
    pub max_iterations: u32,
    pub stop_word: String,
    pub pane_id: String,
    /// v1.6: Loop role (Planner/Worker/Auto)
    pub role: LoopRole,
    /// v1.6: Enable coordination.md (opt-in, only for planners)
    pub enable_coordination: bool,
}

impl Default for RalphConfig {
    fn default() -> Self {
        Self {
            max_iterations: 50,
            stop_word: "DONE".to_string(),
            pane_id: String::new(),
            role: LoopRole::Auto,
            enable_coordination: false,
        }
    }
}

/// Initialize a new Ralph loop directory
///
/// Creates `.ralph/` with:
/// - anchor.md (the task prompt)
/// - guardrails.md (empty, for learned constraints)
/// - progress.md (empty, for tracking)
/// - errors.log (empty)
/// - state.json (initial state)
pub fn init_ralph_dir(project_dir: &Path, prompt: &str, config: &RalphConfig) -> Result<PathBuf> {
    let ralph_dir = project_dir.join(".ralph");

    // Create directory (ok if exists)
    fs::create_dir_all(&ralph_dir)?;

    info!("Initializing Ralph loop in {:?}", ralph_dir);

    // Write anchor.md
    let anchor_content = format!(
        r#"# Ralph Loop Task

## Success Criteria
<!-- Add checkboxes for completion criteria -->
- [ ] Task complete

## Instructions
{prompt}

## Notes
- Update progress.md with your work
- Add signs to guardrails.md when you learn constraints
- Write "{stop_word}" to progress.md when all criteria are met
"#,
        prompt = prompt,
        stop_word = config.stop_word,
    );
    fs::write(ralph_dir.join("anchor.md"), anchor_content)?;

    // Write empty guardrails.md
    let guardrails_content = r#"# Guardrails

Learned constraints from previous iterations. Check these before taking actions.

<!-- Signs will be added here as the loop progresses -->
"#;
    fs::write(ralph_dir.join("guardrails.md"), guardrails_content)?;

    // Write empty progress.md
    let progress_content = r#"# Progress

## Current Status
Starting iteration 1...

## Completed
<!-- Track completed work here -->

## Next Steps
<!-- Track remaining tasks here -->
"#;
    fs::write(ralph_dir.join("progress.md"), progress_content)?;

    // Create empty errors.log
    fs::write(ralph_dir.join("errors.log"), "")?;

    // Create empty activity.log
    fs::write(ralph_dir.join("activity.log"), "")?;

    // Create empty session_history.log
    fs::write(ralph_dir.join("session_history.log"), "")?;

    // v1.6: Create tasks.md for task queue (Cursor-aligned)
    let tasks_content = r#"# Task Queue

## Pending
<!-- Planners add tasks here -->

## In Progress
<!-- Workers claim tasks here -->

## Completed
<!-- Completed tasks move here -->
"#;
    fs::write(ralph_dir.join("tasks.md"), tasks_content)?;

    // v1.4/v1.6: Create coordination.md only if enabled (opt-in)
    // Per Cursor: "Workers never coordinate with each other"
    if config.enable_coordination {
        let coordination_content = r#"# Coordination

Cross-agent discoveries and broadcasts. Only planners use this.

<!-- Format: [timestamp] [agent_id]: message -->
"#;
        fs::write(ralph_dir.join("coordination.md"), coordination_content)?;
    }

    // Write state.json
    let state = RalphState {
        iteration: 0,
        max_iterations: config.max_iterations,
        stop_word: config.stop_word.clone(),
        started_at: Utc::now(),
        pane_id: config.pane_id.clone(),
        project_dir: project_dir.to_path_buf(),
        iteration_started_at: Some(Utc::now()),
        error_counts: HashMap::new(),
        last_commit: None,
        role: config.role,
    };
    let state_json = serde_json::to_string_pretty(&state)?;
    fs::write(ralph_dir.join("state.json"), state_json)?;

    debug!("Ralph directory initialized: {:?}", ralph_dir);
    Ok(ralph_dir)
}

/// Load Ralph state from directory
pub fn load_state(ralph_dir: &Path) -> Result<RalphState> {
    let state_path = ralph_dir.join("state.json");
    let content =
        fs::read_to_string(&state_path).map_err(|e| eyre!("Failed to read state.json: {}", e))?;
    let state: RalphState =
        serde_json::from_str(&content).map_err(|e| eyre!("Failed to parse state.json: {}", e))?;
    Ok(state)
}

/// Save Ralph state to directory
pub fn save_state(ralph_dir: &Path, state: &RalphState) -> Result<()> {
    let state_path = ralph_dir.join("state.json");
    let content = serde_json::to_string_pretty(state)?;
    fs::write(state_path, content)?;
    Ok(())
}

// ============================================================================
// v1.6: Task Queue System (Cursor-aligned)
// ============================================================================

/// A task in the queue
#[derive(Debug, Clone)]
pub struct Task {
    /// Task ID (e.g., "TASK-001")
    pub id: String,
    /// Task description
    pub description: String,
    /// Worker ID if claimed (e.g., "%42")
    pub worker: Option<String>,
}

/// Read all pending tasks from tasks.md
pub fn read_pending_tasks(ralph_dir: &Path) -> Result<Vec<Task>> {
    let tasks_path = ralph_dir.join("tasks.md");
    if !tasks_path.exists() {
        return Ok(vec![]);
    }

    let content = fs::read_to_string(&tasks_path)?;
    let mut tasks = Vec::new();
    let mut in_pending = false;

    for line in content.lines() {
        if line.starts_with("## Pending") {
            in_pending = true;
            continue;
        }
        if line.starts_with("## ") {
            in_pending = false;
            continue;
        }

        if in_pending && line.starts_with("- [ ] ") {
            if let Some(task) = parse_task_line(line) {
                tasks.push(task);
            }
        }
    }

    Ok(tasks)
}

/// Read the next available task from the queue
pub fn read_next_task(ralph_dir: &Path) -> Result<Option<Task>> {
    let tasks = read_pending_tasks(ralph_dir)?;
    Ok(tasks.into_iter().next())
}

/// Claim a task by moving it from Pending to In Progress
pub fn claim_task(ralph_dir: &Path, task_id: &str, worker_id: &str) -> Result<()> {
    let tasks_path = ralph_dir.join("tasks.md");
    let content = fs::read_to_string(&tasks_path)?;

    let mut new_lines = Vec::new();
    let mut claimed_task: Option<String> = None;
    let mut in_pending = false;
    let mut in_progress_section_exists = false;

    for line in content.lines() {
        if line.starts_with("## Pending") {
            in_pending = true;
            new_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("## In Progress") {
            in_pending = false;
            in_progress_section_exists = true;
            new_lines.push(line.to_string());
            // Insert claimed task here
            if let Some(ref task_desc) = claimed_task {
                new_lines.push(format!(
                    "- [~] [{}] {} (worker: {})",
                    task_id, task_desc, worker_id
                ));
            }
            continue;
        }
        if line.starts_with("## ") {
            in_pending = false;
        }

        // Check if this is the task to claim
        if in_pending && line.contains(&format!("[{}]", task_id)) {
            // Extract description (everything after the task ID)
            if let Some(task) = parse_task_line(line) {
                claimed_task = Some(task.description);
            }
            // Skip this line (don't add to new_lines)
            continue;
        }

        new_lines.push(line.to_string());
    }

    // If In Progress section doesn't exist, create it
    if !in_progress_section_exists && claimed_task.is_some() {
        // Find where to insert it (after Pending section)
        let mut insert_idx = None;
        for (i, line) in new_lines.iter().enumerate() {
            if line.starts_with("## Completed") {
                insert_idx = Some(i);
                break;
            }
        }
        if let Some(idx) = insert_idx {
            if let Some(ref task_desc) = claimed_task {
                new_lines.insert(idx, String::new());
                new_lines.insert(idx + 1, "## In Progress".to_string());
                new_lines.insert(
                    idx + 2,
                    format!("- [~] [{}] {} (worker: {})", task_id, task_desc, worker_id),
                );
            }
        }
    }

    fs::write(&tasks_path, new_lines.join("\n") + "\n")?;
    Ok(())
}

/// Complete a task by moving it from In Progress to Completed
pub fn complete_task(ralph_dir: &Path, task_id: &str) -> Result<()> {
    let tasks_path = ralph_dir.join("tasks.md");
    let content = fs::read_to_string(&tasks_path)?;

    let mut new_lines = Vec::new();
    let mut completed_task: Option<String> = None;
    let mut in_progress = false;

    for line in content.lines() {
        if line.starts_with("## In Progress") {
            in_progress = true;
            new_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("## Completed") {
            in_progress = false;
            new_lines.push(line.to_string());
            // Insert completed task here
            if let Some(ref task_desc) = completed_task {
                new_lines.push(format!("- [x] [{}] {}", task_id, task_desc));
            }
            continue;
        }
        if line.starts_with("## ") {
            in_progress = false;
        }

        // Check if this is the task to complete
        if in_progress && line.contains(&format!("[{}]", task_id)) {
            // Extract description (everything after task ID, before worker annotation)
            if let Some(task) = parse_in_progress_line(line) {
                completed_task = Some(task.description);
            }
            continue;
        }

        new_lines.push(line.to_string());
    }

    fs::write(&tasks_path, new_lines.join("\n") + "\n")?;
    Ok(())
}

/// Add a new task to the Pending queue
pub fn add_task(ralph_dir: &Path, task_id: &str, description: &str) -> Result<()> {
    let tasks_path = ralph_dir.join("tasks.md");
    let content = fs::read_to_string(&tasks_path)?;

    let mut new_lines = Vec::new();
    let mut added = false;

    for line in content.lines() {
        new_lines.push(line.to_string());
        // Add after "## Pending" line
        if line.starts_with("## Pending") && !added {
            new_lines.push(format!("- [ ] [{}] {}", task_id, description));
            added = true;
        }
    }

    fs::write(&tasks_path, new_lines.join("\n") + "\n")?;
    Ok(())
}

/// Parse a pending task line: "- [ ] [TASK-001] description"
fn parse_task_line(line: &str) -> Option<Task> {
    let trimmed = line.trim_start_matches("- [ ] ");
    // Extract [TASK-ID] and description
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.find(']') {
            let id = trimmed[start + 1..end].to_string();
            let description = trimmed[end + 1..].trim().to_string();
            return Some(Task {
                id,
                description,
                worker: None,
            });
        }
    }
    None
}

/// Parse an in-progress task line: "- [~] [TASK-001] description (worker: %42)"
fn parse_in_progress_line(line: &str) -> Option<Task> {
    let trimmed = line.trim_start_matches("- [~] ");
    // Extract [TASK-ID]
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.find(']') {
            let id = trimmed[start + 1..end].to_string();
            let rest = &trimmed[end + 1..];
            // Extract description (before "(worker:")
            let description = if let Some(worker_start) = rest.find("(worker:") {
                rest[..worker_start].trim().to_string()
            } else {
                rest.trim().to_string()
            };
            // Extract worker ID if present
            let worker = rest.find("(worker:").and_then(|worker_start| {
                let worker_part = &rest[worker_start + 8..];
                worker_part
                    .find(')')
                    .map(|worker_end| worker_part[..worker_end].trim().to_string())
            });
            return Some(Task {
                id,
                description,
                worker,
            });
        }
    }
    None
}

// ============================================================================
// v1.4: Multi-Agent Coordination
// ============================================================================

/// Broadcast a message to coordination.md for other agents to read
///
/// Messages are appended with timestamp and agent ID.
/// Format: `[2025-01-15T12:34:56Z] [agent-id]: message`
pub fn broadcast(ralph_dir: &Path, agent_id: &str, message: &str) -> Result<()> {
    let coordination_path = ralph_dir.join("coordination.md");

    // Create if doesn't exist
    if !coordination_path.exists() {
        fs::write(&coordination_path, "# Coordination\n\n")?;
    }

    let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let entry = format!("[{}] [{}]: {}\n", timestamp, agent_id, message);

    // Append to file
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&coordination_path)?;
    file.write_all(entry.as_bytes())?;

    debug!("Broadcast from {}: {}", agent_id, message);
    Ok(())
}

/// Read recent broadcasts from coordination.md
///
/// Returns broadcasts from the last N minutes (default: 60)
pub fn read_broadcasts(ralph_dir: &Path, max_age_minutes: Option<u32>) -> Result<Vec<String>> {
    let coordination_path = ralph_dir.join("coordination.md");

    if !coordination_path.exists() {
        return Ok(vec![]);
    }

    let content = fs::read_to_string(&coordination_path)?;
    let max_age = max_age_minutes.unwrap_or(60);
    let cutoff = Utc::now() - chrono::Duration::minutes(max_age as i64);

    let mut broadcasts = Vec::new();

    for line in content.lines() {
        // Parse timestamp from line: [2025-01-15T12:34:56Z] [agent]: message
        if let Some(timestamp_str) = line.strip_prefix('[').and_then(|s| s.split(']').next()) {
            if let Ok(timestamp) = timestamp_str.parse::<DateTime<Utc>>() {
                if timestamp > cutoff {
                    broadcasts.push(line.to_string());
                }
            }
        }
    }

    Ok(broadcasts)
}

/// Join an existing Rehoboam loop (for multi-worker coordination)
///
/// Returns the ralph directory if it exists and has valid state
pub fn join_existing_loop(project_dir: &Path) -> Result<PathBuf> {
    let ralph_dir = project_dir.join(".ralph");

    if !ralph_dir.exists() {
        return Err(eyre!("No .ralph directory found in {:?}", project_dir));
    }

    // Verify state.json exists and is valid
    let _ = load_state(&ralph_dir)?;

    info!("Joining existing Rehoboam loop at {:?}", ralph_dir);
    Ok(ralph_dir)
}

/// Register a worker with the coordination system
///
/// Adds a broadcast announcing the worker joined
pub fn register_worker(ralph_dir: &Path, worker_id: &str, description: &str) -> Result<()> {
    let message = format!("Worker joined: {}", description);
    broadcast(ralph_dir, worker_id, &message)?;

    // Create worker-specific state file
    let workers_dir = ralph_dir.join("workers");
    fs::create_dir_all(&workers_dir)?;

    let worker_file = workers_dir.join(format!("{}.md", worker_id));
    let content = format!(
        "# Worker: {}\n\nJoined: {}\nDescription: {}\n\n## Status\nActive\n",
        worker_id,
        Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
        description
    );
    fs::write(&worker_file, content)?;

    info!("Registered worker {} in {:?}", worker_id, ralph_dir);
    Ok(())
}

/// Update worker status
pub fn update_worker_status(ralph_dir: &Path, worker_id: &str, status: &str) -> Result<()> {
    let worker_file = ralph_dir.join("workers").join(format!("{}.md", worker_id));

    if !worker_file.exists() {
        return Err(eyre!("Worker {} not registered", worker_id));
    }

    let content = fs::read_to_string(&worker_file)?;

    // Replace status line (skip the old status value on the next line)
    let mut updated_lines = Vec::new();
    let mut skip_next = false;

    for line in content.lines() {
        if skip_next {
            skip_next = false;
            continue;
        }
        if line.starts_with("## Status") {
            updated_lines.push("## Status".to_string());
            updated_lines.push(status.to_string());
            skip_next = true;
        } else {
            updated_lines.push(line.to_string());
        }
    }
    let updated = updated_lines.join("\n");

    fs::write(&worker_file, updated)?;
    Ok(())
}

/// List active workers in the loop
pub fn list_workers(ralph_dir: &Path) -> Result<Vec<String>> {
    let workers_dir = ralph_dir.join("workers");

    if !workers_dir.exists() {
        return Ok(vec![]);
    }

    let mut workers = Vec::new();
    for entry in fs::read_dir(&workers_dir)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|e| e == "md") {
            if let Some(stem) = entry.path().file_stem() {
                workers.push(stem.to_string_lossy().to_string());
            }
        }
    }

    Ok(workers)
}

/// Increment iteration counter and return new value
pub fn increment_iteration(ralph_dir: &Path) -> Result<u32> {
    let mut state = load_state(ralph_dir)?;
    state.iteration += 1;
    save_state(ralph_dir, &state)?;
    info!("Ralph iteration incremented to {}", state.iteration);
    Ok(state.iteration)
}

/// Check if stop word is present in progress.md
pub fn check_stop_word(ralph_dir: &Path, stop_word: &str) -> Result<bool> {
    let progress_path = ralph_dir.join("progress.md");

    if !progress_path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(&progress_path)?;
    let found = content.to_uppercase().contains(&stop_word.to_uppercase());

    if found {
        info!("Stop word '{}' found in progress.md", stop_word);
    }

    Ok(found)
}

/// Check if max iterations reached
pub fn check_max_iterations(ralph_dir: &Path) -> Result<bool> {
    let state = load_state(ralph_dir)?;
    let reached = state.iteration >= state.max_iterations;

    if reached {
        warn!(
            "Max iterations reached: {} >= {}",
            state.iteration, state.max_iterations
        );
    }

    Ok(reached)
}

// ============================================================================
// v1.6: Role-Specific Prompts (Cursor-aligned)
// ============================================================================

/// Build a planner-specific prompt
///
/// Planners explore, decompose tasks, and write to tasks.md.
/// They do NOT implement anything themselves.
fn build_planner_prompt(ralph_dir: &Path, state: &RalphState) -> Result<String> {
    let anchor = fs::read_to_string(ralph_dir.join("anchor.md")).unwrap_or_default();
    let progress = fs::read_to_string(ralph_dir.join("progress.md")).unwrap_or_default();
    let tasks = fs::read_to_string(ralph_dir.join("tasks.md")).unwrap_or_default();

    let prompt = format!(
        r#"# Rehoboam Loop - PLANNER - Iteration {iteration}

You are a PLANNER. Your job is to explore and decompose work into tasks.

## Your Goal
{anchor}

## Current Tasks Queue
{tasks}

## Progress So Far
{progress}

## Rules for Planners
1. Explore the codebase to understand structure and patterns
2. Break down the goal into discrete, independent tasks
3. Each task should be completable by a single worker in ONE iteration
4. Write tasks to tasks.md in the Pending section using format:
   `- [ ] [TASK-XXX] Description of the task`
5. Do NOT implement anything yourself
6. Do NOT coordinate with workers - they work in isolation
7. When planning is complete, write "PLANNING COMPLETE" to progress.md
8. If stuck, add more exploration tasks rather than trying to solve everything

## Task Guidelines
- Tasks should be atomic and independent
- Include enough context in the description for a worker to understand
- Prefix related tasks with common identifiers (e.g., TASK-AUTH-001, TASK-AUTH-002)
- Order tasks by dependency (simpler tasks first)

Remember: Your job is PLANNING, not IMPLEMENTING. Create clear tasks for workers.
"#,
        iteration = state.iteration + 1,
        anchor = anchor,
        tasks = tasks,
        progress = progress,
    );

    Ok(prompt)
}

/// Build a worker-specific prompt
///
/// Workers pick ONE task from the queue, execute it in isolation,
/// and mark it complete. They do NOT coordinate with other workers.
fn build_worker_prompt(ralph_dir: &Path, state: &RalphState) -> Result<String> {
    let anchor = fs::read_to_string(ralph_dir.join("anchor.md")).unwrap_or_default();
    let guardrails = fs::read_to_string(ralph_dir.join("guardrails.md")).unwrap_or_default();

    // Get next available task
    let next_task = read_next_task(ralph_dir)?;
    let task_section = if let Some(task) = &next_task {
        format!(
            "## Your Assigned Task\n**[{}]** {}\n",
            task.id, task.description
        )
    } else {
        "## Your Assigned Task\nNo tasks available in queue. Check with planner or wait.\n"
            .to_string()
    };

    let prompt = format!(
        r#"# Rehoboam Loop - WORKER - Iteration {iteration}

You are a WORKER. Your job is to complete ONE assigned task.

{task_section}
## Context (for reference only)
{anchor}

## Guardrails
{guardrails}

## Rules for Workers
1. Focus ONLY on your assigned task - ignore other work
2. Do NOT coordinate with other workers - they handle their own tasks
3. Do NOT explore unrelated code or add scope
4. When done, mark your task complete in tasks.md:
   Change `- [ ] [TASK-XXX]` to `- [x] [TASK-XXX]` in the Completed section
5. Update progress.md with what you accomplished
6. If blocked by something outside your task, note it in progress.md and exit
7. Do NOT try to solve blockers that require other tasks
8. Write "{stop_word}" when your task is fully complete

## Task Completion Checklist
- [ ] Task implemented as described
- [ ] Tests pass (if applicable)
- [ ] Task marked complete in tasks.md
- [ ] Progress.md updated with summary

Remember: Complete YOUR task, then exit. Don't do extra work.
"#,
        iteration = state.iteration + 1,
        task_section = task_section,
        anchor = anchor,
        guardrails = guardrails,
        stop_word = state.stop_word,
    );

    Ok(prompt)
}

/// Build the iteration prompt that includes state files
///
/// This creates a prompt file that tells Claude:
/// - What iteration it's on
/// - The anchor (task spec)
/// - Any guardrails
/// - Progress so far
///
/// v1.6: Dispatches to role-specific prompts based on state.role
pub fn build_iteration_prompt(ralph_dir: &Path) -> Result<String> {
    let state = load_state(ralph_dir)?;

    // v1.6: Dispatch to role-specific prompts
    let prompt = match state.role {
        LoopRole::Planner => build_planner_prompt(ralph_dir, &state)?,
        LoopRole::Worker => build_worker_prompt(ralph_dir, &state)?,
        LoopRole::Auto => build_auto_prompt(ralph_dir, &state)?,
    };

    // Write to a temp file for claude stdin piping
    let prompt_file = ralph_dir.join("_iteration_prompt.md");
    fs::write(&prompt_file, &prompt)?;

    debug!(
        "Built {} iteration prompt for iteration {}",
        state.role,
        state.iteration + 1
    );
    Ok(prompt_file.to_string_lossy().to_string())
}

/// Build the legacy "Auto" prompt (backward compatible)
fn build_auto_prompt(ralph_dir: &Path, state: &RalphState) -> Result<String> {
    let anchor = fs::read_to_string(ralph_dir.join("anchor.md")).unwrap_or_default();
    let guardrails = fs::read_to_string(ralph_dir.join("guardrails.md")).unwrap_or_default();
    let progress = fs::read_to_string(ralph_dir.join("progress.md")).unwrap_or_default();

    // Get recent iteration context (last 5 iterations)
    let recent = get_recent_progress_summary(ralph_dir, 5).unwrap_or_default();
    let recent_section = if recent.is_empty() {
        String::new()
    } else {
        format!("## Recent Activity\n{}\n", recent)
    };

    // v1.4: Get recent broadcasts from coordination.md
    let broadcasts = read_broadcasts(ralph_dir, Some(60)).unwrap_or_default();
    let coordination_section = if broadcasts.is_empty() {
        String::new()
    } else {
        format!(
            "## Coordination (from other workers)\n{}\n\n",
            broadcasts.join("\n")
        )
    };

    // v1.4: List active workers if any
    let workers = list_workers(ralph_dir).unwrap_or_default();
    let workers_section = if workers.is_empty() {
        String::new()
    } else {
        format!(
            "## Active Workers\n{}\n\n",
            workers
                .iter()
                .map(|w| format!("- {}", w))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };

    let prompt = format!(
        r#"# Rehoboam Loop - Iteration {iteration}

You are in a Rehoboam loop. Each iteration starts fresh - make incremental progress.

{recent_section}{coordination_section}{workers_section}## Your Task (Anchor)
{anchor}

## Learned Constraints (Guardrails)
{guardrails}

## Progress So Far
{progress}

## Instructions for This Iteration
1. Read the anchor to understand your task
2. Check guardrails before taking actions
3. Check coordination section for discoveries from other workers
4. Continue from where progress.md left off
5. Update progress.md with your work
6. If you discover something useful for other workers, use broadcast
7. If you hit a repeating problem, add a SIGN to guardrails.md
8. When ALL criteria are met, write either:
   - "{stop_word}" anywhere in progress.md, OR
   - <promise>COMPLETE</promise> tag (more explicit)
9. Exit when you've made progress (don't try to finish everything)

Remember: Progress persists, failures evaporate. Make incremental progress.
Git commits are created between iterations for easy rollback.
"#,
        iteration = state.iteration + 1, // Display as 1-indexed
        recent_section = recent_section,
        coordination_section = coordination_section,
        workers_section = workers_section,
        anchor = anchor,
        guardrails = guardrails,
        progress = progress,
        stop_word = state.stop_word,
    );

    Ok(prompt)
}

/// Add a guardrail/sign to guardrails.md (internal use)
fn add_guardrail(ralph_dir: &Path, sign: &str, trigger: &str, instruction: &str) -> Result<()> {
    let state = load_state(ralph_dir)?;
    let guardrails_path = ralph_dir.join("guardrails.md");

    let entry = format!(
        r#"
### Sign: {sign}
- **Trigger:** {trigger}
- **Instruction:** {instruction}
- **Added:** Iteration {iteration}
"#,
        sign = sign,
        trigger = trigger,
        instruction = instruction,
        iteration = state.iteration,
    );

    let mut content = fs::read_to_string(&guardrails_path).unwrap_or_default();
    content.push_str(&entry);
    fs::write(guardrails_path, content)?;

    info!("Added guardrail: {}", sign);
    Ok(())
}

// =============================================================================
// Git Checkpoints
// =============================================================================

/// Create a git checkpoint after an iteration completes
///
/// Commits all changes with a message indicating the Ralph iteration.
/// Returns the commit hash if successful.
pub fn create_git_checkpoint(ralph_dir: &Path) -> Result<Option<String>> {
    let state = load_state(ralph_dir)?;
    let project_dir = ralph_dir
        .parent()
        .ok_or_else(|| eyre!("Invalid ralph dir"))?;

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

    // Commit with Ralph iteration message
    let commit_msg = format!(
        "ralph: iteration {} checkpoint\n\nAutomated checkpoint from Ralph loop.",
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
            save_state(ralph_dir, &new_state)?;

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

// =============================================================================
// NEW: Activity Log
// =============================================================================

/// Log activity metrics for an iteration
///
/// Records timing, tool calls, and other metrics to activity.log
pub fn log_activity(
    ralph_dir: &Path,
    iteration: u32,
    duration_secs: Option<u64>,
    tool_calls: Option<u32>,
    completion_reason: &str,
) -> Result<()> {
    let activity_path = ralph_dir.join("activity.log");

    let duration_str = duration_secs
        .map(|s| format!("{}m {}s", s / 60, s % 60))
        .unwrap_or_else(|| "unknown".to_string());

    let tools_str = tool_calls
        .map(|t| t.to_string())
        .unwrap_or_else(|| "?".to_string());

    let entry = format!(
        "[{}] Iteration {} completed | Duration: {} | Tool calls: {} | Reason: {}\n",
        Utc::now().format("%Y-%m-%d %H:%M:%S"),
        iteration,
        duration_str,
        tools_str,
        completion_reason
    );

    let mut content = fs::read_to_string(&activity_path).unwrap_or_default();
    content.push_str(&entry);
    fs::write(activity_path, content)?;

    info!(
        "Activity logged: iteration {} in {}",
        iteration, duration_str
    );
    Ok(())
}

/// Mark iteration start time
pub fn mark_iteration_start(ralph_dir: &Path) -> Result<()> {
    let mut state = load_state(ralph_dir)?;
    state.iteration_started_at = Some(Utc::now());
    save_state(ralph_dir, &state)?;
    Ok(())
}

/// Get iteration duration in seconds
pub fn get_iteration_duration(ralph_dir: &Path) -> Option<u64> {
    let state = load_state(ralph_dir).ok()?;
    let started = state.iteration_started_at?;
    let duration = Utc::now().signed_duration_since(started);
    Some(duration.num_seconds().max(0) as u64)
}

// =============================================================================
// NEW: Promise Tag Support
// =============================================================================

/// Check if the <promise>COMPLETE</promise> tag is present in progress.md
///
/// This is a more explicit completion signal than stop word matching.
pub fn check_promise_tag(ralph_dir: &Path) -> Result<bool> {
    let progress_path = ralph_dir.join("progress.md");

    if !progress_path.exists() {
        return Ok(false);
    }

    let content = fs::read_to_string(&progress_path)?;
    let found = content.contains(PROMISE_COMPLETE_TAG);

    if found {
        info!("Promise tag {} found in progress.md", PROMISE_COMPLETE_TAG);
    }

    Ok(found)
}

/// Check for completion using both stop word AND promise tag
///
/// Returns (is_complete, reason)
pub fn check_completion(ralph_dir: &Path, stop_word: &str) -> Result<(bool, String)> {
    // Check promise tag first (more explicit)
    if check_promise_tag(ralph_dir)? {
        return Ok((true, "promise_tag".to_string()));
    }

    // Check stop word
    if check_stop_word(ralph_dir, stop_word)? {
        return Ok((true, "stop_word".to_string()));
    }

    Ok((false, String::new()))
}

// =============================================================================
// v1.4: Judge Mode - Cursor-inspired evaluation phase
// =============================================================================

/// Completion indicators in progress.md that suggest task is done
const COMPLETION_INDICATORS: &[&str] = &[
    "all tasks completed",
    "implementation complete",
    "successfully implemented",
    "task is done",
    "work is complete",
    "finished implementing",
    "all requirements met",
    "nothing left to do",
    "ready for review",
    "all tests pass",
];

/// Stall indicators that suggest the task is blocked
const STALL_INDICATORS: &[&str] = &[
    "blocked by",
    "need clarification",
    "cannot proceed",
    "stuck on",
    "waiting for",
    "unclear requirements",
    "need more information",
    "error persists",
];

/// Judge decision type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JudgeDecision {
    /// Continue to next iteration
    Continue,
    /// Task is complete, stop the loop
    Complete,
    /// Task is stalled, needs intervention
    Stalled,
}

/// v1.4: Evaluate completion using judge heuristics
///
/// Analyzes progress.md against anchor.md to determine if the task is complete.
/// This is a simple heuristic-based judge - a full implementation would spawn
/// a separate Claude session to evaluate.
///
/// Returns (decision, confidence, reason)
pub fn judge_completion(ralph_dir: &Path) -> Result<(JudgeDecision, f64, String)> {
    let progress = read_file_content(ralph_dir, "progress.md")?;
    let anchor = read_file_content(ralph_dir, "anchor.md")?;
    let progress_lower = progress.to_lowercase();

    // Check for explicit completion indicators
    for indicator in COMPLETION_INDICATORS {
        if progress_lower.contains(indicator) {
            return Ok((
                JudgeDecision::Complete,
                0.8,
                format!("Found completion indicator: '{}'", indicator),
            ));
        }
    }

    // Check for stall indicators
    for indicator in STALL_INDICATORS {
        if progress_lower.contains(indicator) {
            return Ok((
                JudgeDecision::Stalled,
                0.7,
                format!("Found stall indicator: '{}'", indicator),
            ));
        }
    }

    // Check if progress mentions all anchor requirements
    let anchor_lower = anchor.to_lowercase();
    let requirements: Vec<&str> = anchor_lower
        .lines()
        .filter(|l| l.starts_with("- ") || l.starts_with("* ") || l.starts_with("1."))
        .collect();

    if !requirements.is_empty() {
        // Simple heuristic: if progress is long and mentions most requirement keywords
        let progress_word_count = progress.split_whitespace().count();
        let requirement_keywords: Vec<&str> = requirements
            .iter()
            .flat_map(|r| r.split_whitespace())
            .filter(|w| w.len() > 4)
            .take(10)
            .collect();

        let keywords_found = requirement_keywords
            .iter()
            .filter(|kw| progress_lower.contains(*kw))
            .count();

        let coverage = if requirement_keywords.is_empty() {
            0.0
        } else {
            keywords_found as f64 / requirement_keywords.len() as f64
        };

        // If progress is substantial and covers most requirements, likely complete
        if progress_word_count > 200 && coverage > 0.7 {
            return Ok((
                JudgeDecision::Complete,
                coverage * 0.8,
                format!(
                    "Progress covers {:.0}% of requirement keywords",
                    coverage * 100.0
                ),
            ));
        }
    }

    // Default: continue
    Ok((
        JudgeDecision::Continue,
        0.5,
        "No completion or stall indicators found".to_string(),
    ))
}

/// Helper to read file content with default for missing files
fn read_file_content(ralph_dir: &Path, filename: &str) -> Result<String> {
    let path = ralph_dir.join(filename);
    if path.exists() {
        Ok(fs::read_to_string(path)?)
    } else {
        Ok(String::new())
    }
}

// =============================================================================
// NEW: Auto-Guardrails from Error Patterns
// =============================================================================

/// Threshold for auto-adding guardrails (error seen N times)
const AUTO_GUARDRAIL_THRESHOLD: u32 = 3;

/// Track an error and return true if it should trigger auto-guardrail
///
/// If the same error pattern appears AUTO_GUARDRAIL_THRESHOLD times,
/// automatically adds it to guardrails.md
pub fn track_error_pattern(ralph_dir: &Path, error: &str) -> Result<bool> {
    let mut state = load_state(ralph_dir)?;

    // Create a simple hash of the error (first 100 chars, normalized)
    let error_key = error
        .chars()
        .take(100)
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .take(10)
        .collect::<Vec<_>>()
        .join("_");

    // Increment count
    let count = state.error_counts.entry(error_key.clone()).or_insert(0);
    *count += 1;
    let current_count = *count;

    save_state(ralph_dir, &state)?;

    // Check if we hit the threshold
    if current_count == AUTO_GUARDRAIL_THRESHOLD {
        // Auto-add guardrail
        let sign_name = format!("Auto-detected: {}", &error_key[..error_key.len().min(30)]);
        let trigger = error.chars().take(200).collect::<String>();
        let instruction = format!(
            "This error has occurred {} times. Review the approach and try a different strategy.",
            current_count
        );

        add_guardrail(ralph_dir, &sign_name, &trigger, &instruction)?;
        info!(
            "Auto-added guardrail for repeated error: {} ({} occurrences)",
            error_key, current_count
        );
        return Ok(true);
    }

    Ok(false)
}

// =============================================================================
// NEW: Session History Logging
// =============================================================================

/// Log a session state transition
///
/// Records transitions like: started -> working -> stopped -> respawning
pub fn log_session_transition(
    ralph_dir: &Path,
    from_state: &str,
    to_state: &str,
    details: Option<&str>,
) -> Result<()> {
    let history_path = ralph_dir.join("session_history.log");
    let state = load_state(ralph_dir)?;

    let details_str = details.map(|d| format!(" | {}", d)).unwrap_or_default();

    let entry = format!(
        "[{}] [Iter {}] {} -> {}{}\n",
        Utc::now().format("%H:%M:%S"),
        state.iteration,
        from_state,
        to_state,
        details_str
    );

    // Read existing content
    let mut content = fs::read_to_string(&history_path).unwrap_or_default();

    // Trim to max entries (keep last N lines)
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() >= MAX_SESSION_HISTORY {
        content = lines[lines.len() - MAX_SESSION_HISTORY + 1..].join("\n") + "\n";
    }

    content.push_str(&entry);
    fs::write(history_path, content)?;

    debug!("Session transition: {} -> {}", from_state, to_state);
    Ok(())
}

// =============================================================================
// NEW: Progress Injection - Recent Iteration Context
// =============================================================================

/// Get summary of recent iteration outcomes for context injection
///
/// Returns the last N entries from activity.log formatted for inclusion
/// in the iteration prompt. This helps Claude understand recent progress.
pub fn get_recent_progress_summary(ralph_dir: &Path, count: usize) -> Result<String> {
    let activity_path = ralph_dir.join("activity.log");

    if !activity_path.exists() {
        return Ok(String::new());
    }

    let content = fs::read_to_string(&activity_path)?;
    let lines: Vec<&str> = content.lines().rev().take(count).collect();

    if lines.is_empty() {
        return Ok(String::new());
    }

    let mut summary = String::from("Recent iteration outcomes:\n");
    for line in lines.iter().rev() {
        summary.push_str(&format!("  {}\n", line));
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_init_ralph_dir() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig {
            max_iterations: 10,
            stop_word: "COMPLETE".to_string(),
            pane_id: "%42".to_string(),
            role: LoopRole::Auto,
            enable_coordination: false,
        };

        let ralph_dir = init_ralph_dir(temp.path(), "Build a REST API", &config).unwrap();

        assert!(ralph_dir.join("anchor.md").exists());
        assert!(ralph_dir.join("guardrails.md").exists());
        assert!(ralph_dir.join("progress.md").exists());
        assert!(ralph_dir.join("errors.log").exists());
        assert!(ralph_dir.join("state.json").exists());

        let state = load_state(&ralph_dir).unwrap();
        assert_eq!(state.iteration, 0);
        assert_eq!(state.max_iterations, 10);
        assert_eq!(state.stop_word, "COMPLETE");
    }

    #[test]
    fn test_increment_iteration() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        let iter1 = increment_iteration(&ralph_dir).unwrap();
        assert_eq!(iter1, 1);

        let iter2 = increment_iteration(&ralph_dir).unwrap();
        assert_eq!(iter2, 2);
    }

    #[test]
    fn test_check_stop_word() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig {
            stop_word: "FINISHED".to_string(),
            ..Default::default()
        };
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        // Initially no stop word
        assert!(!check_stop_word(&ralph_dir, "FINISHED").unwrap());

        // Add stop word to progress
        fs::write(ralph_dir.join("progress.md"), "Task FINISHED successfully").unwrap();
        assert!(check_stop_word(&ralph_dir, "FINISHED").unwrap());

        // Case insensitive
        assert!(check_stop_word(&ralph_dir, "finished").unwrap());
    }

    #[test]
    fn test_check_max_iterations() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig {
            max_iterations: 3,
            ..Default::default()
        };
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        assert!(!check_max_iterations(&ralph_dir).unwrap());

        increment_iteration(&ralph_dir).unwrap();
        increment_iteration(&ralph_dir).unwrap();
        assert!(!check_max_iterations(&ralph_dir).unwrap());

        increment_iteration(&ralph_dir).unwrap();
        assert!(check_max_iterations(&ralph_dir).unwrap());
    }

    #[test]
    fn test_check_promise_tag() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        // Initially no promise tag
        assert!(!check_promise_tag(&ralph_dir).unwrap());

        // Add promise tag to progress
        fs::write(
            ralph_dir.join("progress.md"),
            "Task complete!\n<promise>COMPLETE</promise>",
        )
        .unwrap();
        assert!(check_promise_tag(&ralph_dir).unwrap());
    }

    #[test]
    fn test_check_completion() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig {
            stop_word: "DONE".to_string(),
            ..Default::default()
        };
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        // Initially not complete
        let (complete, _) = check_completion(&ralph_dir, "DONE").unwrap();
        assert!(!complete);

        // Stop word triggers completion
        fs::write(ralph_dir.join("progress.md"), "Task DONE").unwrap();
        let (complete, reason) = check_completion(&ralph_dir, "DONE").unwrap();
        assert!(complete);
        assert_eq!(reason, "stop_word");

        // Promise tag also triggers completion (takes precedence)
        fs::write(ralph_dir.join("progress.md"), "<promise>COMPLETE</promise>").unwrap();
        let (complete, reason) = check_completion(&ralph_dir, "DONE").unwrap();
        assert!(complete);
        assert_eq!(reason, "promise_tag");
    }

    #[test]
    fn test_log_activity() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        log_activity(&ralph_dir, 1, Some(120), Some(50), "continuing").unwrap();
        log_activity(&ralph_dir, 2, Some(90), None, "complete:stop_word").unwrap();

        let content = fs::read_to_string(ralph_dir.join("activity.log")).unwrap();
        assert!(content.contains("Iteration 1"));
        assert!(content.contains("2m 0s"));
        assert!(content.contains("Iteration 2"));
        assert!(content.contains("complete:stop_word"));
    }

    #[test]
    fn test_log_session_transition() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        log_session_transition(&ralph_dir, "init", "starting", Some("%42")).unwrap();
        log_session_transition(&ralph_dir, "working", "stopping", None).unwrap();
        log_session_transition(&ralph_dir, "stopping", "respawning", Some("iteration 2")).unwrap();

        let content = fs::read_to_string(ralph_dir.join("session_history.log")).unwrap();
        assert!(content.contains("init -> starting"));
        assert!(content.contains("working -> stopping"));
        assert!(content.contains("respawning"));
    }

    #[test]
    fn test_track_error_pattern() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        // First two occurrences don't trigger guardrail
        assert!(!track_error_pattern(&ralph_dir, "API rate limit exceeded").unwrap());
        assert!(!track_error_pattern(&ralph_dir, "API rate limit exceeded").unwrap());

        // Third occurrence triggers auto-guardrail
        assert!(track_error_pattern(&ralph_dir, "API rate limit exceeded").unwrap());

        // Check guardrail was added
        let guardrails = fs::read_to_string(ralph_dir.join("guardrails.md")).unwrap();
        assert!(guardrails.contains("Auto-detected"));
        assert!(guardrails.contains("occurred 3 times"));
    }

    #[test]
    fn test_activity_log_created() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        assert!(ralph_dir.join("activity.log").exists());
        assert!(ralph_dir.join("session_history.log").exists());
    }

    // v1.4: Judge mode tests

    #[test]
    fn test_judge_completion_indicators() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        // Test completion indicator
        fs::write(
            ralph_dir.join("progress.md"),
            "All tasks completed successfully.",
        )
        .unwrap();
        let (decision, confidence, _) = super::judge_completion(&ralph_dir).unwrap();
        assert_eq!(decision, JudgeDecision::Complete);
        assert!(confidence > 0.5);
    }

    #[test]
    fn test_judge_stall_indicators() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        // Test stall indicator
        fs::write(
            ralph_dir.join("progress.md"),
            "Blocked by missing API credentials.",
        )
        .unwrap();
        let (decision, _, _) = super::judge_completion(&ralph_dir).unwrap();
        assert_eq!(decision, JudgeDecision::Stalled);
    }

    #[test]
    fn test_judge_continue_default() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        // Test continue (no indicators)
        fs::write(
            ralph_dir.join("progress.md"),
            "Working on implementing the feature.",
        )
        .unwrap();
        let (decision, _, _) = super::judge_completion(&ralph_dir).unwrap();
        assert_eq!(decision, JudgeDecision::Continue);
    }

    // v1.4: Multi-Agent Coordination tests

    #[test]
    fn test_coordination_file_created() {
        let temp = TempDir::new().unwrap();
        // Coordination is opt-in per Cursor guidance
        let config = RalphConfig {
            enable_coordination: true,
            ..Default::default()
        };
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        assert!(ralph_dir.join("coordination.md").exists());
    }

    #[test]
    fn test_coordination_file_not_created_by_default() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        // Coordination is opt-in, so it shouldn't exist by default
        assert!(!ralph_dir.join("coordination.md").exists());
    }

    #[test]
    fn test_broadcast() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig {
            enable_coordination: true,
            ..Default::default()
        };
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        broadcast(&ralph_dir, "worker-1", "Found API schema at /api/v1").unwrap();
        broadcast(&ralph_dir, "worker-2", "Database migration complete").unwrap();

        let content = fs::read_to_string(ralph_dir.join("coordination.md")).unwrap();
        assert!(content.contains("[worker-1]: Found API schema at /api/v1"));
        assert!(content.contains("[worker-2]: Database migration complete"));
    }

    #[test]
    fn test_read_broadcasts() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        broadcast(&ralph_dir, "agent-a", "Test message 1").unwrap();
        broadcast(&ralph_dir, "agent-b", "Test message 2").unwrap();

        let broadcasts = read_broadcasts(&ralph_dir, Some(60)).unwrap();
        assert_eq!(broadcasts.len(), 2);
    }

    #[test]
    fn test_register_worker() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        register_worker(&ralph_dir, "worker-1", "API endpoint worker").unwrap();

        // Check worker file created
        let worker_file = ralph_dir.join("workers/worker-1.md");
        assert!(worker_file.exists());

        let content = fs::read_to_string(&worker_file).unwrap();
        assert!(content.contains("API endpoint worker"));

        // Check broadcast was sent
        let broadcasts = read_broadcasts(&ralph_dir, Some(60)).unwrap();
        assert!(!broadcasts.is_empty());
    }

    #[test]
    fn test_list_workers() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        register_worker(&ralph_dir, "worker-a", "First worker").unwrap();
        register_worker(&ralph_dir, "worker-b", "Second worker").unwrap();

        let workers = list_workers(&ralph_dir).unwrap();
        assert_eq!(workers.len(), 2);
        assert!(workers.contains(&"worker-a".to_string()));
        assert!(workers.contains(&"worker-b".to_string()));
    }

    #[test]
    fn test_join_existing_loop() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let _ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        // Should be able to join existing loop
        let joined_dir = join_existing_loop(temp.path()).unwrap();
        assert!(joined_dir.exists());

        // Should fail for non-existent loop
        let other_temp = TempDir::new().unwrap();
        assert!(join_existing_loop(other_temp.path()).is_err());
    }

    // v1.6: Task Queue tests

    #[test]
    fn test_tasks_file_created() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        assert!(ralph_dir.join("tasks.md").exists());
    }

    #[test]
    fn test_add_and_read_tasks() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        // Add tasks (newest first - LIFO order)
        add_task(&ralph_dir, "TASK-001", "Implement user auth").unwrap();
        add_task(&ralph_dir, "TASK-002", "Add API validation").unwrap();

        // Read pending tasks - newest task is at top
        let tasks = read_pending_tasks(&ralph_dir).unwrap();
        assert_eq!(tasks.len(), 2);
        // Most recently added task is first
        assert_eq!(tasks[0].id, "TASK-002");
        assert_eq!(tasks[0].description, "Add API validation");
        assert_eq!(tasks[1].id, "TASK-001");
    }

    #[test]
    fn test_read_next_task() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        // No tasks initially
        let task = read_next_task(&ralph_dir).unwrap();
        assert!(task.is_none());

        // Add a task
        add_task(&ralph_dir, "TASK-001", "First task").unwrap();

        // Should return the first task
        let task = read_next_task(&ralph_dir).unwrap();
        assert!(task.is_some());
        assert_eq!(task.unwrap().id, "TASK-001");
    }

    #[test]
    fn test_claim_task() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        // Add a task
        add_task(&ralph_dir, "TASK-001", "Auth endpoint").unwrap();

        // Claim it
        claim_task(&ralph_dir, "TASK-001", "%42").unwrap();

        // Should no longer be in pending
        let pending = read_pending_tasks(&ralph_dir).unwrap();
        assert!(pending.is_empty());

        // Check the file content
        let content = fs::read_to_string(ralph_dir.join("tasks.md")).unwrap();
        assert!(content.contains("In Progress"));
        assert!(content.contains("[TASK-001]"));
        assert!(content.contains("worker: %42"));
    }

    #[test]
    fn test_complete_task() {
        let temp = TempDir::new().unwrap();
        let config = RalphConfig::default();
        let ralph_dir = init_ralph_dir(temp.path(), "Test", &config).unwrap();

        // Add and claim a task
        add_task(&ralph_dir, "TASK-001", "Auth endpoint").unwrap();
        claim_task(&ralph_dir, "TASK-001", "%42").unwrap();

        // Complete it
        complete_task(&ralph_dir, "TASK-001").unwrap();

        // Check the file content
        let content = fs::read_to_string(ralph_dir.join("tasks.md")).unwrap();
        assert!(content.contains("Completed"));
        assert!(content.contains("[x] [TASK-001]"));
    }

    #[test]
    fn test_role_specific_prompts() {
        let temp = TempDir::new().unwrap();

        // Test Planner role
        let planner_config = RalphConfig {
            role: LoopRole::Planner,
            ..Default::default()
        };
        let ralph_dir = init_ralph_dir(temp.path(), "Build API", &planner_config).unwrap();
        let prompt_file = build_iteration_prompt(&ralph_dir).unwrap();
        let prompt = fs::read_to_string(&prompt_file).unwrap();
        assert!(prompt.contains("PLANNER"));
        assert!(prompt.contains("Do NOT implement anything yourself"));

        // Test Worker role
        let temp2 = TempDir::new().unwrap();
        let worker_config = RalphConfig {
            role: LoopRole::Worker,
            ..Default::default()
        };
        let ralph_dir2 = init_ralph_dir(temp2.path(), "Build API", &worker_config).unwrap();
        add_task(&ralph_dir2, "TASK-001", "Build auth").unwrap();
        let prompt_file2 = build_iteration_prompt(&ralph_dir2).unwrap();
        let prompt2 = fs::read_to_string(&prompt_file2).unwrap();
        assert!(prompt2.contains("WORKER"));
        assert!(prompt2.contains("TASK-001"));

        // Test Auto role (backward compatible)
        let temp3 = TempDir::new().unwrap();
        let auto_config = RalphConfig::default();
        let ralph_dir3 = init_ralph_dir(temp3.path(), "Build API", &auto_config).unwrap();
        let prompt_file3 = build_iteration_prompt(&ralph_dir3).unwrap();
        let prompt3 = fs::read_to_string(&prompt_file3).unwrap();
        assert!(prompt3.contains("Rehoboam Loop - Iteration"));
        assert!(!prompt3.contains("PLANNER"));
        assert!(!prompt3.contains("WORKER"));
    }
}
