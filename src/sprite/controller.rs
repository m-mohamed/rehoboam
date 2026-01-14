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
    /// Send raw input to a sprite's running Claude process via tmux
    ///
    /// Claude runs inside a tmux session named `claude-{sprite_name}`.
    /// This uses `tmux send-keys` to inject input to the running process.
    pub async fn send_input(sprite: &Sprite, input: &str) -> Result<()> {
        let tmux_session = format!("claude-{}", sprite.name());
        debug!(
            "Sending input to sprite {} (tmux session: {}): {:?}",
            sprite.name(),
            tmux_session,
            input
        );

        // Use tmux send-keys to inject input to the Claude session
        // -l flag sends literal keys (no special interpretation)
        let output = sprite
            .command("tmux")
            .arg("send-keys")
            .arg("-t")
            .arg(&tmux_session)
            .arg("-l")
            .arg(input)
            .output()
            .await
            .map_err(|e| eyre!("Failed to send input via tmux: {}", e))?;

        if !output.success() {
            return Err(eyre!("tmux send-keys failed: {}", output.stderr_str()));
        }

        info!(
            "Input sent to sprite {} via tmux session {}",
            sprite.name(),
            tmux_session
        );
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
