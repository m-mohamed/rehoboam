//! Sprite lifecycle management
//!
//! Scaffolded for sprite command integration (not yet wired into main).

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::sprite::config::{NetworkPreset, SpriteConfig};
use color_eyre::eyre::{eyre, Result};
use sprites::{Sprite, SpriteConfig as SpriteSdkConfig, SpritesClient};
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Manages sprite lifecycle for remote Claude Code agents
pub struct SpriteManager {
    /// Sprites API client
    client: SpritesClient,

    /// Configuration
    config: SpriteConfig,

    /// Active sprite sessions (agent_id -> session)
    sessions: HashMap<String, SpriteSession>,

    /// Counter for generating unique sprite names
    sprite_counter: u64,
}

/// A checkpoint in the timeline
#[derive(Debug, Clone)]
pub struct CheckpointRecord {
    /// Checkpoint ID from Sprites API
    pub id: String,

    /// User-provided comment/description
    pub comment: String,

    /// When the checkpoint was created
    pub created_at: Instant,

    /// Loop iteration at time of checkpoint (0 if not in loop mode)
    pub iteration: u32,
}

/// Active sprite session metadata
#[derive(Debug)]
pub struct SpriteSession {
    /// Sprite handle
    pub sprite: Sprite,

    /// Agent ID (maps to Rehoboam's pane_id)
    pub agent_id: String,

    /// Project name
    pub project: String,

    /// When the sprite was created
    pub created_at: Instant,

    /// Last checkpoint ID
    pub last_checkpoint: Option<String>,

    /// Checkpoint history for timeline UI
    pub checkpoint_history: Vec<CheckpointRecord>,

    /// WebSocket connection for receiving events (set by forwarder)
    pub ws_connected: bool,
}

impl SpriteManager {
    /// Create a new sprite manager
    pub fn new(config: SpriteConfig) -> Self {
        let client = SpritesClient::new(&config.token);

        Self {
            client,
            config,
            sessions: HashMap::new(),
            sprite_counter: 0,
        }
    }

    /// Create a new sprite for running a Claude Code agent
    pub async fn create_agent_sprite(
        &mut self,
        project: &str,
        network_preset: Option<NetworkPreset>,
    ) -> Result<String> {
        // Generate unique sprite name
        self.sprite_counter += 1;
        let sprite_name = format!(
            "rehoboam-{}-{}",
            project.replace(['/', '.'], "-"),
            self.sprite_counter
        );

        info!("Creating sprite: {}", sprite_name);

        // Build sprite configuration
        let sdk_config = SpriteSdkConfig {
            ram_mb: Some(self.config.ram_mb),
            cpus: Some(self.config.cpus),
            region: Some(self.config.region.clone()),
            storage_gb: None,
        };

        // Create the sprite
        let sprite = self
            .client
            .create_with_config(&sprite_name, Some(sdk_config), None)
            .await
            .map_err(|e| eyre!("Failed to create sprite: {}", e))?;

        // Apply network policy
        let preset = network_preset.unwrap_or(self.config.network_preset);
        let policy = preset.into_policy();
        sprite
            .set_policy(policy)
            .await
            .map_err(|e| eyre!("Failed to set network policy: {}", e))?;

        // Generate agent ID
        let agent_id = format!("sprite-{sprite_name}");

        // Store session
        let session = SpriteSession {
            sprite,
            agent_id: agent_id.clone(),
            project: project.to_string(),
            created_at: Instant::now(),
            last_checkpoint: None,
            checkpoint_history: Vec::new(),
            ws_connected: false,
        };

        self.sessions.insert(agent_id.clone(), session);

        info!("Created sprite {} with agent_id {}", sprite_name, agent_id);
        Ok(agent_id)
    }

    /// Initialize a sprite with Claude Code and hook bridge
    pub async fn initialize_sprite(&self, agent_id: &str, _prompt: &str) -> Result<()> {
        let session = self
            .sessions
            .get(agent_id)
            .ok_or_else(|| eyre!("No sprite session for agent_id: {}", agent_id))?;

        let sprite = &session.sprite;

        // Install Claude Code CLI (assuming it's available)
        info!("Installing Claude Code in sprite...");

        // Create the hook bridge configuration
        let rehoboam_host =
            std::env::var("REHOBOAM_PUBLIC_HOST").unwrap_or_else(|_| "localhost".to_string());
        let ws_url = format!("ws://{}:{}", rehoboam_host, self.config.ws_port);

        // Create .claude directory and configure hooks
        let setup_script = format!(
            r#"
#!/bin/bash
set -e

# Create .claude directory
mkdir -p ~/.claude

# Write hook configuration
cat > ~/.claude/settings.json << 'SETTINGS'
{{
  "hooks": {{
    "PreToolUse": ["rehoboam-bridge"],
    "PostToolUse": ["rehoboam-bridge"],
    "PermissionRequest": ["rehoboam-bridge"],
    "Stop": ["rehoboam-bridge"],
    "SubagentSpawn": ["rehoboam-bridge"],
    "SubagentStop": ["rehoboam-bridge"]
  }}
}}
SETTINGS

# Set environment variables for bridge
export REHOBOAM_WS_URL="{ws_url}"
export SPRITE_ID="{agent_id}"

echo "Hook configuration complete"
"#
        );

        let output = sprite
            .command("bash")
            .arg("-c")
            .arg(&setup_script)
            .output()
            .await
            .map_err(|e| eyre!("Failed to setup hooks: {}", e))?;

        if !output.success() {
            return Err(eyre!("Hook setup failed: {}", output.stderr_str()));
        }

        debug!("Hook setup complete for {}", agent_id);
        Ok(())
    }

