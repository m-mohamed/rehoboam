//! hooks.log health monitoring
//!
//! Claude Code writes to `~/.claude/hooks.log` on every hook invocation.
//! With many hooks firing on every tool call, this file can grow to tens of
//! gigabytes and silently kill all hooks. When hooks die, Rehoboam goes blind.
//!
//! This module periodically checks the file size and:
//! - Warns in the TUI footer when it exceeds a configurable threshold
//! - Auto-truncates (keeping last N lines) when it exceeds a critical threshold

use std::path::PathBuf;
use std::time::Instant;

use crate::config::HealthConfig;
use crate::state::AppState;

/// Health checker for hooks.log file size monitoring
pub struct HealthChecker {
    /// Whether health checking is enabled
    enabled: bool,
    /// Seconds between checks
    interval_secs: u64,
    /// Warning threshold in bytes
    warn_bytes: u64,
    /// Auto-truncation threshold in bytes
    truncate_bytes: u64,
    /// Lines to keep when truncating
    truncate_keep_lines: usize,
    /// Path to hooks.log
    path: PathBuf,
    /// Last check time
    last_check: Instant,
    /// Whether we've already sent a desktop notification for the current warning
    notified: bool,
}

impl HealthChecker {
    /// Create a new health checker from config
    pub fn new(config: &HealthConfig) -> Self {
        Self {
            enabled: config.enabled,
            interval_secs: config.interval_secs,
            warn_bytes: config.warn_mb * 1024 * 1024,
            truncate_bytes: config.truncate_mb * 1024 * 1024,
            truncate_keep_lines: config.truncate_keep_lines,
            path: hooks_log_path(),
            last_check: Instant::now(),
            notified: false,
        }
    }

    /// Check if health check should run (timer-gated)
    pub fn should_run(&self) -> bool {
        self.enabled && self.last_check.elapsed().as_secs() >= self.interval_secs
    }

    /// Run the health check, returns true if warning state changed
    pub fn check(&mut self, state: &mut AppState) -> bool {
        self.last_check = Instant::now();

        if !self.enabled {
            return false;
        }

        let file_size = match std::fs::metadata(&self.path) {
            Ok(meta) => meta.len(),
            Err(_) => {
                // File doesn't exist or can't be read — no warning needed
                if state.health_warning.is_some() {
                    state.health_warning = None;
                    self.notified = false;
                    return true;
                }
                return false;
            }
        };

        // Critical: auto-truncate
        if file_size > self.truncate_bytes {
            let size_mb = file_size / (1024 * 1024);
            tracing::warn!(
                path = %self.path.display(),
                size_mb = size_mb,
                keep_lines = self.truncate_keep_lines,
                "hooks.log exceeded critical threshold, truncating"
            );

            if let Err(e) = truncate_file(&self.path, self.truncate_keep_lines) {
                tracing::error!(error = %e, "Failed to truncate hooks.log");
                state.health_warning =
                    Some(format!("hooks.log is {size_mb}MB — truncation failed: {e}"));
                return true;
            }

            // Clear warning after successful truncation
            let had_warning = state.health_warning.is_some();
            state.health_warning = None;
            self.notified = false;
            return had_warning;
        }

        // Warning threshold
        if file_size > self.warn_bytes {
            let size_mb = file_size / (1024 * 1024);
            let new_warning = format!(
                "hooks.log is {size_mb}MB — hooks may fail soon (auto-truncates at {}MB)",
                self.truncate_bytes / (1024 * 1024)
            );

            // Send desktop notification once per warning cycle
            if !self.notified {
                self.notified = true;
                crate::notify::send(
                    "Rehoboam: hooks.log growing",
                    &format!("hooks.log is {size_mb}MB — may kill hooks soon"),
                    Some("Basso"),
                );
            }

            let changed = state.health_warning.as_ref() != Some(&new_warning);
            state.health_warning = Some(new_warning);
            return changed;
        }

        // Below thresholds — clear warning if present
        if state.health_warning.is_some() {
            state.health_warning = None;
            self.notified = false;
            return true;
        }

        false
    }
}

