//! Judge Mode - Cursor-inspired evaluation phase
//!
//! Evaluates loop progress to determine if the task is complete,
//! should continue, or is stalled.
//!
//! **HEURISTIC FALLBACK** (Simplification from strategic analysis):
//! 1. Check stop word / promise tag in progress.md → COMPLETE
//! 2. Check "PLANNING COMPLETE" for planners → COMPLETE
//! 3. Detect stalls (3+ iterations with no progress change) → STALLED
//! 4. Only spawn Claude for ambiguous cases
//!
//! **REQUIRES**: CLAUDE_CODE_TASK_LIST_ID environment variable for Claude fallback.

use color_eyre::eyre::{eyre, Result};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::{Command, Stdio};
use tracing::{debug, info, warn};

use super::activity::check_completion;
use super::state::{load_state, read_file_content, save_state, LoopRole};

/// Get the Claude Code Task List ID, or error if not set
fn require_task_list_id() -> Result<String> {
    std::env::var("CLAUDE_CODE_TASK_LIST_ID").map_err(|_| {
        eyre!("CLAUDE_CODE_TASK_LIST_ID not set. Judge requires Claude Code Tasks API.")
    })
}

/// Number of consecutive stall iterations before declaring STALLED
const STALL_THRESHOLD: u32 = 3;

/// Compute a hash of progress.md content for stall detection
fn compute_progress_hash(loop_dir: &Path) -> u64 {
    let progress = read_file_content(loop_dir, "progress.md").unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    // Normalize whitespace for consistent hashing
    progress
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .hash(&mut hasher);
    hasher.finish()
}

/// Track progress hash and return stall count
///
/// Stores last progress hash in state. Returns consecutive iterations
/// with same progress content.
fn track_progress_stall(loop_dir: &Path) -> Result<u32> {
    let current_hash = compute_progress_hash(loop_dir);
    let mut state = load_state(loop_dir)?;

    // Get or initialize stall tracking
    let last_hash = state
        .error_counts
        .get("_progress_hash")
        .map(|h| *h as u64)
        .unwrap_or(0);
    let stall_count = state.error_counts.get("_stall_count").copied().unwrap_or(0);

    let new_stall_count = if current_hash == last_hash && last_hash != 0 {
        stall_count + 1
    } else {
        0
    };

    // Update state
    state
        .error_counts
        .insert("_progress_hash".to_string(), current_hash as u32);
    state
        .error_counts
        .insert("_stall_count".to_string(), new_stall_count);
    save_state(loop_dir, &state)?;

    debug!(
        progress_hash = current_hash,
        stall_count = new_stall_count,
        "Progress stall tracking"
    );

    Ok(new_stall_count)
}

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

/// Build judge evaluation prompt from loop state files
///
/// Creates a structured prompt for Claude Code to evaluate task completion
/// based on anchor.md (task spec), progress.md (work done), and TaskList status.
///
/// **REQUIRES**: CLAUDE_CODE_TASK_LIST_ID environment variable
fn build_judge_prompt(loop_dir: &Path) -> Result<String> {
    // Fail early if Tasks API not configured
    let task_list_id = require_task_list_id()?;

    let anchor = read_file_content(loop_dir, "anchor.md")?;
    let progress = read_file_content(loop_dir, "progress.md")?;

    let prompt = format!(
        r#"You are a judge evaluating whether a coding task is complete.

TASK LIST ID: {task_list_id}

TASK SPECIFICATION:
{anchor}

PROGRESS REPORT:
{progress}

EVALUATION INSTRUCTIONS:
1. First, use `TaskList` to check the status of all tasks
2. If ALL tasks have status "completed", the overall task is COMPLETE
3. If some tasks are still "pending" or "in_progress", evaluate if meaningful progress is being made
4. If no progress has been made across multiple iterations, the task is STALLED

DECISION CRITERIA:
- COMPLETE: All TaskList tasks are completed AND progress.md shows the goal is met
- CONTINUE: Some tasks remain but progress is being made
- STALLED: No meaningful progress, repeated failures, or all workers are blocked

Respond with EXACTLY ONE LINE containing only one of:
CONTINUE
COMPLETE
STALLED

No explanation needed - just the single word decision."#,
        task_list_id = task_list_id,
        anchor = anchor,
        progress = progress,
    );

    Ok(prompt)
}

