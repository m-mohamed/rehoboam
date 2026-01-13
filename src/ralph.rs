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
//! - state.json: Iteration counter, config

use chrono::{DateTime, Utc};
use color_eyre::eyre::{eyre, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

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
<!-- Track what needs to be done -->
"#;
    fs::write(ralph_dir.join("progress.md"), progress_content)?;

    // Create empty errors.log
    fs::write(ralph_dir.join("errors.log"), "")?;

    // Write state.json
    let state = RalphState {
        iteration: 0,
        max_iterations: config.max_iterations,
        stop_word: config.stop_word.clone(),
        started_at: Utc::now(),
        pane_id: config.pane_id.clone(),
        project_dir: project_dir.to_path_buf(),
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

    let prompt = format!(
        r#"# Ralph Loop - Iteration {iteration}

You are in a Ralph loop. Each iteration starts fresh - make incremental progress.

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
6. When done, write "{stop_word}" to progress.md
7. Exit when you've made progress (don't try to finish everything)

Remember: Progress persists, failures evaporate. Make incremental progress.
"#,
        iteration = state.iteration + 1, // Display as 1-indexed
        anchor = anchor,
        guardrails = guardrails,
        progress = progress,
        stop_word = state.stop_word,
    );

    // Write to a temp file for claude --prompt-file
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
}
