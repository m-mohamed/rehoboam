//! Sprite input controller
//!
//! Provides methods to send input to Claude Code running inside sprites.
//! This mirrors TmuxController but operates on remote sprites.

use color_eyre::eyre::{eyre, Result};
use sprites::Sprite;
use tracing::{debug, info};

/// Controller for sending input to sprites
pub struct SpriteController;

impl SpriteController {
    /// Send raw input to a sprite's running process
    async fn send_input(sprite: &Sprite, input: &str) -> Result<()> {
        debug!("Sending input to sprite: {:?}", input);

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

        let output = sprite
            .command("pkill")
            .arg("-INT")
            .arg("-f")
            .arg("claude")
            .output()
            .await
            .map_err(|e| eyre!("Failed to send kill signal: {}", e))?;

        debug!("Kill signal sent, exit code: {}", output.status);
        Ok(())
    }
}
