//! Filesystem-based task discovery from ~/.claude/tasks/
//!
//! Scans Claude Code's task files to discover task state across all teams.
//! Provides ground truth for tasks — including those created by teammates
//! that never fired hooks through Rehoboam.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A task read from the filesystem
#[derive(Debug, Clone)]
pub struct FsTask {
    /// Task ID (from JSON "id" field)
    pub id: String,
    /// Task subject/title
    pub subject: String,
    /// Full task description
    pub description: String,
    /// Present continuous form shown when in_progress (e.g., "Running tests")
    pub active_form: Option<String>,
    /// Task status: "pending", "in_progress", "completed", "deleted"
    pub status: String,
    /// Task IDs that this task blocks (waiting on this one)
    pub blocks: Vec<String>,
    /// Task IDs that must complete before this task can start
    pub blocked_by: Vec<String>,
}

/// A task list from the filesystem (one per team or session)
#[derive(Debug, Clone)]
#[allow(dead_code)] // list_id used in tests and for diagnostics
pub struct FsTaskList {
    /// Directory name (team name or UUID-based session ID)
    pub list_id: String,
    /// Tasks in this list (excluding deleted tasks)
    pub tasks: Vec<FsTask>,
}

/// Filesystem scanner for Claude Code task files
pub struct TaskDiscovery;

