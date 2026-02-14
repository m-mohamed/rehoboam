//! History scanner for ~/.claude/history.jsonl
//!
//! Parses the global timeline of user inputs across all projects.
//! Each line is a JSON object with display text, timestamp, project path, etc.

use color_eyre::eyre;
use std::path::PathBuf;

/// A single history entry from the JSONL file
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used by UI renderer
pub struct HistoryEntry {
    /// User input text (truncated by Claude Code)
    pub display: String,
    /// Unix timestamp in milliseconds
    pub timestamp: i64,
    /// Full project path
    pub project: String,
    /// Session ID
    pub session_id: String,
    /// Whether pastedContents was non-empty
    pub has_pasted: bool,
}

pub struct HistoryDiscovery;

impl HistoryDiscovery {
    /// Parse ~/.claude/history.jsonl, returning entries newest first, capped at 500
    pub fn scan_history() -> eyre::Result<Vec<HistoryEntry>> {
        let path = Self::history_path()?;
        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(&path)?;
        let mut entries: Vec<HistoryEntry> = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| {
                let json: serde_json::Value = serde_json::from_str(line).ok()?;

                let display = json
                    .get("display")
                    .or_else(|| json.get("text"))
                    .or_else(|| json.get("input"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let timestamp = json
                    .get("timestamp")
                    .or_else(|| json.get("ts"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);

                let project = json
                    .get("project")
                    .or_else(|| json.get("cwd"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let session_id = json
                    .get("sessionId")
                    .or_else(|| json.get("session_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let has_pasted = json
                    .get("pastedContents")
                    .map(|v| {
                        // pastedContents can be a string, array, or object
                        // Empty object {} means no pasted content
                        v.as_str().is_some_and(|s| !s.is_empty())
                            || v.as_array().is_some_and(|a| !a.is_empty())
                            || v.as_object().is_some_and(|o| !o.is_empty())
                    })
                    .unwrap_or(false);

                Some(HistoryEntry {
                    display,
                    timestamp,
                    project,
                    session_id,
                    has_pasted,
                })
            })
            .collect();

        // Sort by timestamp descending (newest first)
        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

        // Cap at 500 entries
        entries.truncate(500);

        Ok(entries)
    }

    fn history_path() -> eyre::Result<PathBuf> {
        let home =
            std::env::var("HOME").map_err(|_| eyre::eyre!("HOME not set"))?;
        Ok(PathBuf::from(home).join(".claude").join("history.jsonl"))
    }
}