/// Get the path to Claude Code's hooks.log
pub fn hooks_log_path() -> PathBuf {
    directories::BaseDirs::new().map_or_else(
        || {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".claude").join("hooks.log")
        },
        |dirs| dirs.home_dir().join(".claude").join("hooks.log"),
    )
}

/// Truncate a file keeping the last N lines
fn truncate_file(path: &PathBuf, keep_lines: usize) -> std::io::Result<()> {
    use std::io::{BufRead, BufReader, Write};

    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    let lines: Vec<String> = reader.lines().collect::<Result<_, _>>()?;

    let start = lines.len().saturating_sub(keep_lines);
    let kept = &lines[start..];

    let mut file = std::fs::File::create(path)?;
    for line in kept {
        writeln!(file, "{line}")?;
    }

    tracing::info!(
        original_lines = lines.len(),
        kept_lines = kept.len(),
        "Truncated hooks.log"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create a test checker pointing at a specific path with byte-level thresholds
    fn test_checker(path: PathBuf, warn_bytes: u64, truncate_bytes: u64) -> HealthChecker {
        HealthChecker {
            enabled: true,
            interval_secs: 0,
            warn_bytes,
            truncate_bytes,
            truncate_keep_lines: 1000,
            path,
            last_check: Instant::now(),
            notified: false,
        }
    }

    #[test]
    fn test_hooks_log_path() {
        let path = hooks_log_path();
        let path_str = path.to_string_lossy();
        assert!(
            path_str.ends_with(".claude/hooks.log"),
            "Expected path ending with .claude/hooks.log, got: {path_str}"
        );
    }

    #[test]
    fn test_should_run_respects_interval() {
        let config = HealthConfig {
            enabled: true,
            interval_secs: 60,
            warn_mb: 100,
            truncate_mb: 500,
            truncate_keep_lines: 1000,
        };
        let checker = HealthChecker::new(&config);
        // Should not run immediately (interval not elapsed)
        assert!(!checker.should_run());
    }

    #[test]
    fn test_should_run_disabled() {
        let config = HealthConfig {
            enabled: false,
            interval_secs: 0,
            warn_mb: 100,
            truncate_mb: 500,
            truncate_keep_lines: 1000,
        };
        let checker = HealthChecker::new(&config);
        assert!(!checker.should_run());
    }

    #[test]
    fn test_check_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.log");
        let mut checker = test_checker(path, 100, 1000);
        let mut state = AppState::new();

        let changed = checker.check(&mut state);
        assert!(!changed);
        assert!(state.health_warning.is_none());
    }

    #[test]
    fn test_check_small_file() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks.log");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "small file").unwrap();
        drop(f);

        let file_size = std::fs::metadata(&path).unwrap().len();
        // Set thresholds well above the file size
        let mut checker = test_checker(path, file_size * 10, file_size * 100);
        let mut state = AppState::new();

        let changed = checker.check(&mut state);
        assert!(!changed);
        assert!(state.health_warning.is_none());
    }

    #[test]
    fn test_check_large_file_warns() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks.log");

        let mut f = std::fs::File::create(&path).unwrap();
        for _ in 0..100 {
            writeln!(f, "hook event line").unwrap();
        }
        drop(f);

        let file_size = std::fs::metadata(&path).unwrap().len();
        // Set warn below file size, truncate well above
        let mut checker = test_checker(path, file_size / 2, file_size * 10);
        let mut state = AppState::new();

        let changed = checker.check(&mut state);
        assert!(changed);
        assert!(state.health_warning.is_some());
    }

    #[test]
    fn test_check_clears_warning() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.log");
        let mut checker = test_checker(path, 100, 1000);
        let mut state = AppState::new();

        // Manually set a warning
        state.health_warning = Some("old warning".to_string());

        // Check with no file should clear warning
        let changed = checker.check(&mut state);
        assert!(changed);
        assert!(state.health_warning.is_none());
    }

    #[test]
    fn test_truncate_file() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hooks.log");

        let mut f = std::fs::File::create(&path).unwrap();
        for i in 0..100 {
            writeln!(f, "line {i}").unwrap();
        }
        drop(f);

        truncate_file(&path, 10).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 10);
        assert_eq!(lines[0], "line 90");
        assert_eq!(lines[9], "line 99");
    }
}
