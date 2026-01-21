//! Permission Policy System (Cursor-aligned auto-approve patterns)
//!
//! Provides auto-approval rules for tools and bash commands in loop mode.

#![allow(dead_code)]

use chrono::Utc;
use color_eyre::eyre::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tracing::{debug, info, warn};

/// Permission policy configuration for auto-approving tools in loop mode
///
/// Loaded from .rehoboam/policy.toml if it exists.
/// Following Cursor's approach of allowlisting safe operations.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermissionPolicy {
    /// Auto-approve configuration
    #[serde(default)]
    pub auto_approve: AutoApprovePolicy,
    /// Step-up approval rules (require explicit approval even in loop mode)
    #[serde(default)]
    pub step_up: StepUpPolicy,
    /// Approval memory settings
    #[serde(default)]
    pub memory: MemorySettings,
}

/// Auto-approve rules for tools and bash commands
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutoApprovePolicy {
    /// Tools that are always safe (read-only) - auto-approve in loop mode
    #[serde(default)]
    pub always: Vec<String>,
    /// Bash command patterns to allow (glob-style matching)
    #[serde(default)]
    pub bash_allow: Vec<String>,
    /// Bash command patterns to deny (always require approval)
    #[serde(default)]
    pub bash_deny: Vec<String>,
}

/// Step-up approval rules - require explicit approval even in loop mode
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StepUpPolicy {
    /// Tools that require step-up approval
    #[serde(default)]
    pub tools: Vec<String>,
    /// Condition for step-up (e.g., "outside_project_dir")
    #[serde(default)]
    pub condition: Option<String>,
}

/// Memory settings for remembering approved operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySettings {
    /// Whether to remember approvals across iterations
    #[serde(default)]
    pub remember_approvals: bool,
    /// How long to remember approvals (hours)
    #[serde(default = "default_approval_ttl")]
    pub approval_ttl_hours: u32,
}

impl Default for MemorySettings {
    fn default() -> Self {
        Self {
            remember_approvals: true,
            approval_ttl_hours: 24,
        }
    }
}

fn default_approval_ttl() -> u32 {
    24
}

/// A remembered approval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalEntry {
    /// Tool that was approved
    pub tool: String,
    /// Path or command that was approved (if applicable)
    #[serde(default)]
    pub target: Option<String>,
    /// When the approval was granted
    pub timestamp: i64,
}

/// Approval memory storage
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApprovalMemory {
    /// List of approved operations
    #[serde(default)]
    pub approved: Vec<ApprovalEntry>,
}

/// Permission decision result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    /// Auto-approve the operation
    Approve,
    /// Deny the operation (never allow in loop mode)
    Deny,
    /// Defer to user (no policy match)
    Defer,
}

impl PermissionDecision {
    /// Get the JSON string value for hook output
    pub fn as_json_value(self) -> Option<&'static str> {
        match self {
            PermissionDecision::Approve => Some("approve"),
            PermissionDecision::Deny => Some("deny"),
            PermissionDecision::Defer => None, // No output = defer to user
        }
    }
}

/// Default policy with safe read-only tools
pub fn default_policy() -> PermissionPolicy {
    PermissionPolicy {
        auto_approve: AutoApprovePolicy {
            always: vec![
                "Read".to_string(),
                "Glob".to_string(),
                "Grep".to_string(),
                "WebFetch".to_string(),
                "WebSearch".to_string(),
                "ListMcpResourcesTool".to_string(),
                "ReadMcpResourceTool".to_string(),
                "Task".to_string(),
            ],
            bash_allow: vec![
                "*--help*".to_string(),
                "*-h *".to_string(),
                "ls *".to_string(),
                "cat *".to_string(),
                "head *".to_string(),
                "tail *".to_string(),
                "git status*".to_string(),
                "git diff*".to_string(),
                "git log*".to_string(),
                "cargo check*".to_string(),
                "cargo test*".to_string(),
                "cargo clippy*".to_string(),
                "npm test*".to_string(),
                "npm run lint*".to_string(),
            ],
            bash_deny: vec![
                "rm -rf *".to_string(),
                "git push --force*".to_string(),
                "git push -f*".to_string(),
                "sudo *".to_string(),
                "chmod 777*".to_string(),
            ],
        },
        step_up: StepUpPolicy {
            tools: vec![],
            condition: None,
        },
        memory: MemorySettings::default(),
    }
}

