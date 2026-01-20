//! Multi-Agent Coordination
//!
//! Provides coordination between multiple agents working on the same loop.
//! Includes broadcast messaging and worker registration.

use chrono::{DateTime, Utc};
use color_eyre::eyre::{eyre, Result};
use std::fs;
use std::path::Path;
use tracing::{debug, info};

use super::state::load_state;

/// Broadcast a message to coordination.md for other agents to read
///
/// Messages are appended with timestamp and agent ID.
/// Format: `[2025-01-15T12:34:56Z] [agent-id]: message`
pub fn broadcast(loop_dir: &Path, agent_id: &str, message: &str) -> Result<()> {
    let coordination_path = loop_dir.join("coordination.md");

    // Create if doesn't exist
    if !coordination_path.exists() {
        fs::write(&coordination_path, "# Coordination\n\n")?;
    }

    let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
    let entry = format!("[{}] [{}]: {}\n", timestamp, agent_id, message);

    // Append to file
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&coordination_path)?;
    file.write_all(entry.as_bytes())?;

    debug!("Broadcast from {}: {}", agent_id, message);
    Ok(())
}

/// Read recent broadcasts from coordination.md
///
/// Returns broadcasts from the last N minutes (default: 60)
pub fn read_broadcasts(loop_dir: &Path, max_age_minutes: Option<u32>) -> Result<Vec<String>> {
    let coordination_path = loop_dir.join("coordination.md");

    if !coordination_path.exists() {
        return Ok(vec![]);
    }

    let content = fs::read_to_string(&coordination_path)?;
    let max_age = max_age_minutes.unwrap_or(60);
    let cutoff = Utc::now() - chrono::Duration::minutes(max_age as i64);

    let mut broadcasts = Vec::new();

    for line in content.lines() {
        // Parse timestamp from line: [2025-01-15T12:34:56Z] [agent]: message
        if let Some(timestamp_str) = line.strip_prefix('[').and_then(|s| s.split(']').next()) {
            if let Ok(timestamp) = timestamp_str.parse::<DateTime<Utc>>() {
                if timestamp > cutoff {
                    broadcasts.push(line.to_string());
                }
            }
        }
    }

    Ok(broadcasts)
}

/// Join an existing Rehoboam loop (for multi-worker coordination)
///
/// Returns the rehoboam directory if it exists and has valid state
pub fn join_existing_loop(project_dir: &Path) -> Result<std::path::PathBuf> {
    let loop_dir = project_dir.join(".rehoboam");

    if !loop_dir.exists() {
        return Err(eyre!("No .rehoboam directory found in {:?}", project_dir));
    }

    // Verify state.json exists and is valid
    let _ = load_state(&loop_dir)?;

    info!("Joining existing Rehoboam loop at {:?}", loop_dir);
    Ok(loop_dir)
}

/// Register a worker with the coordination system
///
/// Adds a broadcast announcing the worker joined
pub fn register_worker(loop_dir: &Path, worker_id: &str, description: &str) -> Result<()> {
    let message = format!("Worker joined: {}", description);
    broadcast(loop_dir, worker_id, &message)?;

    // Create worker-specific state file
    let workers_dir = loop_dir.join("workers");
    fs::create_dir_all(&workers_dir)?;

    let worker_file = workers_dir.join(format!("{}.md", worker_id));
    let content = format!(
        "# Worker: {}\n\nJoined: {}\nDescription: {}\n\n## Status\nActive\n",
        worker_id,
        Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
        description
    );
    fs::write(&worker_file, content)?;

    info!("Registered worker {} in {:?}", worker_id, loop_dir);
    Ok(())
}

/// Update worker status
pub fn update_worker_status(loop_dir: &Path, worker_id: &str, status: &str) -> Result<()> {
    let worker_file = loop_dir.join("workers").join(format!("{}.md", worker_id));

    if !worker_file.exists() {
        return Err(eyre!("Worker {} not registered", worker_id));
    }

    let content = fs::read_to_string(&worker_file)?;

    // Replace status line (skip the old status value on the next line)
    let mut updated_lines = Vec::new();
    let mut skip_next = false;

    for line in content.lines() {
        if skip_next {
            skip_next = false;
            continue;
        }
        if line.starts_with("## Status") {
            updated_lines.push("## Status".to_string());
            updated_lines.push(status.to_string());
            skip_next = true;
        } else {
            updated_lines.push(line.to_string());
        }
    }
    let updated = updated_lines.join("\n");

    fs::write(&worker_file, updated)?;
    Ok(())
}

/// List active workers in the loop
pub fn list_workers(loop_dir: &Path) -> Result<Vec<String>> {
    let workers_dir = loop_dir.join("workers");

    if !workers_dir.exists() {
        return Ok(vec![]);
    }

    let mut workers = Vec::new();
    for entry in fs::read_dir(&workers_dir)? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|e| e == "md") {
            if let Some(stem) = entry.path().file_stem() {
                workers.push(stem.to_string_lossy().to_string());
            }
        }
    }

    Ok(workers)
}
