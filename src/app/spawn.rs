//! Spawn dialog state and agent spawning logic
//!
//! Manages the spawn dialog UI state and handles agent creation across
//! different execution environments.
//!
//! # Spawn Workflow
//!
//! 1. User presses `s` to open spawn dialog
//! 2. Configure agent settings:
//!    - **Project**: Local path or GitHub repo (e.g., `owner/repo`)
//!    - **Prompt**: Initial task for the agent
//!    - **Branch**: Optional git worktree isolation
//!    - **Sprite**: Enable for remote VM execution (cloud)
//! 3. Press `Enter` to spawn or `Esc` to cancel
//!
//! # Execution Environments
//!
//! - **Local (Tmux)**: Spawns Claude Code in a new tmux pane
//! - **Sprite (Cloud)**: Spawns on remote Fly.io VM with checkpoint support

use crate::git::GitController;
use crate::sprite::config::NetworkPreset;
use crate::tmux::TmuxController;
use sprites::SpritesClient;
use std::path::PathBuf;

/// Number of fields in spawn dialog
/// 0=project, 1=prompt, 2=branch, 3=worktree, 4=claude_tasks, 5=task_list_id,
/// 6=sprite, 7=network, 8=ram, 9=cpus, 10=clone_dest
pub const SPAWN_FIELD_COUNT: usize = 11;

/// State for the spawn dialog
#[derive(Debug, Clone)]
pub struct SpawnState {
    /// Selected project path (local) OR leave empty for GitHub clone
    pub project_path: String,
    /// GitHub repository to clone (e.g., "owner/repo" or full URL)
    /// Only used when use_sprite is true and project_path is empty
    pub github_repo: String,
    /// Prompt to send to the new agent
    pub prompt: String,
    /// Branch name for git worktree (optional)
    pub branch_name: String,
    /// Whether to create a git worktree for isolation
    pub use_worktree: bool,
    /// Which field is being edited
    /// 0 = project/github, 1 = prompt, 2 = branch, 3 = worktree toggle,
    /// 4 = claude_tasks toggle, 5 = task_list_id, 6 = sprite toggle, 7 = network
    pub active_field: usize,
    /// Use Claude Code native Tasks API for task management
    /// When enabled, agents use TaskCreate/TaskUpdate instead of tasks.md
    pub use_claude_tasks: bool,
    /// Shared task list ID for multi-agent coordination
    /// Set as CLAUDE_CODE_TASK_LIST_ID env var when spawning
    pub task_list_id: String,
    /// Whether to spawn on a remote sprite (cloud VM)
    pub use_sprite: bool,
    /// Network policy for sprite (only applies when use_sprite is true)
    pub network_preset: NetworkPreset,
    /// RAM allocation in MB (default: 2048)
    /// Applies to sprite VMs; shown for local spawns for consistency
    pub ram_mb: String,
    /// Number of CPUs (default: 2)
    /// Applies to sprite VMs; shown for local spawns for consistency
    pub cpus: String,
    /// Clone destination for GitHub repos (local mode only)
    /// When project_path is a GitHub URL, this is where to clone it
    pub clone_destination: String,
    /// Validation error to display in the dialog
    pub validation_error: Option<String>,
}

/// Check if a string looks like a GitHub repository reference
///
/// Returns true for:
/// - `owner/repo` (only if path doesn't exist locally and owner matches GitHub username pattern)
/// - `https://github.com/owner/repo`
/// - `github.com/owner/repo`
/// - `git@github.com:owner/repo.git`
pub fn is_github_url(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }

    // Check for explicit GitHub URLs - these are unambiguous
    if s.contains("github.com") {
        return true;
    }

    // Check for owner/repo format (must have exactly one slash, no spaces)
    // But first ensure it's not a local path that exists
    if !s.contains(' ') && !s.starts_with('/') && !s.starts_with('.') && !s.starts_with('~') {
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            // Validate GitHub username pattern: alphanumeric and hyphens, 1-39 chars
            let owner = parts[0];
            let is_valid_github_owner = !owner.is_empty()
                && owner.len() <= 39
                && owner
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-')
                && !owner.starts_with('-')
                && !owner.ends_with('-');

            if !is_valid_github_owner {
                return false;
            }

            // Check if it exists locally - if so, it's a local path
            if std::path::Path::new(s).exists() {
                return false; // Local path takes precedence
            }
            // Doesn't exist locally and matches GitHub pattern, assume it's a GitHub repo
            return true;
        }
    }

    false
}

