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

/// Allowed environment variable names for Claude spawning (security allowlist)
///
/// Only these variables can be passed to spawned Claude sessions.
/// This prevents shell injection via malicious variable names.
const ALLOWED_ENV_VARS: &[&str] = &[
    "CLAUDE_CODE_TASK_LIST_ID",
    "REHOBOAM_ROLE",
    "REHOBOAM_WORKER_INDEX",
];

/// Validate environment variable name against allowlist
fn is_allowed_env_var(name: &str) -> bool {
    ALLOWED_ENV_VARS.contains(&name)
}

/// Type of prompt detected in pane output via reconciliation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptType {
    /// Permission request: tool needs approval ([y/n], approve, etc.)
    Permission,
    /// Input request: Claude asking a question
    Input,
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

    /// Capture the last N lines from a pane
    ///
    /// More efficient than full capture for pattern matching during reconciliation.
    /// Uses `capture-pane` with `-S` (start line) option.
    ///
    /// # Arguments
    /// * `pane_id` - Tmux pane identifier
    /// * `lines` - Number of lines to capture from bottom
    ///
    /// # Returns
    /// The captured lines as a string
    pub fn capture_pane_tail(pane_id: &str, lines: usize) -> Result<String> {
        let start_line = format!("-{}", lines);
        let output = Command::new("tmux")
            .args(["capture-pane", "-t", pane_id, "-p", "-S", &start_line])
            .output()
            .wrap_err("Failed to execute tmux capture-pane")?;

        if !output.status.success() {
            bail!(
                "tmux capture-pane failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        // Try proper UTF-8 conversion, fall back to lossy with warning
        match String::from_utf8(output.stdout.clone()) {
            Ok(s) => Ok(s),
            Err(_) => {
                tracing::warn!(pane_id = %pane_id, "Non-UTF-8 output from pane tail, using lossy conversion");
                Ok(String::from_utf8_lossy(&output.stdout).into_owned())
            }
        }
    }

    /// Pattern match captured pane output for permission/input prompts
    ///
    /// Pure function separated for testability without tmux dependency.
    /// Used by reconciliation to detect prompts when hooks fail.
    ///
    /// # Arguments
    /// * `output` - Captured pane content (typically last 30 lines)
    ///
    /// # Returns
    /// - `Some(PromptType::Permission)` if permission prompt detected
    /// - `Some(PromptType::Input)` if input prompt detected
    /// - `None` if no prompt or still working (spinner visible)
    pub fn match_prompt_patterns(output: &str) -> Option<PromptType> {
        // Check for spinner/working indicators first (negative patterns)
        // If spinner is visible, Claude is still working - don't match
        const SPINNER_CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        const WORKING_INDICATORS: &[&str] = &["Thinking", "Running", "Compacting"];

        // Only check last few lines for freshness
        let recent_lines: Vec<&str> = output.lines().rev().take(10).collect();
        let recent_text = recent_lines.join("\n");

        // If any spinner char in recent output, still working
        if SPINNER_CHARS.iter().any(|c| recent_text.contains(*c)) {
            return None;
        }

        // If working indicator present, still working
        if WORKING_INDICATORS.iter().any(|s| recent_text.contains(s)) {
            return None;
        }

        // Permission patterns (Claude Code approval prompts)
        const PERMISSION_PATTERNS: &[&str] = &[
            "[y/n]",
            "(y/n)",
            "[Y/N]",
            "(Y/N)",
            "(yes/no)",
            "[yes/no]",
            "Allow this",
            "allow this",
            "Allow once",
            "Allow always",
            "approve",
            "Approve",
            "Do you want to",
            "Deny",
            "Press y to",
            "Press n to",
        ];

        // Check for permission prompt
        if PERMISSION_PATTERNS.iter().any(|p| recent_text.contains(p)) {
            return Some(PromptType::Permission);
        }

        // Input patterns (Claude asking for user input)
        // More conservative - only match if we see clear input indicators
        // at the end of lines
        for line in recent_lines.iter().take(3) {
            let trimmed = line.trim();
            if trimmed.ends_with('?') && trimmed.len() > 10 {
                return Some(PromptType::Input);
            }
        }

        // Note: We don't detect shell prompts ("$", ">") because Claude Code
        // has its own UI and doesn't show shell prompts when waiting.
        // Shell prompts would only appear if Claude crashed and returned to shell,
        // which would be caught by pane health checks instead.

        None
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
    fn test_match_permission_patterns() {
        // Table-driven test for permission prompt detection
        let cases: Vec<(&str, &str)> = vec![
            ("Do you want to allow this action? [y/n]", "[y/n] pattern"),
            ("Allow this tool to execute?\n> ", "Allow this pattern"),
            ("Please approve the following action:", "approve pattern"),
        ];

        for (output, desc) in cases {
            assert_eq!(
                TmuxController::match_prompt_patterns(output),
                Some(PromptType::Permission),
                "should match permission: {}",
                desc
            );
        }
    }

    #[test]
    fn test_match_working_blocks_and_input() {
        // Working indicators should block permission matching
        let working_cases: Vec<(&str, &str)> = vec![
            ("⠋ Thinking about your request...\n[y/n]", "spinner blocks"),
            (
                "Running tool: Bash\nAllow this?",
                "running indicator blocks",
            ),
        ];

        for (output, desc) in working_cases {
            assert_eq!(
                TmuxController::match_prompt_patterns(output),
                None,
                "should not match: {}",
                desc
            );
        }

        // Input prompt should be detected
        assert_eq!(
            TmuxController::match_prompt_patterns(
                "Some context here\nWhat would you like me to do next?"
            ),
            Some(PromptType::Input),
            "should match input prompt"
        );
    }

    #[test]
    fn test_match_no_prompt() {
        // Table-driven test for non-matching patterns
        let cases: Vec<(&str, &str)> = vec![
            (
                "Reading file contents...\nProcessing data...",
                "normal output",
            ),
            ("", "empty output"),
        ];

        for (output, desc) in cases {
            assert_eq!(
                TmuxController::match_prompt_patterns(output),
                None,
                "should not match: {}",
                desc
            );
        }
    }
}
