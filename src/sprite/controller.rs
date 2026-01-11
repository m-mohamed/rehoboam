//! Sprite input controller
//!
//! Provides methods to send input to Claude Code running inside sprites.
//! This mirrors TmuxController but operates on remote sprites.
//!
//! Scaffolded for sprite command integration (not yet wired into main).

#![allow(dead_code)]

use color_eyre::eyre::{eyre, Result};
use sprites::Sprite;
use tracing::{debug, info};

/// Controller for sending input to sprites
pub struct SpriteController;

impl SpriteController {
    /// Send raw input to a sprite's running process
    pub async fn send_input(sprite: &Sprite, input: &str) -> Result<()> {
        debug!("Sending input to sprite: {:?}", input);

        // We need to write to the running Claude Code process's stdin
        // This requires an active spawn() session - for now we use a command approach
        let output = sprite
            .command("bash")
            .arg("-c")
            .arg(format!(
                "echo -n '{}' > /tmp/claude_input && cat /tmp/claude_input",
                input.replace('\'', "'\\''")
            ))
            .output()
            .await
            .map_err(|e| eyre!("Failed to send input: {}", e))?;

        if !output.success() {
            return Err(eyre!("Input send failed: {}", output.stderr_str()));
        }

        Ok(())
    }

    /// Send approval keystroke (typically 'y' + Enter)
    pub async fn approve(sprite: &Sprite) -> Result<()> {
        info!("Approving permission request in sprite");
        Self::send_input(sprite, "y\n").await
    }

    /// Send rejection keystroke (typically 'n' + Enter)
    pub async fn reject(sprite: &Sprite) -> Result<()> {
        info!("Rejecting permission request in sprite");
        Self::send_input(sprite, "n\n").await
    }

    /// Send Ctrl+C to kill running process
    pub async fn kill(sprite: &Sprite) -> Result<()> {
        info!("Sending Ctrl+C to sprite");

        // Find and kill the Claude process
        let output = sprite
            .command("pkill")
            .arg("-INT")
            .arg("-f")
            .arg("claude")
            .output()
            .await
            .map_err(|e| eyre!("Failed to send kill signal: {}", e))?;

        // pkill returns non-zero if no processes matched, which is fine
        debug!("Kill signal sent, exit code: {}", output.status);
        Ok(())
    }

    /// Send Enter key
    pub async fn send_enter(sprite: &Sprite) -> Result<()> {
        Self::send_input(sprite, "\n").await
    }

    /// Check if Claude is still running in the sprite
    pub async fn is_claude_running(sprite: &Sprite) -> Result<bool> {
        let output = sprite
            .command("pgrep")
            .arg("-f")
            .arg("claude")
            .output()
            .await
            .map_err(|e| eyre!("Failed to check claude process: {}", e))?;

        Ok(output.success())
    }
}
