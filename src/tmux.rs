//! Tmux integration for Claude Code agent orchestration
//!
//! Provides commands for:
//! - Sending input to agents (y/n approval, custom text)
//! - Capturing pane output for monitoring
//! - Creating new panes for agent spawning
//! - Output pattern matching for status detection
//!
//! Key patterns from ecosystem research:
//! - Enter must be a separate argument to send-keys
//! - Use load-buffer + paste-buffer for long/multi-line content
//! - capture-pane for reading agent output

use std::io::Write;
use std::process::{Command, Stdio};

use color_eyre::eyre::{bail, Result, WrapErr};

/// Status detected from pane output via pattern matching
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum DetectedStatus {
    /// Agent needs user attention (permission prompt, question)
    Attention,
    /// Task completed successfully
    Success,
    /// Task failed
    Failure,
    /// Could not determine status
    Unknown,
}

/// Controller for tmux operations
///
/// Provides methods for interacting with tmux panes where Claude Code agents run.
/// All methods use direct CLI commands for simplicity (no libvterm/ffi).
pub struct TmuxController;

impl TmuxController {
    /// Send keys to a tmux pane
    ///
    /// CRITICAL: Enter must be passed as a separate argument, not "\n"
    ///
    /// # Arguments
    /// * `pane_id` - Tmux pane identifier (e.g., "%1", "%2")
    /// * `keys` - Text to send (without Enter)
    ///
    /// # Example
    /// ```ignore
    /// TmuxController::send_keys("%1", "y")?;  // Sends "y" + Enter
    /// ```
    pub fn send_keys(pane_id: &str, keys: &str) -> Result<()> {
        let status = Command::new("tmux")
            .args(["send-keys", "-t", pane_id, keys, "Enter"])
            .status()
            .wrap_err("Failed to execute tmux send-keys")?;

        if !status.success() {
            bail!("tmux send-keys failed with status: {}", status);
        }

        tracing::debug!(pane_id = %pane_id, keys = %keys, "Sent keys to pane");
        Ok(())
    }

    /// Send Enter key to a tmux pane (for loop continuation)
    ///
    /// Sends just Enter without any text. Used by loop mode to continue
    /// Claude Code sessions after Stop events.
    ///
    /// # Arguments
    /// * `pane_id` - Tmux pane identifier (e.g., "%1", "%2")
    pub fn send_enter(pane_id: &str) -> Result<()> {
        let status = Command::new("tmux")
            .args(["send-keys", "-t", pane_id, "Enter"])
            .status()
            .wrap_err("Failed to execute tmux send-keys Enter")?;

        if !status.success() {
            bail!("tmux send-keys Enter failed with status: {}", status);
        }

        tracing::debug!(pane_id = %pane_id, "Sent Enter to pane");
        Ok(())
    }

    /// Send keys without Enter (for partial input)
    ///
    /// # Arguments
    /// * `pane_id` - Tmux pane identifier
    /// * `keys` - Text to send (will not press Enter)
    pub fn send_keys_raw(pane_id: &str, keys: &str) -> Result<()> {
        let status = Command::new("tmux")
            .args(["send-keys", "-t", pane_id, keys])
            .status()
            .wrap_err("Failed to execute tmux send-keys")?;

        if !status.success() {
            bail!("tmux send-keys failed with status: {}", status);
        }

        Ok(())
    }

    /// Send multi-line content via tmux buffer
    ///
    /// Uses load-buffer + paste-buffer to avoid escaping issues with long prompts.
    /// This is the recommended method for sending prompts > 1 line.
    ///
    /// # Arguments
    /// * `pane_id` - Tmux pane identifier
    /// * `content` - Multi-line text to send
    pub fn send_buffered(pane_id: &str, content: &str) -> Result<()> {
        // Load content into tmux buffer via stdin
        let mut child = Command::new("tmux")
            .args(["load-buffer", "-"])
            .stdin(Stdio::piped())
            .spawn()
            .wrap_err("Failed to spawn tmux load-buffer")?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(content.as_bytes())
                .wrap_err("Failed to write to tmux buffer")?;
        }

        let status = child
            .wait()
            .wrap_err("Failed to wait for tmux load-buffer")?;
        if !status.success() {
            bail!("tmux load-buffer failed");
        }

        // Paste buffer to target pane
        let status = Command::new("tmux")
            .args(["paste-buffer", "-t", pane_id])
            .status()
            .wrap_err("Failed to execute tmux paste-buffer")?;

        if !status.success() {
            bail!("tmux paste-buffer failed");
        }

        // Send Enter to execute
        let status = Command::new("tmux")
            .args(["send-keys", "-t", pane_id, "Enter"])
            .status()
            .wrap_err("Failed to send Enter")?;

        if !status.success() {
            bail!("tmux send-keys Enter failed");
        }