/// Load permission policy from .rehoboam/policy.toml
pub fn load_policy(loop_dir: &Path) -> PermissionPolicy {
    let policy_path = loop_dir.join("policy.toml");

    if !policy_path.exists() {
        debug!("No policy.toml found, using default policy");
        return default_policy();
    }

    match fs::read_to_string(&policy_path) {
        Ok(content) => match toml::from_str(&content) {
            Ok(policy) => {
                debug!("Loaded policy from {:?}", policy_path);
                policy
            }
            Err(e) => {
                warn!("Failed to parse policy.toml: {}, using default", e);
                default_policy()
            }
        },
        Err(e) => {
            warn!("Failed to read policy.toml: {}, using default", e);
            default_policy()
        }
    }
}

/// Load approval memory from .rehoboam/approvals.json
pub fn load_approvals(loop_dir: &Path) -> ApprovalMemory {
    let approvals_path = loop_dir.join("approvals.json");

    if !approvals_path.exists() {
        return ApprovalMemory::default();
    }

    match fs::read_to_string(&approvals_path) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(memory) => memory,
            Err(e) => {
                warn!("Failed to parse approvals.json: {}", e);
                ApprovalMemory::default()
            }
        },
        Err(e) => {
            warn!("Failed to read approvals.json: {}", e);
            ApprovalMemory::default()
        }
    }
}

/// Save approval memory to .rehoboam/approvals.json
pub fn save_approvals(loop_dir: &Path, memory: &ApprovalMemory) -> Result<()> {
    let approvals_path = loop_dir.join("approvals.json");
    let content = serde_json::to_string_pretty(memory)?;
    fs::write(approvals_path, content)?;
    Ok(())
}

/// Record an approval for future reference
pub fn record_approval(loop_dir: &Path, tool: &str, target: Option<&str>) -> Result<()> {
    let mut memory = load_approvals(loop_dir);

    let timestamp = Utc::now().timestamp();

    memory.approved.push(ApprovalEntry {
        tool: tool.to_string(),
        target: target.map(String::from),
        timestamp,
    });

    // Keep only recent approvals (last 100)
    if memory.approved.len() > 100 {
        memory.approved = memory.approved.into_iter().rev().take(100).rev().collect();
    }

    save_approvals(loop_dir, &memory)?;
    debug!("Recorded approval for tool: {}, target: {:?}", tool, target);
    Ok(())
}

/// Check if an operation was previously approved and is still valid
pub fn check_approval_memory(
    loop_dir: &Path,
    tool: &str,
    target: Option<&str>,
    ttl_hours: u32,
) -> bool {
    let memory = load_approvals(loop_dir);
    let now = Utc::now().timestamp();
    let ttl_secs = i64::from(ttl_hours) * 3600;

    for entry in &memory.approved {
        if entry.tool == tool {
            // Check if within TTL
            if now - entry.timestamp > ttl_secs {
                continue;
            }

            // For tools without targets, just match tool name
            if target.is_none() && entry.target.is_none() {
                return true;
            }

            // For tools with targets, match both
            if let (Some(t), Some(et)) = (target, &entry.target) {
                if t == et {
                    return true;
                }
            }
        }
    }

    false
}

/// Simple glob-style pattern matching for bash commands
fn matches_pattern(command: &str, pattern: &str) -> bool {
    let pattern = pattern.trim();
    let command = command.trim();

    // Handle exact match
    if !pattern.contains('*') {
        return command == pattern;
    }

    // Handle prefix match (e.g., "git status*")
    if pattern.ends_with('*') && !pattern.starts_with('*') {
        let prefix = &pattern[..pattern.len() - 1];
        return command.starts_with(prefix);
    }

    // Handle suffix match (e.g., "*--help")
    if pattern.starts_with('*') && !pattern.ends_with('*') {
        let suffix = &pattern[1..];
        return command.ends_with(suffix);
    }

    // Handle contains match (e.g., "*--help*")
    if pattern.starts_with('*') && pattern.ends_with('*') {
        let middle = &pattern[1..pattern.len() - 1];
        return command.contains(middle);
    }

    // Complex patterns - treat as contains
    let clean_pattern = pattern.replace('*', "");
    command.contains(&clean_pattern)
}

