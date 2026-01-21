//! Judge Mode - Cursor-inspired evaluation phase
//!
//! Evaluates loop progress to determine if the task is complete,
//! should continue, or is stalled.
//!
//! Judge = Claude Code. No heuristics, no fallback.
//! An ephemeral Claude session evaluates anchor.md + progress.md.

use color_eyre::eyre::{eyre, Result};
use std::path::Path;
use std::process::{Command, Stdio};

use super::state::read_file_content;

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
/// based on anchor.md (task spec) and progress.md (work done).
fn build_judge_prompt(loop_dir: &Path) -> Result<String> {
    let anchor = read_file_content(loop_dir, "anchor.md")?;
    let progress = read_file_content(loop_dir, "progress.md")?;

    Ok(format!(
        r#"You are a judge evaluating whether a coding task is complete.

TASK SPECIFICATION:
{anchor}

PROGRESS REPORT:
{progress}

Evaluate the progress against the task specification.

Respond with EXACTLY ONE LINE containing only one of:
CONTINUE
COMPLETE
STALLED

No explanation needed - just the single word decision."#
    ))
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

/// Evaluate task completion via Claude Code
///
/// Judge = Claude Code. Spawns an ephemeral Claude session to evaluate
/// the current loop state (anchor.md + progress.md).
///
/// # Arguments
/// * `loop_dir` - Path to the .rehoboam/ directory containing state files
///
/// # Returns
/// * `Ok((decision, confidence, explanation))` - The Judge's evaluation
/// * `Err` - If Claude Code fails to spawn or returns an error
pub fn judge_completion(loop_dir: &Path) -> Result<(JudgeDecision, f64, String)> {
    let prompt = build_judge_prompt(loop_dir)?;

    tracing::debug!(prompt_len = prompt.len(), "Running Judge via Claude Code");

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
        return Err(eyre!(
            "Judge (Claude Code) exited with error: {}",
            stderr.trim()
        ));
    }

    let response = String::from_utf8_lossy(&output.stdout);
    tracing::debug!(response = %response.trim(), "Judge response");

    let decision = parse_judge_response(&response)?;
    let explanation = format!("Judge: {:?}", decision);

    tracing::info!(decision = ?decision, "Judge evaluation complete");
    Ok((decision, 0.9, explanation))
}