/// Parse judge response from LLM output
///
/// Looks for CONTINUE, COMPLETE, or STALLED keywords in the response.
/// Defaults to Continue if no clear decision is found.
fn parse_judge_response(response: &str) -> Result<JudgeDecision> {
    let response_upper = response.to_uppercase();

    // Look for keywords in the response
    if response_upper.contains("COMPLETE") {
        Ok(JudgeDecision::Complete)
    } else if response_upper.contains("STALLED") {
        Ok(JudgeDecision::Stalled)
    } else if response_upper.contains("CONTINUE") {
        Ok(JudgeDecision::Continue)
    } else {
        // Default to continue if no clear decision
        tracing::warn!(
            response = %response.trim(),
            "Judge response unclear, defaulting to Continue"
        );
        Ok(JudgeDecision::Continue)
    }
}

/// Evaluate task completion using heuristics first, Claude fallback
///
/// **HEURISTIC PRIORITY** (fast, deterministic):
/// 1. Stop word / promise tag in progress.md → COMPLETE (1.0 confidence)
/// 2. "PLANNING COMPLETE" for Planner role → COMPLETE (1.0 confidence)
/// 3. Stall detection (3+ iterations same progress) → STALLED (0.8 confidence)
/// 4. Only ambiguous cases spawn Claude (0.9 confidence)
///
/// # Arguments
/// * `loop_dir` - Path to the .rehoboam/ directory containing state files
///
/// # Returns
/// * `Ok((decision, confidence, explanation))` - The Judge's evaluation
/// * `Err` - If evaluation fails
pub fn judge_completion(loop_dir: &Path) -> Result<(JudgeDecision, f64, String)> {
    let state = load_state(loop_dir)?;

    // HEURISTIC 1: Check stop word / promise tag
    let (is_complete, reason) = check_completion(loop_dir, &state.stop_word)?;
    if is_complete {
        info!(reason = %reason, "Judge: COMPLETE via heuristic (stop word/promise)");
        return Ok((
            JudgeDecision::Complete,
            1.0,
            format!("Heuristic: {}", reason),
        ));
    }

    // HEURISTIC 2: Check "PLANNING COMPLETE" for Planner role
    if state.role == LoopRole::Planner {
        let progress = read_file_content(loop_dir, "progress.md")?;
        if progress.to_uppercase().contains("PLANNING COMPLETE") {
            info!("Judge: COMPLETE via heuristic (planning complete)");
            return Ok((
                JudgeDecision::Complete,
                1.0,
                "Heuristic: planning_complete".to_string(),
            ));
        }
    }

    // HEURISTIC 3: Stall detection
    let stall_count = track_progress_stall(loop_dir)?;
    if stall_count >= STALL_THRESHOLD {
        warn!(
            stall_count = stall_count,
            threshold = STALL_THRESHOLD,
            "Judge: STALLED via heuristic (no progress)"
        );
        return Ok((
            JudgeDecision::Stalled,
            0.8,
            format!("Heuristic: stalled for {} iterations", stall_count),
        ));
    }

    // HEURISTIC 4: Check max iterations (should continue but warn)
    if state.iteration >= state.max_iterations {
        info!(
            iteration = state.iteration,
            max = state.max_iterations,
            "Judge: STALLED via heuristic (max iterations)"
        );
        return Ok((
            JudgeDecision::Stalled,
            1.0,
            "Heuristic: max_iterations_reached".to_string(),
        ));
    }

    // FALLBACK: Spawn Claude for ambiguous cases
    info!("Judge: No heuristic match, falling back to Claude evaluation");
    judge_with_claude(loop_dir)
}

/// Spawn Claude Code for Judge evaluation (fallback)
///
/// Only called when heuristics don't provide a clear answer.
fn judge_with_claude(loop_dir: &Path) -> Result<(JudgeDecision, f64, String)> {
    let prompt = build_judge_prompt(loop_dir)?;

    debug!(prompt_len = prompt.len(), "Running Judge via Claude Code");

    // Run Claude Code with -p flag for non-interactive execution
    let output = Command::new("claude")
        .arg("-p")
        .arg(&prompt)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| eyre!("Failed to spawn claude for Judge: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // If Claude fails, default to Continue rather than error
        warn!(
            stderr = %stderr.trim(),
            "Claude Judge failed, defaulting to Continue"
        );
        return Ok((
            JudgeDecision::Continue,
            0.5,
            "Claude error, defaulting to continue".to_string(),
        ));
    }

    let response = String::from_utf8_lossy(&output.stdout);
    debug!(response = %response.trim(), "Judge response");

    let decision = parse_judge_response(&response)?;
    let explanation = format!("Claude: {:?}", decision);

    info!(decision = ?decision, "Judge evaluation complete (Claude)");
    Ok((decision, 0.9, explanation))
}
