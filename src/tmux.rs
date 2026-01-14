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
        // Create a new pane with claude command, piping prompt file to stdin
        let cmd = format!("cat '{}' | claude", prompt_file);

        let output = Command::new("tmux")
            .args([
                "split-window",
                "-h", // horizontal split
                "-c",
                cwd,
                "-P",
                "-F",
                "#{pane_id}",
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
}