impl Default for SpawnState {
    fn default() -> Self {
        // Try to get current directory as default
        let default_path = std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_default();

        Self {
            project_path: default_path,
            github_repo: String::new(),
            prompt: String::new(),
            branch_name: String::new(),
            use_worktree: false,
            active_field: 0,
            use_claude_tasks: true, // Default: use Claude Code native Tasks API
            task_list_id: String::new(),
            use_sprite: false,
            network_preset: NetworkPreset::ClaudeOnly,
            ram_mb: "2048".to_string(),
            cpus: "2".to_string(),
            clone_destination: String::new(),
            validation_error: None,
        }
    }
}

/// Validate spawn dialog inputs before spawning
///
/// Returns Ok(()) if all inputs are valid, Err(message) otherwise.
///
/// `has_sprites_client` should be `true` if a sprites token is configured.
/// This prevents the "Use Sprite VM" toggle from silently falling back to local spawning.
pub fn validate_spawn(state: &SpawnState, has_sprites_client: bool) -> Result<(), String> {
    // Check sprites token availability when sprite mode is enabled
    if state.use_sprite && !has_sprites_client {
        return Err(
            "Sprite mode requires SPRITES_TOKEN. Set env var or use --sprites-token".to_string(),
        );
    }

    // Check project path (required for local mode)
    if !state.use_sprite && state.project_path.is_empty() {
        return Err("Local Directory is required".to_string());
    }

    // Check GitHub repo (required for sprite mode without local path)
    if state.use_sprite && state.project_path.is_empty() && state.github_repo.is_empty() {
        return Err("GitHub Repo or Local Directory required".to_string());
    }

    // For local mode: check if project_path is a GitHub URL
    if !state.use_sprite && is_github_url(&state.project_path) {
        // GitHub URL in local mode - require clone destination
        if state.clone_destination.is_empty() {
            return Err("Clone destination required for GitHub repos".to_string());
        }
        // Validate clone destination path
        let dest = expand_tilde(&state.clone_destination);
        if std::path::Path::new(&dest).exists() {
            return Err(format!(
                "Clone destination already exists: {}",
                state.clone_destination
            ));
        }
    } else if !state.use_sprite && !state.project_path.is_empty() {
        // Regular local path - validate it exists
        let path = expand_tilde(&state.project_path);
        if !std::path::Path::new(&path).exists() {
            return Err(format!("Directory not found: {}", state.project_path));
        }
    }

    // Check branch name if worktree enabled
    if state.use_worktree && state.branch_name.is_empty() {
        return Err("Branch name required when Git Isolation enabled".to_string());
    }

    // Validate branch name format (no spaces, special chars)
    if !state.branch_name.is_empty()
        && (state.branch_name.contains(' ') || state.branch_name.contains(".."))
    {
        return Err("Invalid branch name (no spaces or '..')".to_string());
    }

    Ok(())
}