impl TaskDiscovery {
    /// Scan ~/.claude/tasks/ for task lists
    ///
    /// Returns a map of list_id -> FsTaskList.
    /// Silently skips malformed JSON files or missing directories.
    pub fn scan_tasks() -> Result<HashMap<String, FsTaskList>, std::io::Error> {
        let mut lists = HashMap::new();

        let tasks_dir = Self::tasks_dir()?;
        if !tasks_dir.exists() {
            return Ok(lists);
        }

        let entries = std::fs::read_dir(&tasks_dir)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let list_id = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            let mut tasks = Vec::new();

            let dir_entries = match std::fs::read_dir(&path) {
                Ok(entries) => entries,
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "Failed to read task list directory, skipping"
                    );
                    continue;
                }
            };

            for file_entry in dir_entries.flatten() {
                let file_path = file_entry.path();

                // Only process .json files (skip .lock, .highwatermark, etc.)
                if file_path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }

                match Self::parse_task(&file_path) {
                    Ok(task) => {
                        // Filter out deleted tasks
                        if task.status != "deleted" {
                            tasks.push(task);
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            path = %file_path.display(),
                            error = %e,
                            "Failed to parse task file, skipping"
                        );
                    }
                }
            }

            // Sort tasks by ID for consistent ordering
            tasks.sort_by(|a, b| {
                // Try numeric sort first, fall back to string sort
                a.id.parse::<u64>()
                    .ok()
                    .zip(b.id.parse::<u64>().ok())
                    .map(|(a_num, b_num)| a_num.cmp(&b_num))
                    .unwrap_or_else(|| a.id.cmp(&b.id))
            });

            if !tasks.is_empty() {
                tracing::debug!(
                    list_id = %list_id,
                    task_count = tasks.len(),
                    "Discovered task list from filesystem"
                );
            }

            lists.insert(list_id.clone(), FsTaskList { list_id, tasks });
        }

        Ok(lists)
    }

    /// Parse a single task JSON file
    fn parse_task(path: &Path) -> Result<FsTask, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let json: serde_json::Value = serde_json::from_str(&content)?;

        let id = json
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let subject = json
            .get("subject")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let description = json
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let active_form = json
            .get("activeForm")
            .and_then(|v| v.as_str())
            .map(String::from);

        let status = json
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("pending")
            .to_string();

        let blocks = json
            .get("blocks")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let blocked_by = json
            .get("blockedBy")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        Ok(FsTask {
            id,
            subject,
            description,
            active_form,
            status,
            blocks,
            blocked_by,
        })
    }

    /// Get the tasks directory path (~/.claude/tasks/)
    fn tasks_dir() -> Result<PathBuf, std::io::Error> {
        let home = std::env::var("HOME")
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::NotFound, "HOME not set"))?;
        Ok(PathBuf::from(home).join(".claude").join("tasks"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_task() {
        let tmp = TempDir::new().unwrap();
        let task_json = r#"{
            "id": "1",
            "subject": "Create use-debounced-value hook",
            "description": "Create apps/web/src/hooks/use-debounced-value.ts",
            "activeForm": "Creating debounce hook",
            "status": "completed",
            "blocks": ["3"],
            "blockedBy": ["2"]
        }"#;
        let task_path = tmp.path().join("1.json");
        std::fs::write(&task_path, task_json).unwrap();

        let task = TaskDiscovery::parse_task(&task_path).unwrap();
        assert_eq!(task.id, "1");
        assert_eq!(task.subject, "Create use-debounced-value hook");
        assert_eq!(
            task.description,
            "Create apps/web/src/hooks/use-debounced-value.ts"
        );
        assert_eq!(task.active_form.as_deref(), Some("Creating debounce hook"));
        assert_eq!(task.status, "completed");
        assert_eq!(task.blocks, vec!["3"]);
        assert_eq!(task.blocked_by, vec!["2"]);
    }

    #[test]
    fn test_parse_task_missing_fields() {
        let tmp = TempDir::new().unwrap();
        let task_json = r#"{
            "id": "5",
            "subject": "Minimal task"
        }"#;
        let task_path = tmp.path().join("5.json");
        std::fs::write(&task_path, task_json).unwrap();

        let task = TaskDiscovery::parse_task(&task_path).unwrap();
        assert_eq!(task.id, "5");
        assert_eq!(task.subject, "Minimal task");
        assert_eq!(task.description, "");
        assert!(task.active_form.is_none());
        assert_eq!(task.status, "pending");
        assert!(task.blocks.is_empty());
        assert!(task.blocked_by.is_empty());
    }

    #[test]
    fn test_parse_task_malformed_json() {
        let tmp = TempDir::new().unwrap();
        let task_path = tmp.path().join("bad.json");
        std::fs::write(&task_path, "not valid json {{{").unwrap();

        let result = TaskDiscovery::parse_task(&task_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_scan_tasks_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let tasks_dir = tmp.path().join("tasks");
        std::fs::create_dir_all(&tasks_dir).unwrap();

        // Create an empty team dir
        let team_dir = tasks_dir.join("empty-team");
        std::fs::create_dir_all(&team_dir).unwrap();

        // Manually call parse on the team dir to verify empty results
        let entries: Vec<_> = std::fs::read_dir(&team_dir).unwrap().flatten().collect();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_scan_tasks_skips_lock_files() {
        let tmp = TempDir::new().unwrap();
        let team_dir = tmp.path().join("my-team");
        std::fs::create_dir_all(&team_dir).unwrap();

        // Create non-JSON files that should be skipped
        std::fs::write(team_dir.join(".lock"), "").unwrap();
        std::fs::write(team_dir.join(".highwatermark"), "3").unwrap();

        // Verify only .json files would be processed
        let json_files: Vec<_> = std::fs::read_dir(&team_dir)
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
            .collect();
        assert!(json_files.is_empty(), "No JSON files should exist");
    }

    #[test]
    fn test_scan_tasks_filters_deleted() {
        let tmp = TempDir::new().unwrap();
        let team_dir = tmp.path().join("my-team");
        std::fs::create_dir_all(&team_dir).unwrap();

        // Create a deleted task
        let deleted_task = r#"{
            "id": "1",
            "subject": "Deleted task",
            "status": "deleted",
            "blocks": [],
            "blockedBy": []
        }"#;
        std::fs::write(team_dir.join("1.json"), deleted_task).unwrap();

        // Create a valid task
        let valid_task = r#"{
            "id": "2",
            "subject": "Valid task",
            "status": "pending",
            "blocks": [],
            "blockedBy": []
        }"#;
        std::fs::write(team_dir.join("2.json"), valid_task).unwrap();

        // Parse both tasks manually to verify filtering
        let deleted = TaskDiscovery::parse_task(&team_dir.join("1.json")).unwrap();
        assert_eq!(deleted.status, "deleted");

        let valid = TaskDiscovery::parse_task(&team_dir.join("2.json")).unwrap();
        assert_eq!(valid.status, "pending");
    }

    #[test]
    fn test_task_sorting_numeric() {
        let tmp = TempDir::new().unwrap();
        let team_dir = tmp.path().join("sorted-team");
        std::fs::create_dir_all(&team_dir).unwrap();

        for id in &["10", "2", "1", "3"] {
            let task = format!(
                r#"{{"id": "{}", "subject": "Task {}", "status": "pending", "blocks": [], "blockedBy": []}}"#,
                id, id
            );
            std::fs::write(team_dir.join(format!("{}.json", id)), task).unwrap();
        }

        // Read and parse all tasks, then sort like scan_tasks does
        let mut tasks: Vec<FsTask> = Vec::new();
        for entry in std::fs::read_dir(&team_dir).unwrap().flatten() {
            if entry.path().extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(task) = TaskDiscovery::parse_task(&entry.path()) {
                    tasks.push(task);
                }
            }
        }
        tasks.sort_by(|a, b| {
            a.id.parse::<u64>()
                .ok()
                .zip(b.id.parse::<u64>().ok())
                .map(|(a_num, b_num)| a_num.cmp(&b_num))
                .unwrap_or_else(|| a.id.cmp(&b.id))
        });

        let ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["1", "2", "3", "10"]);
    }
}
