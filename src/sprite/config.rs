//! Sprite configuration types

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
    /// Cycle to next preset
    pub fn next(&self) -> Self {
        match self {
            NetworkPreset::Full => NetworkPreset::ClaudeOnly,
            NetworkPreset::ClaudeOnly => NetworkPreset::Restricted,
            NetworkPreset::Restricted => NetworkPreset::Full,
        }
    }

    /// Cycle to previous preset
    pub fn prev(&self) -> Self {
        match self {
            NetworkPreset::Full => NetworkPreset::Restricted,
            NetworkPreset::ClaudeOnly => NetworkPreset::Full,
            NetworkPreset::Restricted => NetworkPreset::ClaudeOnly,
        }
    }

    /// Human-readable display name
    pub fn display(&self) -> &'static str {
        match self {
            NetworkPreset::Full => "Full Internet",
            NetworkPreset::ClaudeOnly => "Claude Only (API + registries)",
            NetworkPreset::Restricted => "No Network Access",
        }
    }
}
