//! Filesystem-based team discovery from ~/.claude/teams/
//!
//! Scans Claude Code's team configuration files to discover team membership.
//! Used to enrich agent metadata when env vars are not propagated to hooks.

use std::collections::HashMap;
use std::path::PathBuf;

/// A team member from config.json
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields populated from JSON, used for diagnostics/future features
pub struct TeamMember {
    /// Human-readable name (used for messaging and task assignment)
    pub name: String,
    /// Unique agent identifier
    pub agent_id: String,
    /// Role/type of the agent (e.g., "general-purpose", "Explore")
    pub agent_type: String,
    /// Model name (e.g., "claude-opus-4-6")
    pub model: Option<String>,
    /// Working directory
    pub cwd: Option<String>,
    /// Tmux pane ID for matching to live agents
    pub tmux_pane_id: Option<String>,
}

/// Parsed team configuration
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields populated from JSON, used for diagnostics/future features
pub struct TeamConfig {
    /// Team name (directory name)
    pub team_name: String,
    /// Team members
    pub members: Vec<TeamMember>,
    /// Agent ID of the team lead
    pub lead_agent_id: Option<String>,
    /// Session ID of the team lead
    pub lead_session_id: Option<String>,
}

/// Filesystem scanner for Claude Code team configs
pub struct TeamDiscovery;

impl TeamDiscovery {
    /// Scan ~/.claude/teams/ for team configurations
    ///
    /// Returns a map of team_name -> TeamConfig.
    /// Silently skips malformed configs or missing directories.
    pub fn scan_teams() -> Result<HashMap<String, TeamConfig>, std::io::Error> {
        let mut teams = HashMap::new();

        let teams_dir = Self::teams_dir()?;
        if !teams_dir.exists() {
            return Ok(teams);
        }

        let entries = std::fs::read_dir(&teams_dir)?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let config_path = path.join("config.json");
            if !config_path.exists() {
                continue;
            }

            match Self::parse_team_config(&config_path) {
                Ok(config) => {
                    tracing::debug!(
                        team = %config.team_name,
                        members = config.members.len(),
                        "Discovered team from filesystem"
                    );
                    teams.insert(config.team_name.clone(), config);
                }
                Err(e) => {
                    tracing::warn!(
                        path = %config_path.display(),
                        error = %e,
                        "Failed to parse team config, skipping"
                    );
                }
            }
        }

        Ok(teams)
    }

    /// Parse a single team config.json file
    fn parse_team_config(path: &PathBuf) -> Result<TeamConfig, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let json: serde_json::Value = serde_json::from_str(&content)?;

        // Team name from parent directory
        let team_name = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Parse members array
        let members = json
            .get("members")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| {
                        let name = m.get("name")?.as_str()?.to_string();
                        let agent_id = m
                            .get("agentId")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let agent_type = m
                            .get("agentType")
                            .and_then(|v| v.as_str())
                            .unwrap_or("general-purpose")
                            .to_string();
                        let model = m
                            .get("model")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        let cwd = m
                            .get("cwd")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        let tmux_pane_id = m
                            .get("tmuxPaneId")
                            .and_then(|v| v.as_str())
                            .map(String::from);
                        Some(TeamMember {
                            name,
                            agent_id,
                            agent_type,
                            model,
                            cwd,
                            tmux_pane_id,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let lead_agent_id = json
            .get("leadAgentId")
            .and_then(|v| v.as_str())
            .map(String::from);

        let lead_session_id = json
            .get("leadSessionId")
            .and_then(|v| v.as_str())
            .map(String::from);

        Ok(TeamConfig {
            team_name,
            members,
            lead_agent_id,
            lead_session_id,
        })
    }

    /// Get the teams directory path (~/.claude/teams/)
    fn teams_dir() -> Result<PathBuf, std::io::Error> {
        let home = std::env::var("HOME")
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::NotFound, "HOME not set"))?;
        Ok(PathBuf::from(home).join(".claude").join("teams"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_team_config() {
        let tmp = TempDir::new().unwrap();
        let team_dir = tmp.path().join("my-team");
        std::fs::create_dir_all(&team_dir).unwrap();

        let config = r#"{
            "leadAgentId": "agent-1",
            "leadSessionId": "sess-abc",
            "members": [
                {"name": "lead", "agentId": "agent-1", "agentType": "general-purpose", "model": "claude-opus-4-6", "cwd": "/tmp/work", "tmuxPaneId": "%0"},
                {"name": "worker-1", "agentId": "agent-2", "agentType": "Bash"}
            ]
        }"#;
        let config_path = team_dir.join("config.json");
        std::fs::write(&config_path, config).unwrap();

        let result = TeamDiscovery::parse_team_config(&config_path).unwrap();
        assert_eq!(result.team_name, "my-team");
        assert_eq!(result.lead_agent_id.as_deref(), Some("agent-1"));
        assert_eq!(result.lead_session_id.as_deref(), Some("sess-abc"));
        assert_eq!(result.members.len(), 2);
        assert_eq!(result.members[0].name, "lead");
        assert_eq!(result.members[0].agent_type, "general-purpose");
        assert_eq!(result.members[0].model.as_deref(), Some("claude-opus-4-6"));
        assert_eq!(result.members[0].cwd.as_deref(), Some("/tmp/work"));
        assert_eq!(result.members[0].tmux_pane_id.as_deref(), Some("%0"));
        assert_eq!(result.members[1].name, "worker-1");
        assert!(result.members[1].model.is_none());
        assert!(result.members[1].cwd.is_none());
        assert!(result.members[1].tmux_pane_id.is_none());
    }

    #[test]
    fn test_parse_team_config_missing_fields() {
        let tmp = TempDir::new().unwrap();
        let team_dir = tmp.path().join("partial-team");
        std::fs::create_dir_all(&team_dir).unwrap();

        // Minimal config with only name
        let config = r#"{
            "members": [
                {"name": "solo-agent"}
            ]
        }"#;
        let config_path = team_dir.join("config.json");
        std::fs::write(&config_path, config).unwrap();

        let result = TeamDiscovery::parse_team_config(&config_path).unwrap();
        assert_eq!(result.members.len(), 1);
        assert_eq!(result.members[0].agent_id, "");
        assert_eq!(result.members[0].agent_type, "general-purpose");
    }

    #[test]
    fn test_parse_team_config_malformed_json() {
        let tmp = TempDir::new().unwrap();
        let team_dir = tmp.path().join("bad-team");
        std::fs::create_dir_all(&team_dir).unwrap();

        let config_path = team_dir.join("config.json");
        std::fs::write(&config_path, "not json").unwrap();

        let result = TeamDiscovery::parse_team_config(&config_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_team_config_empty_members() {
        let tmp = TempDir::new().unwrap();
        let team_dir = tmp.path().join("empty-team");
        std::fs::create_dir_all(&team_dir).unwrap();

        let config = r#"{"members": []}"#;
        let config_path = team_dir.join("config.json");
        std::fs::write(&config_path, config).unwrap();

        let result = TeamDiscovery::parse_team_config(&config_path).unwrap();
        assert_eq!(result.team_name, "empty-team");
        assert!(result.members.is_empty());
    }
}