/// Evaluate a permission request against the policy
///
/// Returns a decision on whether to auto-approve, deny, or defer to user.
pub fn evaluate_permission(
    loop_dir: &Path,
    tool_name: &str,
    tool_input: Option<&serde_json::Value>,
    project_dir: Option<&Path>,
) -> PermissionDecision {
    let policy = load_policy(loop_dir);

    // Check if tool is in always-approve list
    if policy.auto_approve.always.contains(&tool_name.to_string()) {
        info!("Auto-approving tool {} (in always list)", tool_name);
        return PermissionDecision::Approve;
    }

    // Special handling for Bash commands
    if tool_name == "Bash" {
        if let Some(input) = tool_input {
            if let Some(command) = input.get("command").and_then(|c| c.as_str()) {
                // Check deny list first (takes precedence)
                for pattern in &policy.auto_approve.bash_deny {
                    if matches_pattern(command, pattern) {
                        info!("Denying bash command '{}' (matches deny pattern)", command);
                        return PermissionDecision::Deny;
                    }
                }

                // Check allow list
                for pattern in &policy.auto_approve.bash_allow {
                    if matches_pattern(command, pattern) {
                        info!(
                            "Auto-approving bash command '{}' (matches allow pattern)",
                            command
                        );
                        return PermissionDecision::Approve;
                    }
                }
            }
        }
    }

    // Check step-up rules for write operations
    if policy.step_up.tools.contains(&tool_name.to_string()) {
        if let Some(condition) = &policy.step_up.condition {
            if condition == "outside_project_dir" {
                // Check if the operation targets a path outside project
                if let Some(input) = tool_input {
                    if let Some(path) = input.get("file_path").and_then(|p| p.as_str()) {
                        if let Some(proj) = project_dir {
                            let path = Path::new(path);
                            // Canonicalize both paths if possible for comparison
                            let path_canon =
                                path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
                            let proj_canon =
                                proj.canonicalize().unwrap_or_else(|_| proj.to_path_buf());

                            if !path_canon.starts_with(&proj_canon) {
                                info!(
                                    "Step-up required for {} outside project dir: {:?}",
                                    tool_name, path
                                );
                                return PermissionDecision::Defer;
                            }
                        }
                    }
                }
            }
        }
    }

    // Check approval memory
    if policy.memory.remember_approvals {
        let target = tool_input.and_then(|i| {
            i.get("file_path")
                .or_else(|| i.get("command"))
                .and_then(|v| v.as_str())
        });

        if check_approval_memory(
            loop_dir,
            tool_name,
            target,
            policy.memory.approval_ttl_hours,
        ) {
            info!("Auto-approving {} (found in approval memory)", tool_name);
            return PermissionDecision::Approve;
        }
    }

    // No match - defer to user
    PermissionDecision::Defer
}

/// Create a default policy.toml in the loop directory
pub fn create_default_policy(loop_dir: &Path) -> Result<()> {
    let policy_path = loop_dir.join("policy.toml");

    if policy_path.exists() {
        return Ok(()); // Don't overwrite existing policy
    }

    let content = r#"# Rehoboam Permission Policy
# Auto-approve patterns for loop mode

[auto_approve]
# Tools that are always safe (read-only)
always = [
    "Read",
    "Glob",
    "Grep",
    "WebFetch",
    "WebSearch",
    "ListMcpResourcesTool",
    "ReadMcpResourceTool",
    "Task",
]

# Bash patterns to allow (glob-style matching)
bash_allow = [
    "*--help*",
    "*-h *",
    "ls *",
    "cat *",
    "head *",
    "tail *",
    "git status*",
    "git diff*",
    "git log*",
    "cargo check*",
    "cargo test*",
    "cargo clippy*",
]

# Bash patterns to deny (always require approval)
bash_deny = [
    "rm -rf *",
    "git push --force*",
    "git push -f*",
    "sudo *",
    "chmod 777*",
]

[step_up]
# Tools that require explicit approval even in loop mode
tools = []
# Condition: "outside_project_dir" - step up if writing outside project
# condition = "outside_project_dir"

[memory]
# Remember approved operations
remember_approvals = true
# TTL for remembered approvals (hours)
approval_ttl_hours = 24
"#;

    fs::write(&policy_path, content)?;
    info!("Created default policy.toml at {:?}", policy_path);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_pattern() {
        // Table-driven test for glob pattern matching
        let cases = vec![
            // (command, pattern, expected, description)
            ("git status", "git status*", true, "prefix match"),
            (
                "git status --short",
                "git status*",
                true,
                "prefix with args",
            ),
            ("git diff", "git status*", false, "different command"),
            ("my-cmd --help", "*--help*", true, "contains match"),
            ("--help", "*--help*", true, "just --help"),
            ("foo bar --help baz", "*--help*", true, "help in middle"),
            ("ls -la", "ls *", true, "ls with args"),
            ("cat file.txt", "cat *", true, "cat with args"),
            ("rm -rf /", "rm -rf *", true, "rm -rf match"),
        ];

        for (command, pattern, expected, desc) in cases {
            let result = matches_pattern(command, pattern);
            assert_eq!(
                result, expected,
                "{}: pattern '{}' vs command '{}'",
                desc, pattern, command
            );
        }
    }
}
