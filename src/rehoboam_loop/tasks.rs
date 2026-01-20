//! Task Queue System (Cursor-aligned)
//!
//! Provides programmatic access to the tasks.md task queue.
//! Inspired by Cursor's "Scaling Agents" architecture where:
//! - Planners create tasks by exploring and decomposing work
//! - Workers execute tasks in isolation without coordination
//!
//! **Current Usage Pattern:**
//! Agents manage tasks.md directly via their role-specific prompts:
//! - Planners add tasks: `- [ ] [TASK-XXX] description`
//! - Workers mark complete: `- [x] [TASK-XXX] description`
//!
//! **Available APIs (for future automation):**
//! - `read_pending_tasks()` - Get all unclaimed tasks
//! - `read_next_task()` - Get the next available task
//! - `claim_task()` - Mark a task as "In Progress" with worker ID
//! - `complete_task()` - Move task from "In Progress" to "Completed"
//! - `add_task()` - Programmatically add a task to the queue

use color_eyre::eyre::Result;
use std::fs;
use std::path::Path;

/// A task in the queue
#[derive(Debug, Clone)]
pub struct Task {
    /// Task ID (e.g., "TASK-001")
    pub id: String,
    /// Task description
    pub description: String,
    /// Worker ID if claimed (e.g., "%42")
    pub worker: Option<String>,
}

/// Read all pending tasks from tasks.md
pub fn read_pending_tasks(loop_dir: &Path) -> Result<Vec<Task>> {
    let tasks_path = loop_dir.join("tasks.md");
    if !tasks_path.exists() {
        return Ok(vec![]);
    }

    let content = fs::read_to_string(&tasks_path)?;
    let mut tasks = Vec::new();
    let mut in_pending = false;

    for line in content.lines() {
        if line.starts_with("## Pending") {
            in_pending = true;
            continue;
        }
        if line.starts_with("## ") {
            in_pending = false;
            continue;
        }

        if in_pending && line.starts_with("- [ ] ") {
            if let Some(task) = parse_task_line(line) {
                tasks.push(task);
            }
        }
    }

    Ok(tasks)
}

/// Read the next available task from the queue
pub fn read_next_task(loop_dir: &Path) -> Result<Option<Task>> {
    let tasks = read_pending_tasks(loop_dir)?;
    Ok(tasks.into_iter().next())
}

