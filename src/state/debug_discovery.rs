//! Debug log scanner for ~/.claude/debug/
//!
//! Scans directory metadata only (no content loading) for the debug log index.
//! Content is loaded on-demand when the user opens a specific log.

use color_eyre::eyre;
use std::path::PathBuf;
use std::time::SystemTime;

/// A debug log file entry (metadata only)
#[derive(Debug, Clone)]
pub struct DebugLogEntry {
    /// Session ID (filename without .txt extension)
    pub session_id: String,
    /// Full path to the log file
    pub path: PathBuf,
    /// File size in bytes
    pub size_bytes: u64,
    /// Last modification time
    pub modified: SystemTime,
    /// true if this is the target of the `latest` symlink
    pub is_latest: bool,
}

pub struct DebugDiscovery;

impl DebugDiscovery {
    /// Scan ~/.claude/debug/ for log files, sorted by mtime desc
    pub fn scan_debug_logs() -> eyre::Result<Vec<DebugLogEntry>> {
        let debug_dir = Self::debug_dir()?;
        if !debug_dir.is_dir() {
            return Ok(Vec::new());
        }

        // Resolve the `latest` symlink target to mark is_latest
        let latest_target: Option<String> = debug_dir
            .join("latest")
            .read_link()
            .ok()
            .and_then(|link: PathBuf| {
                link.file_stem()
                    .and_then(|s| s.to_str())
                    .map(String::from)
            });

        let mut entries: Vec<DebugLogEntry> = Vec::new();

        for entry in std::fs::read_dir(&debug_dir)?.flatten() {
            let path = entry.path();

            // Skip non-.txt files and the `latest` symlink itself
            if path.extension().and_then(|e| e.to_str()) != Some("txt") {
                continue;
            }

            // Skip if it's a symlink (the `latest` file)
            if path.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
                continue;
            }

            let metadata = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };

            let session_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            let is_latest = latest_target
                .as_deref()
                .is_some_and(|lt| lt == session_id);

            entries.push(DebugLogEntry {
                session_id,
                path,
                size_bytes: metadata.len(),
                modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                is_latest,
            });
        }

        // Sort by modification time descending (newest first)
        entries.sort_by(|a, b| b.modified.cmp(&a.modified));

        Ok(entries)
    }

    fn debug_dir() -> eyre::Result<PathBuf> {
        let home =
            std::env::var("HOME").map_err(|_| eyre::eyre!("HOME not set"))?;
        Ok(PathBuf::from(home).join(".claude").join("debug"))
    }
}