    /// Start Claude Code with a prompt
    pub async fn start_claude(&self, agent_id: &str, prompt: &str) -> Result<()> {
        let session = self
            .sessions
            .get(agent_id)
            .ok_or_else(|| eyre!("No sprite session for agent_id: {}", agent_id))?;

        let sprite = &session.sprite;

        info!("Starting Claude Code in sprite {} with prompt", agent_id);

        // Start Claude Code in the background
        // This uses spawn() to get a Child handle, but we don't wait for it
        let _child = sprite
            .command("claude")
            .arg("--dangerously-skip-permissions")
            .arg(prompt)
            .current_dir("/workspace")
            .spawn()
            .await
            .map_err(|e| eyre!("Failed to start Claude: {}", e))?;

        Ok(())
    }

    /// Get sprite handle for an agent
    pub fn get_sprite(&self, agent_id: &str) -> Option<&Sprite> {
        self.sessions.get(agent_id).map(|s| &s.sprite)
    }

    /// Get session for an agent
    pub fn get_session(&self, agent_id: &str) -> Option<&SpriteSession> {
        self.sessions.get(agent_id)
    }

    /// Get mutable session for an agent
    pub fn get_session_mut(&mut self, agent_id: &str) -> Option<&mut SpriteSession> {
        self.sessions.get_mut(agent_id)
    }

    /// Create a checkpoint for a sprite
    pub async fn checkpoint(&mut self, agent_id: &str, comment: &str) -> Result<String> {
        self.checkpoint_with_iteration(agent_id, comment, 0).await
    }

    /// Create a checkpoint with iteration tracking (for loop mode)
    pub async fn checkpoint_with_iteration(
        &mut self,
        agent_id: &str,
        comment: &str,
        iteration: u32,
    ) -> Result<String> {
        let session = self
            .sessions
            .get_mut(agent_id)
            .ok_or_else(|| eyre!("No sprite session for agent_id: {}", agent_id))?;

        info!("Creating checkpoint for {}: {}", agent_id, comment);

        let checkpoint = session
            .sprite
            .checkpoint(comment)
            .await
            .map_err(|e| eyre!("Checkpoint failed: {}", e))?;

        let checkpoint_id = checkpoint.id.clone();
        session.last_checkpoint = Some(checkpoint_id.clone());

        // Track in history for timeline UI
        session.checkpoint_history.push(CheckpointRecord {
            id: checkpoint_id.clone(),
            comment: comment.to_string(),
            created_at: Instant::now(),
            iteration,
        });

        info!("Checkpoint created: {}", checkpoint_id);
        Ok(checkpoint_id)
    }

    /// Get checkpoint history for an agent
    pub fn get_checkpoint_history(&self, agent_id: &str) -> Vec<CheckpointRecord> {
        self.sessions
            .get(agent_id)
            .map(|s| s.checkpoint_history.clone())
            .unwrap_or_default()
    }

    /// Restore a sprite to a checkpoint
    pub async fn restore(&self, agent_id: &str, checkpoint_id: &str) -> Result<()> {
        let session = self
            .sessions
            .get(agent_id)
            .ok_or_else(|| eyre!("No sprite session for agent_id: {}", agent_id))?;

        info!("Restoring {} to checkpoint {}", agent_id, checkpoint_id);

        session
            .sprite
            .restore(checkpoint_id)
            .await
            .map_err(|e| eyre!("Restore failed: {}", e))?;

        info!("Restored to checkpoint {}", checkpoint_id);
        Ok(())
    }

    /// Destroy a sprite and clean up
    pub async fn destroy(&mut self, agent_id: &str) -> Result<()> {
        let session = self
            .sessions
            .remove(agent_id)
            .ok_or_else(|| eyre!("No sprite session for agent_id: {}", agent_id))?;

        info!("Destroying sprite for {}", agent_id);

        session
            .sprite
            .destroy()
            .await
            .map_err(|e| eyre!("Failed to destroy sprite: {}", e))?;

        info!("Sprite destroyed for {}", agent_id);
        Ok(())
    }

    /// List all active sprite sessions
    pub fn list_sessions(&self) -> impl Iterator<Item = &SpriteSession> {
        self.sessions.values()
    }

    /// Check if an agent is a sprite agent
    pub fn is_sprite_agent(&self, agent_id: &str) -> bool {
        self.sessions.contains_key(agent_id)
    }

    /// Recover existing sprites after restart
    pub async fn recover_existing(&mut self) -> Result<Vec<String>> {
        info!("Recovering existing rehoboam sprites...");

        let sprites = self
            .client
            .list()
            .await
            .map_err(|e| eyre!("Failed to list sprites: {}", e))?;

        let mut recovered = Vec::new();

        for sprite_info in sprites {
            if sprite_info.name.starts_with("rehoboam-") {
                let agent_id = format!("sprite-{}", sprite_info.name);
                let sprite = self.client.sprite(&sprite_info.name);

                let session = SpriteSession {
                    sprite,
                    agent_id: agent_id.clone(),
                    project: sprite_info
                        .name
                        .strip_prefix("rehoboam-")
                        .unwrap_or("unknown")
                        .to_string(),
                    created_at: Instant::now(), // Approximate
                    last_checkpoint: None,
                    checkpoint_history: Vec::new(),
                    ws_connected: false,
                };

                self.sessions.insert(agent_id.clone(), session);
                recovered.push(agent_id);
            }
        }

        info!("Recovered {} sprites", recovered.len());
        Ok(recovered)
    }
}
