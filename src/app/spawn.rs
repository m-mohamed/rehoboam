//! Spawn dialog state and agent spawning logic

use crate::git::GitController;
use crate::ralph::{self, RalphConfig};
use crate::sprite::config::NetworkPreset;
use crate::tmux::TmuxController;
use sprites::SpritesClient;
use std::path::PathBuf;

/// Number of fields in spawn dialog
/// 0=project, 1=prompt, 2=branch, 3=worktree, 4=loop, 5=max_iter, 6=stop_word, 7=sprite, 8=network
pub const SPAWN_FIELD_COUNT: usize = 9;

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
    /// 0 = project/github, 1 = prompt, 2 = branch, 3 = worktree toggle, 4 = loop toggle,
    /// 5 = max iter, 6 = stop word, 7 = sprite toggle, 8 = network policy
    pub active_field: usize,
    // v0.9.0 Loop Mode fields
    /// Whether to enable loop mode for the new agent
    pub loop_enabled: bool,
    /// Maximum iterations before stopping (default: 20)
    pub loop_max_iterations: String,
    /// Stop word to detect completion (default: "COMPLETE")
    pub loop_stop_word: String,
    /// Whether to spawn on a remote sprite (cloud VM)
    pub use_sprite: bool,
    /// Network policy for sprite (only applies when use_sprite is true)
    pub network_preset: NetworkPreset,
    /// Validation error to display in the dialog
    pub validation_error: Option<String>,
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
            loop_enabled: false,
            loop_max_iterations: "20".to_string(),
            loop_stop_word: "COMPLETE".to_string(),
            use_sprite: false,
            network_preset: NetworkPreset::ClaudeOnly,
            validation_error: None,
        }
    }
}

