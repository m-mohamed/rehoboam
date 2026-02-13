//! Tmux integration for Claude Code agent orchestration
//!
//! Provides commands for:
//! - Sending input to agents (y/n approval, custom text)
//! - Checking pane health (alive/dead detection)
//! - Creating new panes for agent spawning
//!
//! Key patterns from ecosystem research:
//! - Enter must be a separate argument to send-keys
//! - Use load-buffer + paste-buffer for long/multi-line content

use std::io::Write;
use std::process::{Command, Stdio};

use color_eyre::eyre::{bail, Result, WrapErr};

/// Allowed environment variable names for Claude spawning (security allowlist)
///
/// Only these variables can be passed to spawned Claude sessions.
/// This prevents shell injection via malicious variable names.
const ALLOWED_ENV_VARS: &[&str] = &[
    "CLAUDE_CODE_TASK_LIST_ID",
    "CLAUDE_CODE_TEAM_NAME",
    "CLAUDE_CODE_AGENT_ID",
    "CLAUDE_CODE_AGENT_NAME",
    "CLAUDE_CODE_AGENT_TYPE",
    "REHOBOAM_ROLE",
    "REHOBOAM_WORKER_INDEX",
];

/// Validate environment variable name against allowlist
fn is_allowed_env_var(name: &str) -> bool {
    ALLOWED_ENV_VARS.contains(&name)
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
        // Use a unique named buffer to prevent race conditions with concurrent calls
        // Format: rehoboam-<pid>-<pane_id> to ensure uniqueness per process and pane
        let buffer_name = format!(
            "rehoboam-{}-{}",
            std::process::id(),
            pane_id.replace('%', "")
        );

        // Load content into named tmux buffer via stdin
        let mut child = Command::new("tmux")
            .args(["load-buffer", "-b", &buffer_name, "-"])
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

        // Paste named buffer to target pane, -d deletes buffer after paste
        let status = Command::new("tmux")
            .args(["paste-buffer", "-t", pane_id, "-b", &buffer_name, "-d"])
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

    /// Check if a tmux pane exists and is alive
    ///
    /// Uses `display-message` to query the pane's `pane_dead` flag.
    ///
    /// # Arguments
    /// * `pane_id` - Tmux pane identifier (e.g., "%1", "%2")
    ///
    /// # Returns
    /// - `Ok(true)` if pane exists and is alive
    /// - `Ok(false)` if pane is dead (pane_dead flag set)
    /// - `Err(_)` if pane doesn't exist or tmux error
    pub fn is_pane_alive(pane_id: &str) -> Result<bool> {
        let output = Command::new("tmux")
            .args(["display-message", "-t", pane_id, "-p", "#{pane_dead}"])
            .output()
            .wrap_err("Failed to execute tmux display-message")?;

        if !output.status.success() {
            bail!(
                "tmux display-message failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let result = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // pane_dead returns "0" if alive, "1" if dead
        Ok(result != "1")
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

    /// Split pane with environment variables set
    ///
    /// Creates a new pane with the specified environment variables.
    ///
    /// # Arguments
    /// * `horizontal` - true for horizontal split
    /// * `cwd` - Working directory for the new pane
    /// * `env_vars` - Environment variables to set (must be in ALLOWED_ENV_VARS)
    ///
    /// # Returns
    /// The pane ID of the newly created pane
    ///
    /// # Errors
    /// Returns an error if any env var name is not in the allowlist.
    pub fn split_pane_with_env(
        horizontal: bool,
        cwd: &str,
        env_vars: &[(&str, &str)],
    ) -> Result<String> {
        let flag = if horizontal { "-h" } else { "-v" };

        // If no env vars, use simple split
        if env_vars.is_empty() {
            return Self::split_pane(horizontal, cwd);
        }

        // Validate env var names against allowlist (security)
        for (name, _) in env_vars {
            if !is_allowed_env_var(name) {
                bail!(
                    "Environment variable '{}' not in allowlist. Allowed: {:?}",
                    name,
                    ALLOWED_ENV_VARS
                );
            }
        }

        // Build env prefix for the shell command
        let exports: Vec<String> = env_vars
            .iter()
            .map(|(k, v)| format!("export {}='{}'", k, v.replace('\'', "'\\''")))
            .collect();
        let env_prefix = exports.join("; ");

        // Use bash -c to set env vars before dropping to interactive shell
        let cmd = format!("bash -c '{}; exec bash'", env_prefix);

        let output = Command::new("tmux")
            .args([
                "split-window",
                flag,
                "-c",
                cwd,
                "-P",
                "-F",
                "#{pane_id}",
                &cmd,
            ])
            .output()
            .wrap_err("Failed to execute tmux split-window with env")?;

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
            env_count = env_vars.len(),
            "Created new tmux pane with environment variables"
        );
        Ok(pane_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allowed_env_vars() {
        assert!(is_allowed_env_var("CLAUDE_CODE_TEAM_NAME"));
        assert!(is_allowed_env_var("REHOBOAM_ROLE"));
        assert!(!is_allowed_env_var("PATH"));
        assert!(!is_allowed_env_var("HOME"));
    }
}
