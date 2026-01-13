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
}

/// Configuration for starting a Ralph loop
#[derive(Debug, Clone)]
pub struct RalphConfig {
    pub max_iterations: u32,
    pub stop_word: String,
    pub pane_id: String,
}

impl Default for RalphConfig {
    fn default() -> Self {
        Self {
            max_iterations: 50,
            stop_word: "DONE".to_string(),
            pane_id: String::new(),
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

/// Build the iteration prompt that includes state files
///
/// This creates a prompt file that tells Claude:
/// - What iteration it's on
/// - The anchor (task spec)
/// - Any guardrails
/// - Progress so far
pub fn build_iteration_prompt(ralph_dir: &Path) -> Result<String> {
    let state = load_state(ralph_dir)?;

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

    let prompt = format!(
        r#"# Ralph Loop - Iteration {iteration}

You are in a Ralph loop. Each iteration starts fresh - make incremental progress.

{recent_section}
## Your Task (Anchor)
{anchor}

## Learned Constraints (Guardrails)
{guardrails}

## Progress So Far
{progress}

## Instructions for This Iteration
1. Read the anchor to understand your task
2. Check guardrails before taking actions
3. Continue from where progress.md left off
4. Update progress.md with your work
5. If you hit a repeating problem, add a SIGN to guardrails.md
6. When ALL criteria are met, write either:
   - "{stop_word}" anywhere in progress.md, OR
   - <promise>COMPLETE</promise> tag (more explicit)
7. Exit when you've made progress (don't try to finish everything)

Remember: Progress persists, failures evaporate. Make incremental progress.
Git commits are created between iterations for easy rollback.
"#,
        iteration = state.iteration + 1, // Display as 1-indexed
        recent_section = recent_section,
        anchor = anchor,
        guardrails = guardrails,
        progress = progress,
        stop_word = state.stop_word,
    );

    // Write to a temp file for claude stdin piping
    let prompt_file = ralph_dir.join("_iteration_prompt.md");
    fs::write(&prompt_file, &prompt)?;

    debug!(
        "Built iteration prompt for iteration {}",
        state.iteration + 1
    );
    Ok(prompt_file.to_string_lossy().to_string())
}

/// Add a guardrail/sign to guardrails.md
#[allow(dead_code)]
pub fn add_guardrail(ralph_dir: &Path, sign: &str, trigger: &str, instruction: &str) -> Result<()> {
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

/// Log an error to errors.log
#[allow(dead_code)]
pub fn log_error(ralph_dir: &Path, error: &str) -> Result<()> {
    let state = load_state(ralph_dir)?;
    let errors_path = ralph_dir.join("errors.log");

    let entry = format!(
        "[Iteration {}] [{}] {}\n",
        state.iteration,
        Utc::now().format("%Y-%m-%d %H:%M:%S"),
        error
    );

    let mut content = fs::read_to_string(&errors_path).unwrap_or_default();
    content.push_str(&entry);
    fs::write(errors_path, content)?;

    warn!("Logged error: {}", error);
    Ok(())
}

/// Check if a Ralph loop is active in a directory
#[allow(dead_code)]
pub fn is_ralph_active(project_dir: &Path) -> bool {
    let ralph_dir = project_dir.join(".ralph");
    let state_path = ralph_dir.join("state.json");
    state_path.exists()
}

/// Get the Ralph directory for a project
#[allow(dead_code)]
pub fn get_ralph_dir(project_dir: &Path) -> PathBuf {
    project_dir.join(".ralph")
}

/// Clean up Ralph directory (for cancellation or completion)
#[allow(dead_code)]
pub fn cleanup_ralph_dir(ralph_dir: &Path) -> Result<()> {
    if ralph_dir.exists() {
        // Don't delete - rename to .ralph.done for history
        let done_dir = ralph_dir.with_extension("done");
        if done_dir.exists() {
            fs::remove_dir_all(&done_dir)?;
        }
        fs::rename(ralph_dir, &done_dir)?;
        info!("Ralph loop archived to {:?}", done_dir);
    }
    Ok(())
}

// =============================================================================
// NEW: Git Checkpoints
// =============================================================================

/// Create a git checkpoint after an iteration completes
///
/// Commits all changes with a message indicating the Ralph iteration.
/// Returns the commit hash if successful.
pub fn create_git_checkpoint(ralph_dir: &Path) -> Result<Option<String>> {
    let state = load_state(ralph_dir)?;
    let project_dir = ralph_dir.parent().ok_or_else(|| eyre!("Invalid ralph dir"))?;

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
        content = lines[lines.len() - MAX_SESSION_HISTORY + 1..]
            .join("\n")
            + "\n";
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

/// Append to guardrails without full sign format (for simple additions)
#[allow(dead_code)]
pub fn append_guardrail(ralph_dir: &Path, content: &str) -> Result<()> {
    let guardrails_path = ralph_dir.join("guardrails.md");
    let mut existing = fs::read_to_string(&guardrails_path).unwrap_or_default();
    existing.push_str(content);
    fs::write(guardrails_path, existing)?;
    Ok(())
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
        fs::write(
            ralph_dir.join("progress.md"),
            "<promise>COMPLETE</promise>",
        )
        .unwrap();
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
}
