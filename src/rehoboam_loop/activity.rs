//! Activity logging and iteration tracking
//!
//! Provides logging for iteration metrics, session transitions,
//! and auto-guardrails from error patterns.

use chrono::Utc;
use color_eyre::eyre::Result;
use std::fs;
use std::path::Path;
use tracing::{debug, info};

use super::state::{load_state, save_state};

/// Promise tag for explicit completion signal
pub const PROMISE_COMPLETE_TAG: &str = "<promise>COMPLETE</promise>";

/// Max session history entries to keep
const MAX_SESSION_HISTORY: usize = 50;

/// Threshold for auto-adding guardrails (error seen N times)
const AUTO_GUARDRAIL_THRESHOLD: u32 = 3;

/// Log activity metrics for an iteration
///
/// Records timing, tool calls, and other metrics to activity.log
pub fn log_activity(
    loop_dir: &Path,
    iteration: u32,
    duration_secs: Option<u64>,
    tool_calls: Option<u32>,
    completion_reason: &str,
) -> Result<()> {
    let activity_path = loop_dir.join("activity.log");

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
pub fn mark_iteration_start(loop_dir: &Path) -> Result<()> {
    let mut state = load_state(loop_dir)?;
    state.iteration_started_at = Some(Utc::now());
    save_state(loop_dir, &state)?;
    Ok(())
}

/// Get iteration duration in seconds
pub fn get_iteration_duration(loop_dir: &Path) -> Option<u64> {
    let state = load_state(loop_dir).ok()?;
    let started = state.iteration_started_at?;
    let duration = Utc::now().signed_duration_since(started);
    Some(duration.num_seconds().max(0) as u64)
}

/// Check if the <promise>COMPLETE</promise> tag is present in progress.md
///
/// This is a more explicit completion signal than stop word matching.
pub fn check_promise_tag(loop_dir: &Path) -> Result<bool> {
    let progress_path = loop_dir.join("progress.md");

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
pub fn check_completion(loop_dir: &Path, stop_word: &str) -> Result<(bool, String)> {
    use super::state::check_stop_word;

    // Check promise tag first (more explicit)
    if check_promise_tag(loop_dir)? {
        return Ok((true, "promise_tag".to_string()));
    }

    // Check stop word
    if check_stop_word(loop_dir, stop_word)? {
        return Ok((true, "stop_word".to_string()));
    }

    Ok((false, String::new()))
}

/// Log a session state transition
///
/// Records transitions like: started -> working -> stopped -> respawning
pub fn log_session_transition(
    loop_dir: &Path,
    from_state: &str,
    to_state: &str,
    details: Option<&str>,
) -> Result<()> {
    let history_path = loop_dir.join("session_history.log");
    let state = load_state(loop_dir)?;

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

/// Track an error and return true if it should trigger auto-guardrail
///
/// If the same error pattern appears AUTO_GUARDRAIL_THRESHOLD times,
/// automatically adds it to guardrails.md
pub fn track_error_pattern(loop_dir: &Path, error: &str) -> Result<bool> {
    let mut state = load_state(loop_dir)?;

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

    save_state(loop_dir, &state)?;

    // Check if we hit the threshold
    if current_count == AUTO_GUARDRAIL_THRESHOLD {
        // Auto-add guardrail
        let sign_name = format!("Auto-detected: {}", &error_key[..error_key.len().min(30)]);
        let trigger = error.chars().take(200).collect::<String>();
        let instruction = format!(
            "This error has occurred {} times. Review the approach and try a different strategy.",
            current_count
        );

        add_guardrail(loop_dir, &sign_name, &trigger, &instruction)?;
        info!(
            "Auto-added guardrail for repeated error: {} ({} occurrences)",
            error_key, current_count
        );
        return Ok(true);
    }

    Ok(false)
}

/// Add a guardrail/sign to guardrails.md
fn add_guardrail(loop_dir: &Path, sign: &str, trigger: &str, instruction: &str) -> Result<()> {
    let state = load_state(loop_dir)?;
    let guardrails_path = loop_dir.join("guardrails.md");

    let entry = format!(
        r"
### Sign: {sign}
- **Trigger:** {trigger}
- **Instruction:** {instruction}
- **Added:** Iteration {iteration}
",
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

/// Get summary of recent iteration outcomes for context injection
///
/// Returns the last N entries from activity.log formatted for inclusion
/// in the iteration prompt. This helps Claude understand recent progress.
pub fn get_recent_progress_summary(loop_dir: &Path, count: usize) -> Result<String> {
    let activity_path = loop_dir.join("activity.log");

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
