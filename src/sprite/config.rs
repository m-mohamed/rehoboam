//! Sprite configuration types

use sprites::{NetworkPolicy, NetworkPolicyRule, PolicyAction};

/// Configuration for sprite creation
#[derive(Debug, Clone)]
pub struct SpriteConfig {
    /// Sprites API token
    pub token: String,

    /// Default region for sprite creation
    pub region: String,

    /// RAM allocation in MB
    pub ram_mb: u32,

    /// Number of CPUs
    pub cpus: u32,

    /// Network policy preset
    pub network_preset: NetworkPreset,

    /// WebSocket port for receiving hook events
    pub ws_port: u16,

    /// Auto-checkpoint interval (None = disabled)
    pub checkpoint_interval_secs: Option<u64>,
}

impl Default for SpriteConfig {
    fn default() -> Self {
        Self {
            token: String::new(),
            region: "iad".to_string(),
            ram_mb: 2048,
            cpus: 2,
            network_preset: NetworkPreset::ClaudeOnly,
            ws_port: 9876,
            checkpoint_interval_secs: None,
        }
    }
}

/// Network access presets for sprites
#[derive(Debug, Clone, Copy, Default)]
pub enum NetworkPreset {
    /// Full unrestricted internet access
    Full,

    /// Only Claude API and essential development services
    #[default]
    ClaudeOnly,

    /// Completely restricted (no external network)
    Restricted,
}

impl NetworkPreset {
    /// Convert preset to sprites NetworkPolicy
    pub fn to_policy(&self) -> NetworkPolicy {
        match self {
            NetworkPreset::Full => NetworkPolicy {
                rules: vec![],
                include: vec![],
            },
            NetworkPreset::ClaudeOnly => NetworkPolicy {
                rules: vec![
                    NetworkPolicyRule {
                        domain: "api.anthropic.com".to_string(),
                        action: PolicyAction::Allow,
                    },
                    NetworkPolicyRule {
                        domain: "*.anthropic.com".to_string(),
                        action: PolicyAction::Allow,
                    },
                    NetworkPolicyRule {
                        domain: "api.github.com".to_string(),
                        action: PolicyAction::Allow,
                    },
                    NetworkPolicyRule {
                        domain: "github.com".to_string(),
                        action: PolicyAction::Allow,
                    },
                    NetworkPolicyRule {
                        domain: "registry.npmjs.org".to_string(),
                        action: PolicyAction::Allow,
                    },
                    NetworkPolicyRule {
                        domain: "*.crates.io".to_string(),
                        action: PolicyAction::Allow,
                    },
                    NetworkPolicyRule {
                        domain: "pypi.org".to_string(),
                        action: PolicyAction::Allow,
                    },
                    // Deny everything else
                    NetworkPolicyRule {
                        domain: "*".to_string(),
                        action: PolicyAction::Deny,
                    },
                ],
                include: vec![],
            },
            NetworkPreset::Restricted => NetworkPolicy {
                rules: vec![NetworkPolicyRule {
                    domain: "*".to_string(),
                    action: PolicyAction::Deny,
                }],
                include: vec![],
            },
        }
    }
}