        tracing::debug!(
            pane_id = %pane_id,
            content_len = content.len(),
            "Sent buffered content to pane"
        );
        Ok(())
    }

    /// Capture pane output for monitoring
    ///
    /// Returns the visible content of the pane as a string.
    /// Use this to detect agent status via pattern matching.
    ///
    /// # Arguments
    /// * `pane_id` - Tmux pane identifier
    ///
    /// # Returns
    /// The captured pane content
    #[allow(dead_code)]
    pub fn capture_pane(pane_id: &str) -> Result<String> {
        let output = Command::new("tmux")
            .args(["capture-pane", "-t", pane_id, "-p"])
            .output()
            .wrap_err("Failed to execute tmux capture-pane")?;

        if !output.status.success() {
            bail!(
                "tmux capture-pane failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Create a new tmux pane via split
    ///
    /// # Arguments
    /// * `horizontal` - true for horizontal split (-h), false for vertical (-v)
    /// * `cwd` - Working directory for the new pane
    ///
    /// # Returns
    /// The pane ID of the newly created pane (e.g., "%3")
    pub fn split_pane(horizontal: bool, cwd: &str) -> Result<String> {
        let flag = if horizontal { "-h" } else { "-v" };

        let output = Command::new("tmux")
            .args(["split-window", flag, "-c", cwd, "-P", "-F", "#{pane_id}"])
            .output()
            .wrap_err("Failed to execute tmux split-window")?;

        if !output.status.success() {
            bail!(
                "tmux split-window failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let pane_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        tracing::info!(pane_id = %pane_id, cwd = %cwd, "Created new tmux pane");
        Ok(pane_id)
    }

    /// Kill a tmux pane
    ///
    /// Used by Ralph loops to kill the old session before respawning fresh.
    ///
    /// # Arguments
    /// * `pane_id` - Tmux pane identifier (e.g., "%1", "%2")
    pub fn kill_pane(pane_id: &str) -> Result<()> {
        let status = Command::new("tmux")
            .args(["kill-pane", "-t", pane_id])
            .status()
            .wrap_err("Failed to execute tmux kill-pane")?;

        if !status.success() {
            bail!("tmux kill-pane failed with status: {}", status);
        }

        tracing::info!(pane_id = %pane_id, "Killed tmux pane");
        Ok(())
    }

    /// Respawn a pane with a fresh Claude Code session
    ///
    /// Creates a new pane in the same window and starts Claude with the given prompt file.
    /// Used by Ralph loops to spawn fresh sessions per iteration.
    ///
    /// # Arguments
    /// * `cwd` - Working directory for the new pane
    /// * `prompt_file` - Path to the iteration prompt file
    ///
    /// # Returns
    /// The pane ID of the newly created pane
    pub fn respawn_claude(cwd: &str, prompt_file: &str) -> Result<String> {
        // Create a new pane with claude command using prompt file
        let cmd = format!("claude --prompt-file '{}'", prompt_file);

        let output = Command::new("tmux")
            .args([
                "split-window",
                "-h", // horizontal split
                "-c", cwd,
                "-P",
                "-F", "#{pane_id}",
                &cmd,
            ])
            .output()
            .wrap_err("Failed to execute tmux split-window for respawn")?;

        if !output.status.success() {
            bail!(
                "tmux split-window failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let pane_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        tracing::info!(
            pane_id = %pane_id,
            cwd = %cwd,
            prompt_file = %prompt_file,
            "Respawned Claude in new pane"
        );
        Ok(pane_id)
    }

    /// Send Ctrl+C to interrupt current process
    ///
    /// Used to cleanly stop Claude before killing the pane.
    ///
    /// # Arguments
    /// * `pane_id` - Tmux pane identifier
    pub fn send_interrupt(pane_id: &str) -> Result<()> {
        let status = Command::new("tmux")
            .args(["send-keys", "-t", pane_id, "C-c"])
            .status()
            .wrap_err("Failed to execute tmux send-keys C-c")?;

        if !status.success() {
            bail!("tmux send-keys C-c failed with status: {}", status);
        }

        tracing::debug!(pane_id = %pane_id, "Sent Ctrl+C to pane");
        Ok(())
    }

    /// Focus a specific tmux pane
    ///
    /// # Arguments
    /// * `pane_id` - Tmux pane identifier
    #[allow(dead_code)]
    pub fn select_pane(pane_id: &str) -> Result<()> {
        let status = Command::new("tmux")
            .args(["select-pane", "-t", pane_id])
            .status()
            .wrap_err("Failed to execute tmux select-pane")?;

        if !status.success() {
            bail!("tmux select-pane failed");
        }

        Ok(())
    }

    /// Detect status from captured pane output via pattern matching
    ///
    /// Looks for common signals in the last 10 lines:
    /// - Attention: "[y/n]", "permission", "approve", "?"
    /// - Success: checkmark, "PASSED", "tests passing", "Complete"
    /// - Failure: X mark, "FAILED", "Error:", "error["
    ///
    /// # Arguments
    /// * `output` - Captured pane output
    ///
    /// # Returns
    /// The detected status
    #[cfg(test)]
    pub fn detect_status(output: &str) -> DetectedStatus {
        // Get last 10 lines for analysis (order doesn't matter for contains() checks)
        let lines: Vec<&str> = output.lines().collect();
        let start = lines.len().saturating_sub(10);
        let last_lines = lines[start..].join("\n");

        let lower = last_lines.to_lowercase();

        // Check for attention signals (highest priority)
        if lower.contains("[y/n]")
            || lower.contains("permission")
            || lower.contains("approve")
            || lower.contains("allow this")
            || (lower.contains("?") && !lower.contains("http"))
        {
            return DetectedStatus::Attention;
        }

        // Check for success signals
        if last_lines.contains('✅')
            || lower.contains("passed")
            || lower.contains("tests passing")
            || lower.contains("complete")
            || lower.contains("success")
        {
            return DetectedStatus::Success;
        }

        // Check for failure signals
        if last_lines.contains('❌')
            || lower.contains("failed")
            || lower.contains("error:")
            || lower.contains("error[")
            || lower.contains("panic")
        {
            return DetectedStatus::Failure;
        }

        DetectedStatus::Unknown
    }

    /// Get current tmux session info
    ///
    /// Returns (session_name, window_index, pane_index)
    #[allow(dead_code)]
    pub fn get_current_context() -> Result<(String, String, String)> {
        let session = Command::new("tmux")
            .args(["display-message", "-p", "#S"])
            .output()
            .wrap_err("Failed to get session name")?;

        let window = Command::new("tmux")
            .args(["display-message", "-p", "#I"])
            .output()
            .wrap_err("Failed to get window index")?;

        let pane = Command::new("tmux")
            .args(["display-message", "-p", "#P"])
            .output()
            .wrap_err("Failed to get pane index")?;

        Ok((
            String::from_utf8_lossy(&session.stdout).trim().to_string(),
            String::from_utf8_lossy(&window.stdout).trim().to_string(),
            String::from_utf8_lossy(&pane.stdout).trim().to_string(),
        ))
    }

    /// List all panes in current session
    ///
    /// Returns a list of (pane_id, pane_title, current_command)
    #[allow(dead_code)]
    pub fn list_panes() -> Result<Vec<(String, String, String)>> {
        let output = Command::new("tmux")
            .args([
                "list-panes",
                "-F",
                "#{pane_id}\t#{pane_title}\t#{pane_current_command}",
            ])
            .output()
            .wrap_err("Failed to list panes")?;

        if !output.status.success() {
            bail!("tmux list-panes failed");
        }

        let panes: Vec<(String, String, String)> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.split('\t').collect();
                if parts.len() >= 3 {
                    Some((
                        parts[0].to_string(),
                        parts[1].to_string(),
                        parts[2].to_string(),
                    ))
                } else {
                    None
                }
            })
            .collect();

        Ok(panes)
    }

    /// Check if tmux is available and we're inside a tmux session
    #[allow(dead_code)]
    pub fn is_available() -> bool {
        // Check if tmux command exists
        if Command::new("which")
            .arg("tmux")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            // Check if we're inside a tmux session
            std::env::var("TMUX").is_ok()
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_status_attention() {
        let output = "Some output\nDo you want to proceed? [y/n]";
        assert_eq!(
            TmuxController::detect_status(output),
            DetectedStatus::Attention
        );

        let output = "Requesting permission to run bash";
        assert_eq!(
            TmuxController::detect_status(output),
            DetectedStatus::Attention
        );
    }

    #[test]
    fn test_detect_status_success() {
        let output = "Running tests...\n✅ All tests passed!";
        assert_eq!(
            TmuxController::detect_status(output),
            DetectedStatus::Success
        );

        let output = "Build complete. Success!";
        assert_eq!(
            TmuxController::detect_status(output),
            DetectedStatus::Success
        );
    }

    #[test]
    fn test_detect_status_failure() {
        let output = "Running tests...\n❌ 3 tests failed";
        assert_eq!(
            TmuxController::detect_status(output),
            DetectedStatus::Failure
        );

        let output = "error[E0433]: failed to resolve";
        assert_eq!(
            TmuxController::detect_status(output),
            DetectedStatus::Failure
        );
    }

    #[test]
    fn test_detect_status_unknown() {
        let output = "Processing...\nWorking on task";
        assert_eq!(
            TmuxController::detect_status(output),
            DetectedStatus::Unknown
        );
    }
}
