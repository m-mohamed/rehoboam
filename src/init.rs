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
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

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
fn hook_template() -> String {
    let path = get_rehoboam_path();
    format!(
        r#"{{
  "hooks": {{
    "SessionStart": [{{
      "matcher": "*",
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 5 }}]
    }}],
    "UserPromptSubmit": [{{
      "matcher": "",
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 5 }}]
    }}],
    "PermissionRequest": [{{
      "matcher": "",
      "hooks": [{{ "type": "command", "command": "{path} hook -N", "timeout": 10 }}]
    }}],
    "Stop": [{{
      "matcher": "",
      "hooks": [{{ "type": "command", "command": "{path} hook -N", "timeout": 10 }}]
    }}],
    "Notification": [{{
      "matcher": "",
      "hooks": [{{ "type": "command", "command": "{path} hook -N", "timeout": 5 }}]
    }}],
    "PreToolUse": [{{
      "matcher": "",
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 10 }}]
    }}],
    "PostToolUse": [{{
      "matcher": "",
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 10 }}]
    }}],
    "SessionEnd": [{{
      "matcher": "*",
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 5 }}]
    }}],
    "PreCompact": [{{
      "matcher": "*",
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 10 }}]
    }}],
    "SubagentStart": [{{
      "matcher": "*",
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 5 }}]
    }}],
    "SubagentStop": [{{
      "matcher": "*",
      "hooks": [{{ "type": "command", "command": "{path} hook", "timeout": 5 }}]
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

/// Interactive project selection
pub fn select_projects() -> Vec<PathBuf> {
    let projects = discover_projects();

    if projects.is_empty() {
        println!("No git repositories found.");
        return Vec::new();
    }

    println!("Select projects to initialize (enter numbers separated by spaces, or 'all'):\n");

    for (i, project) in projects.iter().enumerate() {
        let name = project.file_name().map_or_else(
            || "unknown".to_string(),
            |n| n.to_string_lossy().to_string(),
        );

        let status = if has_rehoboam_hooks(project) {
            " [already initialized]"
        } else {
            ""
        };

        println!("  [{}] {}{}", i + 1, name, status);
    }

    print!("\nSelection: ");
    let _ = io::stdout().flush();

    let mut input = String::new();
    if io::stdin().lock().read_line(&mut input).is_err() {
        return Vec::new();
    }

    let input = input.trim().to_lowercase();

    if input == "all" {
        return projects;
    }

    // Parse space-separated numbers
    let mut selected = Vec::new();
    for part in input.split_whitespace() {
        if let Ok(num) = part.parse::<usize>() {
            if num > 0 && num <= projects.len() {
                selected.push(projects[num - 1].clone());
            }
        }
    }

    selected
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

    // Parse our template (with configurable path)
    let template = hook_template();
    let our_hooks: serde_json::Value =
        serde_json::from_str(&template).map_err(|e| RehoboamError::InitError {
            project: name.clone(),
            reason: format!("Failed to parse hook template: {e}"),
        })?;

    // Handle existing settings
    let final_settings = if settings_path.exists() && !force {
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

        // Merge hooks
        if let (Some(existing_hooks), Some(our_hooks_obj)) =
            (existing.get_mut("hooks"), our_hooks.get("hooks"))
        {
            // For each hook type, merge arrays
            if let (Some(existing_obj), Some(our_obj)) =
                (existing_hooks.as_object_mut(), our_hooks_obj.as_object())
            {
                for (hook_type, our_hook_array) in our_obj {
                    if let Some(existing_array) = existing_obj.get_mut(hook_type) {
                        // Check if rehoboam already present (v1.0 "hook" or legacy "send")
                        if let Some(arr) = existing_array.as_array() {
                            let has_rehoboam = arr.iter().any(|entry| {
                                entry
                                    .get("hooks")
                                    .and_then(|h| h.as_array())
                                    .is_some_and(|hooks| {
                                        hooks.iter().any(|h| {
                                            h.get("command").and_then(|c| c.as_str()).is_some_and(
                                                |s| {
                                                    s.contains("rehoboam hook")
                                                        || s.contains("rehoboam send")
                                                },
                                            )
                                        })
                                    })
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
            // No hooks key, add ours
            if let Some(obj) = existing.as_object_mut() {
                obj.insert(
                    "hooks".to_string(),
                    our_hooks
                        .get("hooks")
                        .cloned()
                        .expect("hook template must contain 'hooks' key"),
                );
            }
            existing
        } else {
            existing
        }
    } else {
        // No existing settings or force mode, use our template
        our_hooks
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

/// Run init command
pub fn run(path: Option<PathBuf>, all: bool, list: bool, force: bool) -> Result<(), RehoboamError> {
    // List mode
    if list {
        list_projects();
        return Ok(());
    }

    // All mode - interactive selection
    if all {
        let projects = select_projects();
        if projects.is_empty() {
            println!("No projects selected.");
            return Ok(());
        }

        println!("\nInitializing {} project(s)...\n", projects.len());
        for project in &projects {
            if let Err(e) = init_project(project, force) {
                eprintln!("  ✗ {e}");
            }
        }
        println!("\nDone!");
        return Ok(());
    }

    // Single project mode
    let project = path.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    if !project.is_dir() {
        return Err(RehoboamError::DiscoveryError(format!(
            "Directory does not exist: {}",
            project.display()
        )));
    }

    // Check if it's a git repo
    if !project.join(".git").exists() {
        println!("Warning: {} is not a git repository", project.display());
    }

    init_project(&project, force)?;

    println!("\nHooks enabled (11):");
    println!("  SessionStart, UserPromptSubmit, PermissionRequest, Stop, Notification,");
    println!("  PreToolUse, PostToolUse, SessionEnd, PreCompact,");
    println!("  SubagentStart, SubagentStop");
    println!("\nTest with: claude /hooks");

    Ok(())
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
        assert!(
            content.contains("rehoboam hook"),
            "should contain rehoboam hooks (v1.0)"
        );
        assert!(
            content.contains("SessionStart"),
            "should have SessionStart hook"
        );
        assert!(content.contains("Stop"), "should have Stop hook");
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
    fn test_init_force_overwrites() {
        let tmp = TempDir::new().unwrap();
        let project = tmp.path();
        fs::create_dir(project.join(".git")).unwrap();
        fs::create_dir(project.join(".claude")).unwrap();

        let existing = r#"{"custom": "value", "hooks": {}}"#;
        fs::write(project.join(".claude/settings.json"), existing).unwrap();

        init_project(project, true).unwrap(); // force = true

        let content = fs::read_to_string(project.join(".claude/settings.json")).unwrap();
        assert!(
            !content.contains("custom"),
            "force should overwrite existing"
        );
        assert!(
            content.contains("rehoboam hook"),
            "should have rehoboam hooks (v1.0)"
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
}