/// Validate spawn dialog inputs before spawning
///
/// Returns Ok(()) if all inputs are valid, Err(message) otherwise.
pub fn validate_spawn(state: &SpawnState) -> Result<(), String> {
    // Check project path (required for local mode)
    if !state.use_sprite && state.project_path.is_empty() {
        return Err("Local Directory is required".to_string());
    }

    // Check GitHub repo (required for sprite mode without local path)
    if state.use_sprite && state.project_path.is_empty() && state.github_repo.is_empty() {
        return Err("GitHub Repo or Local Directory required".to_string());
    }

    // Validate local path exists (only for local mode)
    if !state.use_sprite && !state.project_path.is_empty() {
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

    // Validate max iterations
    if state.loop_enabled {
        match state.loop_max_iterations.parse::<u32>() {
            Ok(0) => return Err("Max iterations must be > 0".to_string()),
            Ok(n) if n > 1000 => return Err("Max iterations too high (max 1000)".to_string()),
            Err(_) => return Err("Invalid max iterations number".to_string()),
            _ => {}
        }
    }

    Ok(())
}

/// Spawn a new agent (local tmux or remote sprite)
///
/// This handles the full spawning flow including:
/// - Git worktree creation (if enabled)
/// - Ralph loop initialization (if enabled)
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

    let project_path = &spawn_state.project_path;
    let prompt = &spawn_state.prompt;
    let use_worktree = spawn_state.use_worktree;
    let branch_name = &spawn_state.branch_name;

    // Branch: Sprite spawning vs tmux spawning
    if spawn_state.use_sprite {
        if let Some(client) = sprites_client {
            spawn_sprite_agent(spawn_state, client);
            return None;
        }
        tracing::warn!(
            "Sprite mode requested but no sprites token configured. \
             Set SPRITES_TOKEN or use --sprites-token. Falling back to tmux."
        );
        // Fall through to tmux spawning
    }

    // Local tmux spawning
    spawn_tmux_agent(
        project_path,
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
    let loop_enabled = spawn_state.loop_enabled;
    let max_iter = spawn_state.loop_max_iterations.parse::<u32>().unwrap_or(50);
    let stop_word = spawn_state.loop_stop_word.clone();

    tracing::info!(
        project = %project_path,
        github_repo = %github_repo,
        prompt_len = prompt.len(),
        loop_enabled = loop_enabled,
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

                // Build Claude command
                let claude_cmd = if prompt.is_empty() {
                    "claude".to_string()
                } else {
                    format!("claude '{}'", prompt.replace('\'', "'\\''"))
                };

                match sprite
                    .command("bash")
                    .arg("-c")
                    .arg(&claude_cmd)
                    .current_dir(&work_dir)
                    .spawn()
                    .await
                {
                    Ok(_) => {
                        tracing::info!(
                            sprite_name = %sprite_name,
                            work_dir = %work_dir,
                            loop_enabled = loop_enabled,
                            "Claude Code started in sprite"
                        );
                        if loop_enabled {
                            tracing::debug!(
                                max_iter = max_iter,
                                stop_word = %stop_word,
                                "Loop mode enabled for sprite"
                            );
                        }
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
                    tracing::error!(
                        error = %e,
                        branch = %branch_name,
                        "Failed to create worktree, using project path"
                    );
                    PathBuf::from(project_path)
                }
            }
        }
    } else {
        PathBuf::from(project_path)
    };

    tracing::info!(
        project = %project_path,
        working_dir = %working_dir.display(),
        use_worktree = use_worktree,
        prompt_len = prompt.len(),
        "Spawning new Claude agent"
    );

    // Create new tmux pane in the working directory
    let working_dir_str = working_dir.to_string_lossy().to_string();
    match TmuxController::split_pane(true, &working_dir_str) {
        Ok(pane_id) => {
            tracing::info!(pane_id = %pane_id, "Created new tmux pane");

            // Initialize Ralph loop if loop mode is enabled
            let ralph_dir = if spawn_state.loop_enabled && !prompt.is_empty() {
                let max_iter = spawn_state.loop_max_iterations.parse::<u32>().unwrap_or(50);
                let config = RalphConfig {
                    max_iterations: max_iter,
                    stop_word: spawn_state.loop_stop_word.clone(),
                    pane_id: pane_id.clone(),
                };

                match ralph::init_ralph_dir(&working_dir, prompt, &config) {
                    Ok(dir) => {
                        let _ =
                            ralph::log_session_transition(&dir, "init", "starting", Some(&pane_id));
                        let _ = ralph::mark_iteration_start(&dir);

                        tracing::info!(
                            ralph_dir = ?dir,
                            "Initialized Ralph loop directory"
                        );
                        Some(dir)
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to initialize Ralph directory");
                        None
                    }
                }
            } else {
                None
            };

            // Register loop config if loop mode is enabled
            if spawn_state.loop_enabled {
                let max_iter = spawn_state.loop_max_iterations.parse::<u32>().unwrap_or(50);
                state.register_loop_config(
                    &pane_id,
                    max_iter,
                    &spawn_state.loop_stop_word,
                    ralph_dir.clone(),
                );
            }

            // Store working directory for git operations
            state.set_agent_working_dir(&pane_id, working_dir.clone());

            // Start Claude Code in the new pane
            start_claude_in_pane(&pane_id, prompt, ralph_dir.as_ref());
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to create tmux pane");
        }
    }
}

/// Start Claude Code in a tmux pane
fn start_claude_in_pane(pane_id: &str, prompt: &str, ralph_dir: Option<&PathBuf>) {
    if let Some(ralph_dir) = ralph_dir {
        // Ralph loop mode: pipe the iteration prompt to Claude
        match ralph::build_iteration_prompt(ralph_dir) {
            Ok(prompt_file) => {
                let cmd = format!("cat '{}' | claude", prompt_file);
                if let Err(e) = TmuxController::send_keys(pane_id, &cmd) {
                    tracing::error!(error = %e, "Failed to start Claude with prompt file");
                    return;
                }
                tracing::info!(
                    pane_id = %pane_id,
                    prompt_file = %prompt_file,
                    "Started Claude in Ralph loop mode"
                );
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to build iteration prompt");
                // Fall back to normal start
                if let Err(e) = TmuxController::send_keys(pane_id, "claude") {
                    tracing::error!(error = %e, "Failed to start Claude");
                }
            }
        }
    } else {
        // Normal mode: start Claude and send prompt via buffer
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
}

/// Extract repository name from a GitHub URL or path
///
/// Handles:
/// - "owner/repo" -> "repo"
/// - "github.com/owner/repo" -> "repo"
/// - "https://github.com/owner/repo" -> "repo"
/// - "https://github.com/owner/repo.git" -> "repo"
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
/// - "owner/repo" -> "owner/repo" (already correct)
/// - "https://github.com/owner/repo" -> "owner/repo"
/// - "github.com/owner/repo" -> "owner/repo"
/// - "git@github.com:owner/repo.git" -> "owner/repo" (SSH format)
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
