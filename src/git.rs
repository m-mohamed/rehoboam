//! Git operations for Claude Code agent orchestration
//!
//! Provides worktree management for isolated agent work:
//! - Create worktrees for parallel development
//! - List existing worktrees
//! - Remove completed worktrees
//! - Checkpoint commits for progress saving
//!
//! Uses direct CLI commands (no libgit2) for simplicity and compatibility.

use color_eyre::eyre::{bail, Result, WrapErr};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Clone a GitHub repository to a local directory
///
/// Uses `gh repo clone` for authentication support (private repos).
///
/// # Arguments
/// * `repo` - Repository in format "owner/repo" or full URL
/// * `destination` - Local directory to clone into
///
/// # Returns
/// Path to the cloned repository
///
/// # Example
/// ```ignore
/// let path = clone_repo("anthropics/claude-code", "/tmp/claude-code")?;
/// ```
pub fn clone_repo(repo: &str, destination: &Path) -> Result<PathBuf> {
    // Normalize the repo string (remove URL prefix if present)
    let normalized = repo
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("git@")
        .replace("github.com:", "")
        .trim_start_matches("github.com/")
        .trim_end_matches(".git")
        .trim_matches('/')
        .to_string();

    if normalized.is_empty() {
        bail!("Invalid repository: {}", repo);
    }

    // Note: destination existence is already validated in spawn.rs before calling this

    // Create parent directory if needed
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)
            .wrap_err_with(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    tracing::info!(
        repo = %normalized,
        destination = %destination.display(),
        "Cloning GitHub repository..."
    );

    // Use gh CLI for clone (supports auth for private repos)
    let output = Command::new("gh")
        .args([
            "repo",
            "clone",
            &normalized,
            destination.to_str().unwrap_or("."),
        ])
        .output()
        .wrap_err("Failed to execute gh repo clone")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("gh repo clone failed: {}", stderr);
    }

    tracing::info!(
        repo = %normalized,
        path = %destination.display(),
        "Repository cloned successfully"
    );

    Ok(destination.to_path_buf())
}

/// Controller for git operations
pub struct GitController {
    /// Path to the main repository
    repo_path: PathBuf,
}

impl GitController {
    /// Create a new GitController for a repository
    pub fn new(repo_path: PathBuf) -> Self {
        Self { repo_path }
    }

    /// Check if the path is a git repository
    pub fn is_git_repo(&self) -> bool {
        self.repo_path.join(".git").exists()
    }

    /// Create a new worktree for isolated agent work
    ///
    /// # Arguments
    /// * `branch` - Name for the new branch (will be created)
    ///
    /// # Returns
    /// Path to the new worktree directory
    ///
    /// # Example
    /// ```ignore
    /// let git = GitController::new("/path/to/repo".into());
    /// let worktree_path = git.create_worktree("fix/auth-bug")?;
    /// // Worktree created at /path/to/repo-fix-auth-bug
    /// ```
    pub fn create_worktree(&self, branch: &str) -> Result<PathBuf> {
        if !self.is_git_repo() {
            bail!("Not a git repository: {}", self.repo_path.display());
        }

        // Sanitize branch name for directory
        let safe_branch = branch.replace(['/', '\\', ' '], "-");

        // Create worktree path: repo-branch
        let repo_name = self
            .repo_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repo");

        let worktree_name = format!("{repo_name}-{safe_branch}");
        let worktree_path = self
            .repo_path
            .parent()
            .unwrap_or(Path::new("/tmp"))
            .join(&worktree_name);

        // Check if worktree already exists
        if worktree_path.exists() {
            bail!("Worktree path already exists: {}", worktree_path.display());
        }

        // Create worktree with new branch
        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args([
                "worktree",
                "add",
                worktree_path.to_str().unwrap(),
                "-b",
                branch,
            ])
            .output()
            .wrap_err("Failed to execute git worktree add")?;

        if !output.status.success() {
            bail!(
                "git worktree add failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        tracing::info!(
            branch = %branch,
            path = %worktree_path.display(),
            "Created git worktree"
        );

        Ok(worktree_path)
    }

    /// Create a checkpoint commit
    ///
    /// Stages all changes and creates a commit. Useful for periodic auto-saves.
    ///
    /// # Arguments
    /// * `message` - Commit message
    pub fn checkpoint(&self, message: &str) -> Result<()> {
        if !self.is_git_repo() {
            bail!("Not a git repository: {}", self.repo_path.display());
        }

        // Stage all changes
        let add_output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["add", "-A"])
            .output()
            .wrap_err("Failed to execute git add")?;

        if !add_output.status.success() {
            bail!(
                "git add failed: {}",
                String::from_utf8_lossy(&add_output.stderr)
            );
        }

        // Check if there are changes to commit
        let status_output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["status", "--porcelain"])
            .output()
            .wrap_err("Failed to execute git status")?;