/// Claim a task by moving it from Pending to In Progress
pub fn claim_task(loop_dir: &Path, task_id: &str, worker_id: &str) -> Result<()> {
    let tasks_path = loop_dir.join("tasks.md");
    let content = fs::read_to_string(&tasks_path)?;

    let mut new_lines = Vec::new();
    let mut claimed_task: Option<String> = None;
    let mut in_pending = false;
    let mut in_progress_section_exists = false;

    for line in content.lines() {
        if line.starts_with("## Pending") {
            in_pending = true;
            new_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("## In Progress") {
            in_pending = false;
            in_progress_section_exists = true;
            new_lines.push(line.to_string());
            // Insert claimed task here
            if let Some(ref task_desc) = claimed_task {
                new_lines.push(format!(
                    "- [~] [{}] {} (worker: {})",
                    task_id, task_desc, worker_id
                ));
            }
            continue;
        }
        if line.starts_with("## ") {
            in_pending = false;
        }

        // Check if this is the task to claim
        if in_pending && line.contains(&format!("[{}]", task_id)) {
            // Extract description (everything after the task ID)
            if let Some(task) = parse_task_line(line) {
                claimed_task = Some(task.description);
            }
            // Skip this line (don't add to new_lines)
            continue;
        }

        new_lines.push(line.to_string());
    }

    // If In Progress section doesn't exist, create it
    if !in_progress_section_exists && claimed_task.is_some() {
        // Find where to insert it (after Pending section)
        let mut insert_idx = None;
        for (i, line) in new_lines.iter().enumerate() {
            if line.starts_with("## Completed") {
                insert_idx = Some(i);
                break;
            }
        }
        if let Some(idx) = insert_idx {
            if let Some(ref task_desc) = claimed_task {
                new_lines.insert(idx, String::new());
                new_lines.insert(idx + 1, "## In Progress".to_string());
                new_lines.insert(
                    idx + 2,
                    format!("- [~] [{}] {} (worker: {})", task_id, task_desc, worker_id),
                );
            }
        }
    }

    fs::write(&tasks_path, new_lines.join("\n") + "\n")?;
    Ok(())
}

/// Complete a task by moving it from In Progress to Completed
pub fn complete_task(loop_dir: &Path, task_id: &str) -> Result<()> {
    let tasks_path = loop_dir.join("tasks.md");
    let content = fs::read_to_string(&tasks_path)?;

    let mut new_lines = Vec::new();
    let mut completed_task: Option<String> = None;
    let mut in_progress = false;

    for line in content.lines() {
        if line.starts_with("## In Progress") {
            in_progress = true;
            new_lines.push(line.to_string());
            continue;
        }
        if line.starts_with("## Completed") {
            in_progress = false;
            new_lines.push(line.to_string());
            // Insert completed task here
            if let Some(ref task_desc) = completed_task {
                new_lines.push(format!("- [x] [{}] {}", task_id, task_desc));
            }
            continue;
        }
        if line.starts_with("## ") {
            in_progress = false;
        }

        // Check if this is the task to complete
        if in_progress && line.contains(&format!("[{}]", task_id)) {
            // Extract description (everything after task ID, before worker annotation)
            if let Some(task) = parse_in_progress_line(line) {
                completed_task = Some(task.description);
            }
            continue;
        }

        new_lines.push(line.to_string());
    }

    fs::write(&tasks_path, new_lines.join("\n") + "\n")?;
    Ok(())
}

/// Add a new task to the Pending queue
pub fn add_task(loop_dir: &Path, task_id: &str, description: &str) -> Result<()> {
    let tasks_path = loop_dir.join("tasks.md");
    let content = fs::read_to_string(&tasks_path)?;

    let mut new_lines = Vec::new();
    let mut added = false;

    for line in content.lines() {
        new_lines.push(line.to_string());
        // Add after "## Pending" line
        if line.starts_with("## Pending") && !added {
            new_lines.push(format!("- [ ] [{}] {}", task_id, description));
            added = true;
        }
    }

    fs::write(&tasks_path, new_lines.join("\n") + "\n")?;
    Ok(())
}

/// Parse a pending task line: "- [ ] [TASK-001] description"
fn parse_task_line(line: &str) -> Option<Task> {
    let trimmed = line.trim_start_matches("- [ ] ");
    // Extract [TASK-ID] and description
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.find(']') {
            let id = trimmed[start + 1..end].to_string();
            let description = trimmed[end + 1..].trim().to_string();
            return Some(Task {
                id,
                description,
                worker: None,
            });
        }
    }
    None
}

/// Parse an in-progress task line: "- [~] [TASK-001] description (worker: %42)"
fn parse_in_progress_line(line: &str) -> Option<Task> {
    let trimmed = line.trim_start_matches("- [~] ");
    // Extract [TASK-ID]
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.find(']') {
            let id = trimmed[start + 1..end].to_string();
            let rest = &trimmed[end + 1..];
            // Extract description (before "(worker:")
            let description = if let Some(worker_start) = rest.find("(worker:") {
                rest[..worker_start].trim().to_string()
            } else {
                rest.trim().to_string()
            };
            // Extract worker ID if present
            let worker = rest.find("(worker:").and_then(|worker_start| {
                let worker_part = &rest[worker_start + 8..];
                worker_part
                    .find(')')
                    .map(|worker_end| worker_part[..worker_end].trim().to_string())
            });
            return Some(Task {
                id,
                description,
                worker,
            });
        }
    }
    None
}
