//! Project initialization and hook installation
//!
//! Installs Claude Code hooks to projects, supporting:
//! - Single project initialization
//! - Git-based project discovery
//! - Safe merging with existing settings
//!
//! # Configuration
//!
//! The rehoboam binary path in hooks can be configured via `REHOBOAM_PATH`
//! environment variable. Defaults to `~/.local/bin/rehoboam`.

use crate::errors::RehoboamError;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Rich project metadata for display in picker and preview
pub struct ProjectInfo {
    pub path: PathBuf,
    pub name: String,
    pub has_hooks: bool,
    /// Shortened path with ~ for home dir (e.g. ~/startups/foo)
    #[cfg_attr(not(feature = "builtin-picker"), allow(dead_code))]
    pub short_path: String,
    #[cfg_attr(not(feature = "builtin-picker"), allow(dead_code))]
    pub branch: Option<String>,
}

impl ProjectInfo {
    /// Create a display line for the fuzzy picker
    #[cfg_attr(not(feature = "builtin-picker"), allow(dead_code))]
    pub fn picker_line(&self) -> String {
        let check = if self.has_hooks { "✓" } else { " " };
        let branch = self.branch.as_deref().unwrap_or("");
        let branch_part = if branch.is_empty() {
            String::new()
        } else {
            format!(" ({branch})")
        };
        format!("{check} {}{branch_part}  {}", self.name, self.short_path)
    }
}

/// Shorten a path by replacing the home directory with ~
fn shorten_path(path: &Path) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let display = path.display().to_string();
    if !home.is_empty() && display.starts_with(&home) {
        format!("~{}", &display[home.len()..])
    } else {
        display
    }
}

/// Discover projects with rich git metadata
pub fn discover_projects_rich() -> Vec<ProjectInfo> {
    let projects = discover_projects();
    projects
        .into_iter()
        .map(|path| {
            let name = path.file_name().map_or_else(
                || "unknown".to_string(),
                |n| n.to_string_lossy().to_string(),
            );
            let has_hooks = has_rehoboam_hooks(&path);
            let short_path = shorten_path(&path);

            // Get current branch (lightweight git call)
            let branch = Command::new("git")
                .args(["rev-parse", "--abbrev-ref", "HEAD"])
                .current_dir(&path)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string());

            ProjectInfo {
                path,
                name,
                has_hooks,
                short_path,
                branch,
            }
        })
        .collect()
}

/// Get the rehoboam binary path from environment or default
///
/// Checks `REHOBOAM_PATH` environment variable first, falls back to
/// `~/.local/bin/rehoboam`.
fn get_rehoboam_path() -> String {
    std::env::var("REHOBOAM_PATH").unwrap_or_else(|_| "~/.local/bin/rehoboam".to_string())
}

/// Generate hook template with configurable binary path (v1.0)
///
/// Uses `rehoboam hook` which reads JSON from stdin - status is derived
/// automatically from hook_event_name, no manual flags needed.
///
/// Claude Code 2.1.x: Supports `once: true` for one-time hooks like SessionStart.
///
/// NOTE: Claude Code does NOT propagate CLAUDE_CODE_TEAM_NAME, CLAUDE_CODE_AGENT_ID,
/// CLAUDE_CODE_AGENT_NAME, CLAUDE_CODE_AGENT_TYPE to hook subprocesses.
/// Team identity is recovered via: (1) JSON team_name field on TeammateIdle/TaskCompleted,
/// (2) session-ID correlation, (3) ~/.claude/teams/ filesystem discovery,
/// (4) tool_input parsing from TeamCreate/SendMessage calls.
fn hook_template() -> String {
    let path = get_rehoboam_path();
    format!(
        r#"{{
  "env": {{
    "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "1"
  }},
  "hooks": {{
    "SessionStart": [{{
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 3 }}]
    }}],
    "UserPromptSubmit": [{{
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 3 }}]
    }}],
    "PermissionRequest": [{{
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 3 }}]
    }}],
    "Stop": [{{
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 3 }}]
    }}],
    "Notification": [{{
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 3, "async": true }}]
    }}],
    "PreToolUse": [{{
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 3 }}]
    }}],
    "PostToolUse": [{{
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 3, "async": true }}]
    }}],
    "PostToolUseFailure": [{{
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 3, "async": true }}]
    }}],
    "SessionEnd": [{{
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 3, "async": true }}]
    }}],
    "PreCompact": [{{
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 3, "async": true }}]
    }}],
    "SubagentStart": [{{
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 3, "async": true }}]
    }}],
    "SubagentStop": [{{
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 3, "async": true }}]
    }}],
    "TeammateIdle": [{{
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 3 }}]
    }}],
    "TaskCompleted": [{{
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 3 }}]
    }}]
  }}
}}"#
    )
}

