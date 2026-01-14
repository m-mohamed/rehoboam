use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Maximum events to keep in history
pub const MAX_EVENTS: usize = 50;

/// Maximum sparkline data points
pub const MAX_SPARKLINE_POINTS: usize = 60;

/// Maximum agents to track (prevents unbounded memory growth)
pub const MAX_AGENTS: usize = 500;

/// Application configuration loaded from file
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RehoboamConfig {
    /// Sprites configuration
    #[serde(default)]
    pub sprites: SpritesConfig,

    /// Timeout configuration
    #[serde(default)]
    pub timeouts: TimeoutConfig,

    /// Reconciliation configuration
    #[serde(default)]
    pub reconciliation: ReconciliationConfig,
}

/// Timeout configuration for state transitions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    /// Seconds before Working -> Idle transition (default: 60)
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_secs: i64,

    /// Seconds before removing stale sessions (default: 300)
    #[serde(default = "default_stale_timeout")]
    pub stale_timeout_secs: i64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            idle_timeout_secs: default_idle_timeout(),
            stale_timeout_secs: default_stale_timeout(),
        }
    }
}

fn default_idle_timeout() -> i64 {
    60
}

fn default_stale_timeout() -> i64 {
    300
}

/// Configuration for tmux-based reconciliation
///
/// Reconciliation supplements unreliable Claude Code hooks by polling
/// tmux panes to detect permission prompts and repair stuck states.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconciliationConfig {
    /// Enable tmux reconciliation (default: true)
    #[serde(default = "default_reconcile_enabled")]
    pub enabled: bool,

    /// Seconds between reconciliation runs (default: 5)
    #[serde(default = "default_reconcile_interval")]
    pub interval_secs: u64,

    /// Agent is "uncertain" if no events for this many seconds (default: 30)
    #[serde(default = "default_uncertain_threshold")]
    pub uncertain_threshold_secs: i64,
}

impl Default for ReconciliationConfig {
    fn default() -> Self {
        Self {
            enabled: default_reconcile_enabled(),
            interval_secs: default_reconcile_interval(),
            uncertain_threshold_secs: default_uncertain_threshold(),
        }
    }
}

fn default_reconcile_enabled() -> bool {
    true
}

fn default_reconcile_interval() -> u64 {
    5
}

fn default_uncertain_threshold() -> i64 {
    30
}

/// Sprites-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpritesConfig {
    /// Enable sprite support
    #[serde(default)]
    pub enabled: bool,

    /// Default region for new sprites
    #[serde(default = "default_region")]
    pub default_region: String,

    /// Default RAM in MB for new sprites
    #[serde(default = "default_ram_mb")]
    pub default_ram_mb: u32,

    /// Default CPU count for new sprites
    #[serde(default = "default_cpus")]
    pub default_cpus: u32,

    /// Network preset for new sprites
    #[serde(default)]
    pub network_preset: NetworkPresetConfig,

    /// WebSocket port for receiving hook events
    #[serde(default = "default_ws_port")]
    pub ws_port: u16,

    /// Checkpoint configuration
    #[serde(default)]
    pub checkpoints: CheckpointConfig,
}

impl Default for SpritesConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            default_region: default_region(),
            default_ram_mb: default_ram_mb(),
            default_cpus: default_cpus(),
            network_preset: NetworkPresetConfig::default(),
            ws_port: default_ws_port(),
            checkpoints: CheckpointConfig::default(),
        }
    }
}

fn default_region() -> String {
    "iad".to_string()
}

fn default_ram_mb() -> u32 {
    2048
}

fn default_cpus() -> u32 {
    2
}

fn default_ws_port() -> u16 {
    9876
}

/// Network preset for sprites
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NetworkPresetConfig {
    /// Full internet access
    #[default]
    Full,
    /// Only Claude API, GitHub, npm
    ClaudeOnly,
    /// Explicit allowlist only
    Restricted,
}

/// Checkpoint configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointConfig {
    /// Enable auto-checkpointing
    #[serde(default)]
    pub auto_checkpoint: bool,

    /// Checkpoint interval in minutes
    #[serde(default = "default_checkpoint_interval")]
    pub interval_minutes: u32,
}

impl Default for CheckpointConfig {
    fn default() -> Self {
        Self {
            auto_checkpoint: false,
            interval_minutes: default_checkpoint_interval(),
        }
    }
}

fn default_checkpoint_interval() -> u32 {
    15
}

impl RehoboamConfig {
    /// Load configuration from default path (~/.config/rehoboam/config.toml)
    pub fn load() -> Self {
        Self::load_from_path(Self::default_path())
    }

    /// Get the default configuration path
    pub fn default_path() -> PathBuf {
        directories::BaseDirs::new().map_or_else(
            || PathBuf::from("~/.config/rehoboam/config.toml"),
            |dirs| dirs.config_dir().join("rehoboam").join("config.toml"),
        )
    }

    /// Load configuration from a specific path
    pub fn load_from_path(path: PathBuf) -> Self {
        if !path.exists() {
            tracing::debug!("Config file not found at {:?}, using defaults", path);
            return Self::default();
        }

        match std::fs::read_to_string(&path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(config) => {
                    tracing::info!("Loaded configuration from {:?}", path);
                    config
                }
                Err(e) => {
                    tracing::warn!("Failed to parse config file: {}, using defaults", e);
                    Self::default()
                }
            },
            Err(e) => {
                tracing::warn!("Failed to read config file: {}, using defaults", e);
                Self::default()
            }
        }
    }
}

/// Tokyo Night color palette
pub mod colors {
    use super::Color;

    pub const BG: Color = Color::Rgb(26, 27, 38); // #1a1b26
    pub const BG_LIGHT: Color = Color::Rgb(41, 46, 66); // #292e42 lighter bg
    pub const FG: Color = Color::Rgb(192, 202, 245); // #c0caf5
    pub const WORKING: Color = Color::Rgb(122, 162, 247); // #7aa2f7 blue
    pub const WORKING_BRIGHT: Color = Color::Rgb(157, 187, 255); // brighter blue for pulse
    pub const ATTENTION: Color = Color::Rgb(255, 158, 100); // #ff9e64 orange
    pub const IDLE: Color = Color::Rgb(86, 95, 137); // #565f89 gray
    pub const COMPACTING: Color = Color::Rgb(224, 175, 104); // #e0af68 yellow
    pub const BORDER: Color = Color::Rgb(59, 66, 97); // #3b4261
    pub const HIGHLIGHT: Color = Color::Rgb(187, 154, 247); // #bb9af7 purple
}
