//! Stall Detection - Monitoring-only evaluation
//!
//! Detects loop stalls and completion via heuristics for TUI display.
//! Does NOT control loop behavior - that's handled by TeammateTool.
//!
//! Heuristics:
//! 1. Check stop word / promise tag in progress.md → COMPLETE
//! 2. Detect stalls (3+ iterations with no progress change) → STALLED
//! 3. Check max iterations reached → STALLED
//! 4. Otherwise → CONTINUE (default)

use color_eyre::eyre::Result;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use tracing::{debug, info, warn};

use super::activity::check_completion;
use super::state::{load_state, read_file_content, save_state};

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

/// Judge decision type (for monitoring/display purposes)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum JudgeDecision {
    /// Continue to next iteration
    Continue,
    /// Task is complete, stop the loop
    Complete,
    /// Task is stalled, needs intervention
    Stalled,
}

/// Detect completion or stall via heuristics (monitoring-only)
///
/// This function is for TUI display purposes. It does NOT control
/// the loop - TeammateTool's requestShutdown handles termination.
///
/// # Heuristics
/// 1. Stop word / promise tag in progress.md → COMPLETE
/// 2. Stall detection (3+ iterations same progress) → STALLED
/// 3. Max iterations reached → STALLED
/// 4. Otherwise → CONTINUE
///
/// # Returns
/// * `Ok((decision, confidence, explanation))` - The evaluation result
/// * `Err` - If evaluation fails
pub fn judge_completion(loop_dir: &Path) -> Result<(JudgeDecision, f64, String)> {
    let state = load_state(loop_dir)?;

    // HEURISTIC 1: Check stop word / promise tag
    let (is_complete, reason) = check_completion(loop_dir, &state.stop_word)?;
    if is_complete {
        info!(reason = %reason, "Detected COMPLETE via heuristic (stop word/promise)");
        return Ok((
            JudgeDecision::Complete,
            1.0,
            format!("Heuristic: {}", reason),
        ));
    }

    // HEURISTIC 2: Check "PLANNING COMPLETE" marker
    // Note: This is role-agnostic - any agent can use this marker
    let progress = read_file_content(loop_dir, "progress.md")?;
    if progress.to_uppercase().contains("PLANNING COMPLETE") {
        info!("Detected COMPLETE via heuristic (planning complete marker)");
        return Ok((
            JudgeDecision::Complete,
            1.0,
            "Heuristic: planning_complete".to_string(),
        ));
    }

    // HEURISTIC 3: Stall detection
    let stall_count = track_progress_stall(loop_dir)?;
    if stall_count >= STALL_THRESHOLD {
        warn!(
            stall_count = stall_count,
            threshold = STALL_THRESHOLD,
            "Detected STALLED via heuristic (no progress)"
        );
        return Ok((
            JudgeDecision::Stalled,
            0.8,
            format!("Heuristic: stalled for {} iterations", stall_count),
        ));
    }

    // HEURISTIC 4: Check max iterations
    if state.iteration >= state.max_iterations {
        info!(
            iteration = state.iteration,
            max = state.max_iterations,
            "Detected STALLED via heuristic (max iterations)"
        );
        return Ok((
            JudgeDecision::Stalled,
            1.0,
            "Heuristic: max_iterations_reached".to_string(),
        ));
    }

    // DEFAULT: Continue
    debug!(
        iteration = state.iteration,
        stall_count = stall_count,
        "No completion/stall detected, continuing"
    );
    Ok((
        JudgeDecision::Continue,
        0.7,
        "Heuristic: no completion signal detected".to_string(),
    ))
}