/// Get scan roots from environment or defaults
///
/// Configurable via `REHOBOAM_SCAN_ROOTS` environment variable (comma-separated paths).
/// Defaults to: ~/projects, ~/startups, ~/obsidian, ~
fn get_scan_roots() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let home_path = PathBuf::from(&home);

    if let Ok(roots) = std::env::var("REHOBOAM_SCAN_ROOTS") {
        roots
            .split(',')
            .map(|s| {
                let expanded = s.trim().replace('~', &home);
                PathBuf::from(expanded)
            })
            .filter(|p| p.exists())
            .collect()
    } else {
        // Default scan roots
        vec![
            home_path.join("projects"),
            home_path.join("startups"),
            home_path.join("obsidian"),
            home_path,
        ]
        .into_iter()
        .filter(|p| p.exists())
        .collect()
    }
}

/// Get discovery depth from environment or default
///
/// Configurable via `REHOBOAM_DISCOVERY_DEPTH` environment variable.
/// Defaults to 3.
fn get_discovery_depth() -> usize {
    std::env::var("REHOBOAM_DISCOVERY_DEPTH")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3)
}

/// Discover git repositories using the ignore crate (same as ripgrep/fd)
///
/// Scans directories from `REHOBOAM_SCAN_ROOTS` env var or defaults.
/// Discovery depth configurable via `REHOBOAM_DISCOVERY_DEPTH` (default: 3).
/// Uses git-aware walking to respect .gitignore patterns.
pub fn discover_projects() -> Vec<PathBuf> {
    use ignore::WalkBuilder;

    let scan_roots = get_scan_roots();
    let depth = get_discovery_depth();
    let mut projects = Vec::new();
    let mut seen = HashSet::new();

    for root in scan_roots {
        // Build walker with git-aware settings
        let walker = WalkBuilder::new(&root)
            .hidden(false) // Don't skip hidden dirs (we want .git)
            .ignore(false) // Don't read .ignore files
            .git_ignore(false) // Don't skip gitignored paths
            .git_global(false) // Don't use global gitignore
            .git_exclude(false) // Don't use .git/info/exclude
            .max_depth(Some(depth)) // Configurable recursion depth
            .follow_links(false) // Don't follow symlinks
            .build();

        for entry in walker.filter_map(Result::ok) {
            let path = entry.path();

            // We're looking for .git directories
            if path.ends_with(".git") && path.is_dir() {
                if let Some(parent) = path.parent() {
                    let project_path = parent.to_path_buf();
                    if !seen.contains(&project_path) {
                        // Skip hidden directories at project level
                        if let Some(name) = project_path.file_name() {
                            if !name.to_string_lossy().starts_with('.') {
                                seen.insert(project_path.clone());
                                projects.push(project_path);
                            }
                        }
                    }
                }
            }
        }
    }

    // Sort by name for consistent ordering
    projects.sort_by(|a, b| {
        let name_a = a.file_name().map(|n| n.to_string_lossy().to_lowercase());
        let name_b = b.file_name().map(|n| n.to_string_lossy().to_lowercase());
        name_a.cmp(&name_b)
    });

    projects
}

/// Check if a project has rehoboam hooks installed
pub fn has_rehoboam_hooks(project: &Path) -> bool {
    let settings_path = project.join(".claude").join("settings.json");
    if !settings_path.exists() {
        return false;
    }

    // Read and check for rehoboam in hook commands (v1.0: "hook", legacy: "send")
    if let Ok(content) = fs::read_to_string(&settings_path) {
        return content.contains("rehoboam hook") || content.contains("rehoboam send");
    }

    false
}