/// Spawn a new agent (local tmux or remote sprite)
///
/// This handles the full spawning flow including:
/// - GitHub clone (if project_path is a GitHub URL in local mode)
/// - Git worktree creation (if enabled)
/// - Rehoboam loop initialization (if enabled)
/// - Sprite creation with GitHub clone (if sprite mode)
/// - Tmux pane creation (if local mode)
///
/// Returns an optional status message if there's an error.
pub fn spawn_agent(
    spawn_state: &SpawnState,
    sprites_client: Option<&SpritesClient>,
    state: &mut crate::state::AppState,
) -> Option<String> {
    // For sprites, we can use either a local project path OR a GitHub repo
    let use_github = spawn_state.use_sprite && !spawn_state.github_repo.is_empty();

    if spawn_state.project_path.is_empty() && !use_github {
        return Some("⚠ No project path or GitHub repo specified".to_string());
    }

    // Branch: Sprite spawning vs tmux spawning
    if spawn_state.use_sprite {
        if let Some(client) = sprites_client {
            spawn_sprite_agent(spawn_state, client);
            return None;
        }
        // This should be caught by validate_spawn(), but guard against it anyway
        tracing::error!(
            "Sprite mode requested but no sprites token configured. \
             This should have been caught by validation."
        );
        return Some("⚠ Sprite mode requires SPRITES_TOKEN".to_string());
    }

    // Check if we need to clone a GitHub repo for local spawning
    let project_path = if is_github_url(&spawn_state.project_path) {
        let dest = expand_tilde(&spawn_state.clone_destination);
        let dest_path = std::path::Path::new(&dest);

        tracing::info!(
            repo = %spawn_state.project_path,
            destination = %dest,
            "Cloning GitHub repository for local spawn..."
        );

        match crate::git::clone_repo(&spawn_state.project_path, dest_path) {
            Ok(path) => path.to_string_lossy().to_string(),
            Err(e) => {
                tracing::error!(error = %e, "Failed to clone repository");
                return Some(format!("⚠ Clone failed: {}", e));
            }
        }
    } else {
        spawn_state.project_path.clone()
    };

    let prompt = &spawn_state.prompt;
    let use_worktree = spawn_state.use_worktree;
    let branch_name = &spawn_state.branch_name;

    // Local tmux spawning
    spawn_tmux_agent(
        &project_path,
        prompt,
        use_worktree,
        branch_name,
        spawn_state,
        state,
    );

    None
}

/// Spawn agent on remote sprite
fn spawn_sprite_agent(spawn_state: &SpawnState, client: &SpritesClient) {
    let github_repo = spawn_state.github_repo.clone();
    let project_path = spawn_state.project_path.clone();
    let prompt = spawn_state.prompt.clone();

    tracing::info!(
        project = %project_path,
        github_repo = %github_repo,
        prompt_len = prompt.len(),
        "Spawning agent on remote sprite"
    );

    let client = client.clone();

    tokio::spawn(async move {
        // Determine sprite name and working directory
        let (sprite_name, work_dir) = if !github_repo.is_empty() {
            let repo_name = extract_repo_name(&github_repo);
            let sprite_name = format!("rehoboam-{}", repo_name.replace(['/', '.'], "-"));
            let work_dir = format!("/workspace/{}", repo_name);
            (sprite_name, work_dir)
        } else {
            let sprite_name = format!("rehoboam-{}", project_path.replace(['/', '.'], "-"));
            (sprite_name, "/workspace".to_string())
        };

        tracing::info!(sprite_name = %sprite_name, "Creating sprite...");

        match client.create(&sprite_name).await {
            Ok(sprite) => {
                tracing::info!(sprite_name = %sprite_name, "Sprite created");

                // If GitHub repo specified, clone it first
                if !github_repo.is_empty() {
                    tracing::info!(
                        sprite_name = %sprite_name,
                        repo = %github_repo,
                        "Cloning GitHub repository..."
                    );

                    let clone_target = normalize_github_repo(&github_repo);

                    let clone_result = sprite
                        .command("gh")
                        .arg("repo")
                        .arg("clone")
                        .arg(&clone_target)
                        .arg(&work_dir)
                        .output()
                        .await;

                    match clone_result {
                        Ok(output) if output.success() => {
                            tracing::info!(
                                sprite_name = %sprite_name,
                                repo = %github_repo,
                                "Repository cloned successfully"
                            );
                        }
                        Ok(output) => {
                            tracing::error!(
                                sprite_name = %sprite_name,
                                repo = %github_repo,
                                stderr = %output.stderr_str(),
                                "Failed to clone repository"
                            );
                            return;
                        }
                        Err(e) => {
                            tracing::error!(
                                sprite_name = %sprite_name,
                                repo = %github_repo,
                                error = %e,
                                "gh repo clone command failed"
                            );
                            return;
                        }
                    }
                }

                // Build Claude command - run inside tmux for input control
                // This allows us to use tmux send-keys for sprite input
                let tmux_session = format!("claude-{}", sprite_name);
                let claude_cmd = if prompt.is_empty() {
                    "claude".to_string()
                } else {
                    format!("claude '{}'", prompt.replace('\'', "'\\''"))
                };

                // Create tmux session with Claude running inside
                let tmux_cmd = format!(
                    "tmux new-session -d -s '{}' -c '{}' '{}' 2>/dev/null || tmux send-keys -t '{}' '' 2>/dev/null",
                    tmux_session, work_dir, claude_cmd, tmux_session
                );

                match sprite
                    .command("bash")
                    .arg("-c")
                    .arg(&tmux_cmd)
                    .spawn()
                    .await
                {
                    Ok(_) => {
                        tracing::info!(
                            sprite_name = %sprite_name,
                            tmux_session = %tmux_session,
                            work_dir = %work_dir,
                            "Claude Code started in sprite (tmux session: {})",
                            tmux_session
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            error = %e,
                            sprite_name = %sprite_name,
                            "Failed to start Claude in sprite"
                        );
                    }
                }
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    sprite_name = %sprite_name,
                    "Failed to create sprite"
                );
            }
        }
    });
}