        if status_output.stdout.is_empty() {
            tracing::debug!("No changes to checkpoint");
            return Ok(());
        }

        // Create commit
        let commit_output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["commit", "-m", message])
            .output()
            .wrap_err("Failed to execute git commit")?;

        if !commit_output.status.success() {
            bail!(
                "git commit failed: {}",
                String::from_utf8_lossy(&commit_output.stderr)
            );
        }

        tracing::info!(message = %message, "Created checkpoint commit");
        Ok(())
    }

    /// Check if there are uncommitted changes
    pub fn has_changes(&self) -> Result<bool> {
        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["status", "--porcelain"])
            .output()
            .wrap_err("Failed to check git status")?;

        Ok(!output.stdout.is_empty())
    }

    /// Get full git diff output (with actual changes)
    ///
    /// Returns the complete diff showing line-by-line changes.
    pub fn diff_full(&self) -> Result<String> {
        if !self.is_git_repo() {
            bail!("Not a git repository: {}", self.repo_path.display());
        }

        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["diff", "--color=never"])
            .output()
            .wrap_err("Failed to execute git diff")?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Push current branch to remote
    ///
    /// Pushes the current branch to the default remote (usually origin).
    /// Will fail if the branch has no upstream configured.
    pub fn push(&self) -> Result<()> {
        if !self.is_git_repo() {
            bail!("Not a git repository: {}", self.repo_path.display());
        }

        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["push"])
            .output()
            .wrap_err("Failed to execute git push")?;

        if !output.status.success() {
            bail!(
                "git push failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        tracing::info!(repo = %self.repo_path.display(), "Pushed to remote");
        Ok(())
    }

    /// Get the current HEAD commit hash
    ///
    /// Returns the short commit hash (7 chars) of HEAD.
    pub fn head_commit(&self) -> Result<String> {
        if !self.is_git_repo() {
            bail!("Not a git repository: {}", self.repo_path.display());
        }

        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .wrap_err("Failed to execute git rev-parse")?;

        if !output.status.success() {
            bail!(
                "git rev-parse failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Get diff since a specific commit
    ///
    /// Returns the diff from the given commit to HEAD (working tree).
    /// Includes both staged and unstaged changes.
    ///
    /// # Arguments
    /// * `commit` - Commit hash to diff from
    pub fn diff_since(&self, commit: &str) -> Result<String> {
        if !self.is_git_repo() {
            bail!("Not a git repository: {}", self.repo_path.display());
        }

        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["diff", "--color=never", commit])
            .output()
            .wrap_err("Failed to execute git diff")?;

        if !output.status.success() {
            bail!(
                "git diff failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_repo() -> (TempDir, GitController) {
        let tmp = TempDir::new().unwrap();
        let repo_path = tmp.path().to_path_buf();

        // Initialize git repo
        Command::new("git")
            .current_dir(&repo_path)
            .args(["init"])
            .output()
            .unwrap();

        // Configure git for tests
        Command::new("git")
            .current_dir(&repo_path)
            .args(["config", "user.email", "test@test.com"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(&repo_path)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();

        // Create initial commit
        std::fs::write(repo_path.join("README.md"), "# Test").unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        Command::new("git")
            .current_dir(&repo_path)
            .args(["commit", "-m", "Initial commit"])
            .output()
            .unwrap();

        let git = GitController::new(repo_path);
        (tmp, git)
    }

    #[test]
    fn test_is_git_repo() {
        let (_tmp, git) = setup_test_repo();
        assert!(git.is_git_repo());
    }

    #[test]
    fn test_checkpoint() {
        let (tmp, git) = setup_test_repo();

        // Make a change
        std::fs::write(tmp.path().join("new_file.txt"), "test content").unwrap();

        // Verify there are changes
        assert!(git.has_changes().unwrap());

        // Create checkpoint
        git.checkpoint("Test checkpoint").unwrap();

        // No more changes
        assert!(!git.has_changes().unwrap());
    }

    #[test]
    fn test_checkpoint_no_changes() {
        let (_tmp, git) = setup_test_repo();

        // No changes to commit
        assert!(!git.has_changes().unwrap());

        // Checkpoint should succeed silently
        git.checkpoint("Empty checkpoint").unwrap();
    }
}