/// List discovered projects with status
pub fn list_projects() {
    let projects = discover_projects();

    if projects.is_empty() {
        let roots = get_scan_roots();
        let root_strs: Vec<String> = roots.iter().map(|p| p.display().to_string()).collect();
        println!("No git repositories found.");
        println!("Scanned: {}", root_strs.join(", "));
        println!("\nConfigure with: export REHOBOAM_SCAN_ROOTS=~/projects,~/work");
        return;
    }

    println!("Discovered git repositories:\n");
    for project in &projects {
        let name = project.file_name().map_or_else(
            || "unknown".to_string(),
            |n| n.to_string_lossy().to_string(),
        );

        let status = if has_rehoboam_hooks(project) {
            "✓ initialized"
        } else {
            "  not initialized"
        };

        println!("  {} {} ({})", status, name, project.display());
    }
    println!();
}

/// Initialize a single project with hooks
pub fn init_project(project: &Path, force: bool) -> Result<(), RehoboamError> {
    let name = project.file_name().map_or_else(
        || "unknown".to_string(),
        |n| n.to_string_lossy().to_string(),
    );

    // Verify it's a directory
    if !project.is_dir() {
        return Err(RehoboamError::InitError {
            project: name,
            reason: format!("Not a directory: {}", project.display()),
        });
    }

    // Create .claude directory
    let claude_dir = project.join(".claude");
    if !claude_dir.exists() {
        fs::create_dir_all(&claude_dir).map_err(|e| RehoboamError::InitError {
            project: name.clone(),
            reason: format!("Failed to create .claude directory: {e}"),
        })?;
    }

    let settings_path = claude_dir.join("settings.json");

    // Parse our settings template (hooks + env)
    let template = hook_template();
    let our_settings: serde_json::Value =
        serde_json::from_str(&template).map_err(|e| RehoboamError::InitError {
            project: name.clone(),
            reason: format!("Failed to parse settings template: {e}"),
        })?;

    // Handle existing settings
    // Strategy: always preserve user's non-rehoboam settings (permissions, attribution,
    // model, enabledPlugins, MCP config, etc). Only modify `hooks` and `env` keys.
    let final_settings = if settings_path.exists() {
        // Read existing settings
        let existing_content =
            fs::read_to_string(&settings_path).map_err(|e| RehoboamError::InitError {
                project: name.clone(),
                reason: format!("Failed to read existing settings: {e}"),
            })?;

        let mut existing: serde_json::Value =
            serde_json::from_str(&existing_content).map_err(|e| RehoboamError::InitError {
                project: name.clone(),
                reason: format!("Failed to parse existing settings: {e}"),
            })?;

        // Merge env vars (add ours without overwriting existing keys)
        if let Some(our_env) = our_settings.get("env").and_then(|v| v.as_object()) {
            if let Some(obj) = existing.as_object_mut() {
                let existing_env = obj
                    .entry("env")
                    .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
                if let Some(env_obj) = existing_env.as_object_mut() {
                    for (key, value) in our_env {
                        env_obj.entry(key.clone()).or_insert_with(|| value.clone());
                    }
                }
            }
        }

        if force {
            // Force: replace hooks entirely but preserve all other user settings
            if let Some(obj) = existing.as_object_mut() {
                obj.insert(
                    "hooks".to_string(),
                    our_settings
                        .get("hooks")
                        .cloned()
                        .expect("template must contain 'hooks' key"),
                );
            }
            existing
        } else {
            // Merge hooks (add missing hook types, skip where rehoboam already present)
            if let (Some(existing_hooks), Some(our_hooks_obj)) =
                (existing.get_mut("hooks"), our_settings.get("hooks"))
            {
                if let (Some(existing_obj), Some(our_obj)) =
                    (existing_hooks.as_object_mut(), our_hooks_obj.as_object())
                {
                    for (hook_type, our_hook_array) in our_obj {
                        if let Some(existing_array) = existing_obj.get_mut(hook_type) {
                            // Check if rehoboam already present (v1.0 "hook" or legacy "send")
                            if let Some(arr) = existing_array.as_array() {
                                let has_rehoboam = arr.iter().any(|entry| {
                                    entry.get("hooks").and_then(|h| h.as_array()).is_some_and(
                                        |hooks| {
                                            hooks.iter().any(|h| {
                                                h.get("command")
                                                    .and_then(|c| c.as_str())
                                                    .is_some_and(|s| {
                                                        s.contains("rehoboam hook")
                                                            || s.contains("rehoboam send")
                                                    })
                                            })
                                        },
                                    )
                                });

                                if !has_rehoboam {
                                    // Append our hook to existing array
                                    if let Some(arr_mut) = existing_array.as_array_mut() {
                                        if let Some(our_arr) = our_hook_array.as_array() {
                                            arr_mut.extend(our_arr.clone());
                                        }
                                    }
                                }
                            }
                        } else {
                            // No existing hooks for this type, add ours
                            existing_obj.insert(hook_type.clone(), our_hook_array.clone());
                        }
                    }
                }
                existing
            } else if existing.get("hooks").is_none() {
                // No hooks key at all, add ours
                if let Some(obj) = existing.as_object_mut() {
                    obj.insert(
                        "hooks".to_string(),
                        our_settings
                            .get("hooks")
                            .cloned()
                            .expect("template must contain 'hooks' key"),
                    );
                }
                existing
            } else {
                existing
            }
        }
    } else {
        // No existing settings file, use our template
        our_settings
    };

    // Write settings with pretty formatting
    let formatted =
        serde_json::to_string_pretty(&final_settings).map_err(|e| RehoboamError::InitError {
            project: name.clone(),
            reason: format!("Failed to serialize settings: {e}"),
        })?;

    fs::write(&settings_path, formatted).map_err(|e| RehoboamError::InitError {
        project: name.clone(),
        reason: format!("Failed to write settings: {e}"),
    })?;

    println!("  ✓ {name} - hooks installed");
    Ok(())
}

