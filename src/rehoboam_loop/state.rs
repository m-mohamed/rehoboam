//! Loop state management
//!
//! Core state types and persistence for Rehoboam loops.

use chrono::{DateTime, Utc};
use color_eyre::eyre::{eyre, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Rehoboam loop state persisted to state.json
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopState {
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

    /// Assigned task ID for Workers (Cursor isolation model)
    /// Workers only see their pre-assigned task, not shared tasks.md
    #[serde(default)]
    pub assigned_task: Option<String>,
}

// ============================================================================
// Loop Role - Cursor-aligned behavioral patterns
// ============================================================================

/// Role for a loop agent (Cursor-aligned)
///
/// Different roles get different prompts and behaviors:
/// - Planner: Explores, decomposes tasks, uses TaskCreate to add tasks
/// - Worker: Uses TaskList to find tasks, TaskUpdate to claim/complete
/// - Auto: General autonomous loop, uses TaskCreate/TaskUpdate as needed
///
/// **REQUIRES**: `CLAUDE_CODE_TASK_LIST_ID` environment variable for all roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LoopRole {
    /// Planner: Explores and creates tasks with TaskCreate (doesn't implement)
    Planner,
    /// Worker: Claims tasks via TaskUpdate, executes in isolation
    Worker,
    /// Auto: General autonomous loop with TaskCreate/TaskUpdate
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

/// Configuration for starting a Rehoboam loop
#[derive(Debug, Clone)]
pub struct RehoboamConfig {
    pub max_iterations: u32,
    pub stop_word: String,
    pub pane_id: String,
    /// Loop role (Planner/Worker/Auto)
    pub role: LoopRole,
    /// Enable coordination.md (opt-in, only for planners)
    pub enable_coordination: bool,
}

impl Default for RehoboamConfig {
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

/// Initialize a new Rehoboam loop directory
///
/// Creates `.rehoboam/` with:
/// - anchor.md (the task prompt)
/// - guardrails.md (empty, for learned constraints)
/// - progress.md (empty, for tracking)
/// - errors.log (empty)
/// - state.json (initial state)
pub fn init_loop_dir(project_dir: &Path, prompt: &str, config: &RehoboamConfig) -> Result<PathBuf> {
    // Create OTEL span for loop session initialization
    let _span = tracing::info_span!(
        "loop_session_init",
        project = %project_dir.display(),
        max_iterations = config.max_iterations,
        role = ?config.role,
        otel.kind = "internal",
    )
    .entered();

    let loop_dir = project_dir.join(".rehoboam");

    // Create directory (ok if exists)
    fs::create_dir_all(&loop_dir)?;

    info!("Initializing Rehoboam loop in {:?}", loop_dir);

    // Write anchor.md
    let anchor_content = format!(
        r#"# Rehoboam Loop Task

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
    fs::write(loop_dir.join("anchor.md"), anchor_content)?;

    // Write empty guardrails.md
    let guardrails_content = r"# Guardrails

Learned constraints from previous iterations. Check these before taking actions.

<!-- Signs will be added here as the loop progresses -->
";
    fs::write(loop_dir.join("guardrails.md"), guardrails_content)?;

    // Write empty progress.md
    let progress_content = r"# Progress

## Current Status
Starting iteration 1...

## Completed
<!-- Track completed work here -->

## Next Steps
<!-- Track remaining tasks here -->
";
    fs::write(loop_dir.join("progress.md"), progress_content)?;

    // Create empty errors.log
    fs::write(loop_dir.join("errors.log"), "")?;

    // Create empty activity.log
    fs::write(loop_dir.join("activity.log"), "")?;

    // Create empty session_history.log
    fs::write(loop_dir.join("session_history.log"), "")?;

    // NOTE: tasks.md is NO LONGER CREATED
    // Tasks are managed via Claude Code Tasks API (CLAUDE_CODE_TASK_LIST_ID)
    // Agents use TaskCreate/TaskUpdate/TaskList/TaskGet tools

    // Create coordination.md only if enabled (opt-in)
    // Per Cursor: "Workers never coordinate with each other"
    if config.enable_coordination {
        let coordination_content = r"# Coordination

Cross-agent discoveries and broadcasts. Only planners use this.

<!-- Format: [timestamp] [agent_id]: message -->
";
        fs::write(loop_dir.join("coordination.md"), coordination_content)?;
    }

    // Write state.json
    let state = LoopState {
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
        assigned_task: None,
    };
    let state_json = serde_json::to_string_pretty(&state)?;
    fs::write(loop_dir.join("state.json"), state_json)?;

    debug!("Rehoboam directory initialized: {:?}", loop_dir);
    Ok(loop_dir)
}

/// Load Rehoboam state from directory
pub fn load_state(loop_dir: &Path) -> Result<LoopState> {
    let state_path = loop_dir.join("state.json");
    let content =
        fs::read_to_string(&state_path).map_err(|e| eyre!("Failed to read state.json: {}", e))?;
    let state: LoopState =
        serde_json::from_str(&content).map_err(|e| eyre!("Failed to parse state.json: {}", e))?;
    Ok(state)
}

/// Save Rehoboam state to directory
pub fn save_state(loop_dir: &Path, state: &LoopState) -> Result<()> {
    let state_path = loop_dir.join("state.json");
    let content = serde_json::to_string_pretty(state)?;
    fs::write(state_path, content)?;
    Ok(())
}

/// Find the Rehoboam loop directory
///
/// Searches for `.rehoboam/` directory starting from cwd and going up.
///
/// With git worktrees, workers run in their own isolated worktree directories,
/// each with its own `.rehoboam/` folder. Standard discovery works for all roles:
/// - Planner: main repo's `.rehoboam/`
/// - Worker: worktree's `.rehoboam/` (created during auto-spawn)
/// - Auto: main repo's `.rehoboam/`
pub fn find_rehoboam_dir() -> Option<std::path::PathBuf> {
    // Search for .rehoboam/ directory starting from cwd
    let mut current = std::env::current_dir().ok()?;
    loop {
        let candidate = current.join(".rehoboam");
        if candidate.is_dir() && candidate.join("state.json").exists() {
            return Some(candidate);
        }

        if !current.pop() {
            break;
        }
    }

    None
}

/// Increment iteration counter
///
/// Records OTEL span for iteration lifecycle tracking.
pub fn increment_iteration(loop_dir: &Path) -> Result<u32> {
    let mut state = load_state(loop_dir)?;
    state.iteration += 1;

    // Create OTEL span for this iteration
    let _span = tracing::info_span!(
        "loop_iteration",
        iteration = state.iteration,
        max = state.max_iterations,
        role = ?state.role,
        otel.name = format!("iteration_{}", state.iteration),
    )
    .entered();

    save_state(loop_dir, &state)?;
    info!(
        iteration = state.iteration,
        max = state.max_iterations,
        "Rehoboam iteration incremented"
    );
    Ok(state.iteration)
}

/// Check if stop word is present in progress.md
pub fn check_stop_word(loop_dir: &Path, stop_word: &str) -> Result<bool> {
    let progress_path = loop_dir.join("progress.md");

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
pub fn check_max_iterations(loop_dir: &Path) -> Result<bool> {
    let state = load_state(loop_dir)?;
    let reached = state.iteration >= state.max_iterations;

    if reached {
        tracing::warn!(
            "Max iterations reached: {} >= {}",
            state.iteration,
            state.max_iterations
        );
    }

    Ok(reached)
}

/// Helper to read file content with default for missing files
pub fn read_file_content(loop_dir: &Path, filename: &str) -> Result<String> {
    let path = loop_dir.join(filename);
    if path.exists() {
        Ok(fs::read_to_string(path)?)
    } else {
        Ok(String::new())
    }
}