/// Generate a task list ID if not provided
fn generate_task_list_id(project_path: &str) -> String {
    // Use project name + millisecond timestamp for uniqueness (prevents same-second collisions)
    let project_name = std::path::Path::new(project_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    format!("{}-{}", project_name, timestamp)
}

/// Spawn agent in local tmux pane
fn spawn_tmux_agent(
    project_path: &str,
    prompt: &str,
    use_worktree: bool,
    branch_name: &str,
    spawn_state: &SpawnState,
    state: &mut crate::state::AppState,
) {
    // Determine working directory (worktree or project)
    let working_dir: PathBuf = if use_worktree && !branch_name.is_empty() {
        let git = GitController::new(PathBuf::from(project_path));

        if !git.is_git_repo() {
            tracing::warn!(
                project = %project_path,
                "Cannot create worktree: not a git repository"
            );
            PathBuf::from(project_path)
        } else {
            match git.create_worktree(branch_name) {
                Ok(worktree_path) => {
                    tracing::info!(
                        branch = %branch_name,
                        path = %worktree_path.display(),
                        "Created isolated worktree for agent"
                    );
                    worktree_path
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        branch = %branch_name,
                        project = %project_path,
                        "Worktree creation failed, falling back to project root"
                    );
                    PathBuf::from(project_path)
                }
            }
        }
    } else {
        PathBuf::from(project_path)
    };

    // Generate or use provided task list ID (for multi-agent coordination)
    let task_list_id = if spawn_state.use_claude_tasks {
        if spawn_state.task_list_id.is_empty() {
            Some(generate_task_list_id(project_path))
        } else {
            Some(spawn_state.task_list_id.clone())
        }
    } else {
        None
    };

    tracing::info!(
        project = %project_path,
        working_dir = %working_dir.display(),
        use_worktree = use_worktree,
        use_claude_tasks = spawn_state.use_claude_tasks,
        task_list_id = ?task_list_id,
        prompt_len = prompt.len(),
        "Spawning new Claude agent"
    );

    // Build environment variables
    let env_vars: Vec<(&str, String)> = if let Some(ref id) = task_list_id {
        vec![("CLAUDE_CODE_TASK_LIST_ID", id.clone())]
    } else {
        vec![]
    };
    let env_refs: Vec<(&str, &str)> = env_vars.iter().map(|(k, v)| (*k, v.as_str())).collect();

    // Create new tmux pane in the working directory
    let working_dir_str = working_dir.to_string_lossy().to_string();
    let pane_result = if env_refs.is_empty() {
        TmuxController::split_pane(true, &working_dir_str)
    } else {
        TmuxController::split_pane_with_env(true, &working_dir_str, &env_refs)
    };

    match pane_result {
        Ok(pane_id) => {
            tracing::info!(pane_id = %pane_id, "Created new tmux pane");

            // Store task list ID on agent if using Claude Tasks
            if let Some(ref id) = task_list_id {
                state.set_agent_task_list_id(&pane_id, id.clone());
            }

            // Store working directory for git operations
            state.set_agent_working_dir(&pane_id, working_dir.clone());

            // Start Claude Code in the new pane
            start_claude_in_pane(&pane_id, prompt);
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to create tmux pane");
        }
    }
}