/// Batch-initialize all discovered projects (scripting-friendly, no prompt)
fn batch_init_all(force: bool) -> Result<(), RehoboamError> {
    let projects = discover_projects();
    if projects.is_empty() {
        println!("No git repositories found.");
        return Ok(());
    }

    println!("Initializing {} project(s)...\n", projects.len());
    let mut success_count = 0;
    for project in &projects {
        if let Err(e) = init_project(project, force) {
            eprintln!("  ✗ {e}");
        } else {
            success_count += 1;
        }
    }
    println!("\n✓ Initialized {} project(s)", success_count);

    println!("\nNext steps:");
    println!("  1. Run 'rehoboam' in Terminal 1 (dashboard)");
    println!("  2. Run 'claude' in Terminal 2 (in any initialized project)");

    Ok(())
}

/// Initialize selected projects with success summary
fn init_selected(projects: Vec<PathBuf>, force: bool) -> Result<(), RehoboamError> {
    if projects.is_empty() {
        println!("No projects selected.");
        return Ok(());
    }

    println!("\nInitializing {} project(s)...\n", projects.len());
    let mut success_count = 0;
    for project in &projects {
        if let Err(e) = init_project(project, force) {
            eprintln!("  ✗ {e}");
        } else {
            success_count += 1;
        }
    }
    println!("\n✓ Initialized {} project(s)", success_count);

    println!("\nNext steps:");
    println!("  1. Run 'rehoboam' in Terminal 1 (dashboard)");
    println!("  2. Run 'claude' in Terminal 2 (in any initialized project)");

    Ok(())
}

