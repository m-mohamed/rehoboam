//! Judge Mode - Cursor-inspired evaluation phase
//!
//! Evaluates loop progress to determine if the task is complete,
//! should continue, or is stalled.

use color_eyre::eyre::Result;
use std::path::Path;

use super::state::read_file_content;

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

/// Evaluate completion using judge heuristics
///
/// Analyzes progress.md against anchor.md to determine if the task is complete.
/// This is a simple heuristic-based judge - a full implementation would spawn
/// a separate Claude session to evaluate.
///
/// Returns (decision, confidence, reason)
pub fn judge_completion(loop_dir: &Path) -> Result<(JudgeDecision, f64, String)> {
    let progress = read_file_content(loop_dir, "progress.md")?;
    let anchor = read_file_content(loop_dir, "anchor.md")?;
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
