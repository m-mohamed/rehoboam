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
//!    - **Sprite**: Enable for remote VM execution (cloud)
//! 3. Press `Enter` to spawn or `Esc` to cancel
//!
//! # Execution Environments
//!
//! - **Local (Tmux)**: Spawns Claude Code in a new tmux pane
//! - **Sprite (Cloud)**: Spawns on remote Fly.io VM with checkpoint support

use crate::sprite::config::NetworkPreset;
use crate::tmux::TmuxController;
use sprites::SpritesClient;
use std::path::PathBuf;

/// Number of fields in spawn dialog
/// 0=project/repo, 1=prompt, 2=sprite toggle, 3=network preset
pub const SPAWN_FIELD_COUNT: usize = 4;

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
    /// Whether to spawn on a remote sprite (cloud VM)
    pub use_sprite: bool,
    /// Network policy for sprite (only applies when use_sprite is true)
    pub network_preset: NetworkPreset,
    /// Which field is being edited
    /// 0 = project/github, 1 = prompt, 2 = sprite toggle, 3 = network
    pub active_field: usize,
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
            use_sprite: false,
            network_preset: NetworkPreset::ClaudeOnly,
            active_field: 0,
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

    // For local mode: validate project path exists
    if !state.use_sprite && !state.project_path.is_empty() {
        let path = expand_tilde(&state.project_path);
        if !std::path::Path::new(&path).exists() {
            return Err(format!("Directory not found: {}", state.project_path));
        }
    }

    Ok(())
}

/// Spawn a new agent (local tmux or remote sprite)
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
        return Some("No project path or GitHub repo specified".to_string());
    }

    // Branch: Sprite spawning vs tmux spawning
    if spawn_state.use_sprite {
        if let Some(client) = sprites_client {
            spawn_sprite_agent(spawn_state, client);
            return None;
        }
        tracing::error!(
            "Sprite mode requested but no sprites token configured. \
             This should have been caught by validation."
        );
        return Some("Sprite mode requires SPRITES_TOKEN".to_string());
    }

    // Local tmux spawning
    let project_path = &spawn_state.project_path;
    let prompt = &spawn_state.prompt;

    spawn_tmux_agent(project_path, prompt, state);

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

/// Spawn agent in local tmux pane
fn spawn_tmux_agent(project_path: &str, prompt: &str, _state: &mut crate::state::AppState) {
    let working_dir = PathBuf::from(project_path);

    tracing::info!(
        project = %project_path,
        working_dir = %working_dir.display(),
        prompt_len = prompt.len(),
        "Spawning new Claude agent"
    );

    // Create new tmux pane in the working directory
    let working_dir_str = working_dir.to_string_lossy().to_string();
    let pane_result = TmuxController::split_pane(true, &working_dir_str);

    match pane_result {
        Ok(pane_id) => {
            tracing::info!(pane_id = %pane_id, "Created new tmux pane");

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