/// Run init command
pub fn run(path: Option<PathBuf>, all: bool, list: bool, force: bool) -> Result<(), RehoboamError> {
    // List mode
    if list {
        list_projects();
        return Ok(());
    }

    // --all: batch install ALL discovered projects (no prompt)
    if all {
        return batch_init_all(force);
    }

    // Explicit path provided → init that path
    if let Some(ref p) = path {
        if !p.is_dir() {
            return Err(RehoboamError::DiscoveryError(format!(
                "Directory does not exist: {}",
                p.display()
            )));
        }
        if !p.join(".git").exists() {
            println!("Warning: {} is not a git repository", p.display());
        }
        init_project(p, force)?;
        let settings_path = p.join(".claude").join("settings.json");
        println!("✓ Installed hooks to {}", settings_path.display());
        println!("✓ Configured 14 hook events");
        println!("\nNext steps:");
        println!("  1. Run 'rehoboam' in Terminal 1 (dashboard)");
        println!("  2. Run 'claude' in Terminal 2 (agent)");
        println!("\nVerify with: claude /hooks");
        return Ok(());
    }

    // No path provided → launch fuzzy picker (shows init status for all projects)
    let projects = discover_projects_rich();
    if projects.is_empty() {
        println!("No git repositories found.");
        let roots = get_scan_roots();
        let root_strs: Vec<String> = roots.iter().map(|p| p.display().to_string()).collect();
        println!("Scanned: {}", root_strs.join(", "));
        println!("\nConfigure with: export REHOBOAM_SCAN_ROOTS=~/projects,~/work");
        return Ok(());
    }

    let selected = crate::picker::pick_projects(&projects);
    init_selected(selected, force)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_init_new_project() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        fs::create_dir(project.join(".git")).unwrap();

        init_project(project, false).unwrap();

        let settings = project.join(".claude/settings.json");
        assert!(settings.exists(), "settings.json should be created");

        let content = fs::read_to_string(&settings).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert!(
            content.contains("rehoboam hook"),
            "should contain rehoboam hooks (v1.0)"
        );
        assert!(
            content.contains("SessionStart"),
            "should have SessionStart hook"
        );
        assert!(content.contains("Stop"), "should have Stop hook");
        assert_eq!(
            parsed["env"]["CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"], "1",
            "should enable agent teams"
        );
    }

    #[test]
    fn test_init_merge_with_existing() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        fs::create_dir(project.join(".git")).unwrap();
        fs::create_dir(project.join(".claude")).unwrap();

        // Existing settings with user hooks
        let existing = r#"{
            "hooks": {
                "PreToolUse": [{
                    "matcher": "*",
                    "hooks": [{"type": "command", "command": "echo user hook"}]
                }]
            }
        }"#;
        fs::write(project.join(".claude/settings.json"), existing).unwrap();

        init_project(project, false).unwrap();

        let content = fs::read_to_string(project.join(".claude/settings.json")).unwrap();
        // Should have BOTH user hook AND rehoboam hook
        assert!(
            content.contains("echo user hook"),
            "should preserve user hook"
        );
        assert!(
            content.contains("rehoboam hook"),
            "should add rehoboam hooks (v1.0)"
        );
    }

    #[test]
    fn test_init_skip_if_rehoboam_exists() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        fs::create_dir(project.join(".git")).unwrap();
        fs::create_dir(project.join(".claude")).unwrap();

        // Already has rehoboam for PreToolUse (user's existing hook)
        let existing = r#"{
            "hooks": {
                "PreToolUse": [{
                    "matcher": "*",
                    "hooks": [{"type": "command", "command": "rehoboam send -S working"}]
                }]
            }
        }"#;
        fs::write(project.join(".claude/settings.json"), existing).unwrap();

        init_project(project, false).unwrap();

        let content = fs::read_to_string(project.join(".claude/settings.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

        // PreToolUse should NOT be duplicated - check it has exactly 1 entry
        let pretool_hooks = parsed["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(
            pretool_hooks.len(),
            1,
            "PreToolUse should not be duplicated"
        );

        // The single entry should be the original user hook (without the path prefix)
        let cmd = pretool_hooks[0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(
            cmd, "rehoboam send -S working",
            "should preserve original hook"
        );

        // Other hook types should be added
        assert!(
            parsed["hooks"]["SessionStart"].is_array(),
            "should add SessionStart"
        );
        assert!(parsed["hooks"]["Stop"].is_array(), "should add Stop");
    }

    #[test]
    fn test_init_force_replaces_hooks_preserves_rest() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        fs::create_dir(project.join(".git")).unwrap();
        fs::create_dir(project.join(".claude")).unwrap();

        // Existing settings with user's custom keys and old hooks
        let existing = r#"{"permissions": {"allow": ["Read"]}, "hooks": {"Stop": []}}"#;
        fs::write(project.join(".claude/settings.json"), existing).unwrap();

        init_project(project, true).unwrap(); // force = true

        let content = fs::read_to_string(project.join(".claude/settings.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

        // Force should replace hooks entirely
        assert!(
            content.contains("rehoboam hook"),
            "should have rehoboam hooks (v1.0)"
        );
        // Force should preserve non-hook settings
        assert!(
            parsed["permissions"]["allow"].is_array(),
            "force should preserve permissions"
        );
        // Force should add env
        assert_eq!(
            parsed["env"]["CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"], "1",
            "should add agent teams env var"
        );
    }

    #[test]
    fn test_init_env_merge_preserves_existing() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        fs::create_dir(project.join(".git")).unwrap();
        fs::create_dir(project.join(".claude")).unwrap();

        // Existing settings with user's own env vars
        let existing = r#"{
            "env": {
                "MY_API_KEY": "secret123",
                "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS": "0"
            },
            "permissions": {"allow": ["Read"]},
            "hooks": {}
        }"#;
        fs::write(project.join(".claude/settings.json"), existing).unwrap();

        init_project(project, false).unwrap();

        let content = fs::read_to_string(project.join(".claude/settings.json")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();

        // Should preserve user's env vars
        assert_eq!(
            parsed["env"]["MY_API_KEY"], "secret123",
            "should preserve user env vars"
        );
        // Should NOT overwrite existing agent teams value (user explicitly set to 0)
        assert_eq!(
            parsed["env"]["CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"], "0",
            "should not overwrite existing env vars"
        );
        // Should preserve permissions
        assert!(
            parsed["permissions"]["allow"].is_array(),
            "should preserve permissions"
        );
    }

    #[test]
    fn test_has_rehoboam_hooks_false_no_dir() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();

        assert!(
            !has_rehoboam_hooks(project),
            "no .claude dir means no hooks"
        );
    }

    #[test]
    fn test_has_rehoboam_hooks_false_empty() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        fs::create_dir(project.join(".claude")).unwrap();
        fs::write(project.join(".claude/settings.json"), "{}").unwrap();

        assert!(
            !has_rehoboam_hooks(project),
            "empty settings means no hooks"
        );
    }

    #[test]
    fn test_has_rehoboam_hooks_true_v1() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        fs::create_dir(project.join(".claude")).unwrap();
        fs::write(
            project.join(".claude/settings.json"),
            r#"{"hooks": {"Stop": [{"hooks": [{"command": "rehoboam hook -N"}]}]}}"#,
        )
        .unwrap();

        assert!(
            has_rehoboam_hooks(project),
            "should detect v1.0 rehoboam hooks"
        );
    }

    #[test]
    fn test_has_rehoboam_hooks_true_legacy() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        fs::create_dir(project.join(".claude")).unwrap();
        fs::write(
            project.join(".claude/settings.json"),
            r#"{"hooks": {"Stop": [{"hooks": [{"command": "rehoboam send -S idle"}]}]}}"#,
        )
        .unwrap();

        assert!(
            has_rehoboam_hooks(project),
            "should detect legacy rehoboam hooks"
        );
    }

    #[test]
    fn test_init_adds_missing_hook_types() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        fs::create_dir(project.join(".git")).unwrap();
        fs::create_dir(project.join(".claude")).unwrap();

        // Existing settings with only one hook type
        let existing = r#"{
            "hooks": {
                "Stop": [{
                    "matcher": "",
                    "hooks": [{"type": "command", "command": "echo stop"}]
                }]
            }
        }"#;
        fs::write(project.join(".claude/settings.json"), existing).unwrap();

        init_project(project, false).unwrap();

        let content = fs::read_to_string(project.join(".claude/settings.json")).unwrap();
        // Should add missing hook types
        assert!(content.contains("SessionStart"), "should add SessionStart");
        assert!(content.contains("PreToolUse"), "should add PreToolUse");
        assert!(content.contains("PreCompact"), "should add PreCompact");
        // Should preserve user's Stop hook
        assert!(content.contains("echo stop"), "should preserve user hook");
    }

    #[test]
    fn test_hook_template_has_all_events() {
        let template = hook_template();
        let parsed: serde_json::Value = serde_json::from_str(&template).unwrap();
        let hooks = parsed["hooks"].as_object().unwrap();

        // Should have 14 hook events (all valid Claude Code hooks)
        assert_eq!(hooks.len(), 14, "template should have 14 hook events");

        // Verify key hooks are present
        assert!(
            hooks.contains_key("PostToolUseFailure"),
            "should have PostToolUseFailure"
        );

        // Verify all expected hooks
        let expected = [
            "SessionStart",
            "UserPromptSubmit",
            "PermissionRequest",
            "Stop",
            "Notification",
            "PreToolUse",
            "PostToolUse",
            "PostToolUseFailure",
            "SessionEnd",
            "PreCompact",
            "SubagentStart",
            "SubagentStop",
            "TeammateIdle",
            "TaskCompleted",
        ];
        for hook_name in expected {
            assert!(
                hooks.contains_key(hook_name),
                "should have {hook_name} hook"
            );
        }
    }
}