/// Start Claude Code in a tmux pane
fn start_claude_in_pane(pane_id: &str, prompt: &str) {
    // Start Claude in the pane
    if let Err(e) = TmuxController::send_keys(pane_id, "claude") {
        tracing::error!(error = %e, "Failed to start Claude");
        return;
    }

    // If we have a prompt, send it after a short delay
    if !prompt.is_empty() {
        let pane_id_clone = pane_id.to_string();
        let prompt_clone = prompt.to_string();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            if let Err(e) = TmuxController::send_buffered(&pane_id_clone, &prompt_clone) {
                tracing::error!(error = %e, "Failed to send prompt");
            } else {
                tracing::info!(pane_id = %pane_id_clone, "Sent initial prompt");
            }
        });
    }
}

/// Extract repository name from a GitHub URL or path
///
/// Handles:
/// - `owner/repo` -> `repo`
/// - `github.com/owner/repo` -> `repo`
/// - `https://github.com/owner/repo` -> `repo`
/// - `https://github.com/owner/repo.git` -> `repo`
pub fn extract_repo_name(input: &str) -> String {
    // Remove protocol prefix if present
    let path = input
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("git@")
        .trim_start_matches("github.com/")
        .trim_start_matches("github.com:");

    // Get the last segment (repo name)
    let repo = path
        .split('/')
        .next_back()
        .unwrap_or(path)
        .trim_end_matches(".git");

    repo.to_string()
}

/// Normalize GitHub repo input to a format gh CLI can use
///
/// Handles:
/// - `owner/repo` -> `owner/repo` (already correct)
/// - `https://github.com/owner/repo` -> `owner/repo`
/// - `github.com/owner/repo` -> `owner/repo`
/// - `git@github.com:owner/repo.git` -> `owner/repo` (SSH format)
pub fn normalize_github_repo(input: &str) -> String {
    input
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("git@")
        .replace("github.com:", "") // SSH format: git@github.com:owner/repo
        .trim_start_matches("github.com/")
        .trim_end_matches(".git")
        .trim_matches('/')
        .to_string()
}

/// Expand tilde in path to home directory
pub fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return path.replacen("~", &home.to_string_lossy(), 1);
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_github_repo() {
        assert_eq!(normalize_github_repo("owner/repo"), "owner/repo");
        assert_eq!(
            normalize_github_repo("https://github.com/owner/repo"),
            "owner/repo"
        );
        assert_eq!(
            normalize_github_repo("git@github.com:owner/repo.git"),
            "owner/repo"
        );
        assert_eq!(
            normalize_github_repo("github.com/owner/repo/"),
            "owner/repo"
        );
    }

    #[test]
    fn test_expand_tilde() {
        // Test with actual home dir
        let home = std::env::var("HOME").unwrap_or_default();
        if !home.is_empty() {
            assert_eq!(expand_tilde("~/test"), format!("{}/test", home));
            assert_eq!(expand_tilde("~/a/b/c"), format!("{}/a/b/c", home));
        }
        // Non-tilde paths unchanged
        assert_eq!(expand_tilde("/absolute/path"), "/absolute/path");
        assert_eq!(expand_tilde("relative/path"), "relative/path");
    }

    #[test]
    fn test_extract_repo_name() {
        assert_eq!(extract_repo_name("owner/repo"), "repo");
        assert_eq!(extract_repo_name("https://github.com/owner/repo"), "repo");
        assert_eq!(
            extract_repo_name("https://github.com/owner/repo.git"),
            "repo"
        );
        assert_eq!(extract_repo_name("git@github.com:owner/repo.git"), "repo");
    }
}
