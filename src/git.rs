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

/// Controller for git operations
#[allow(dead_code)]
pub struct GitController {
    /// Path to the main repository
    repo_path: PathBuf,
}

/// Information about a git worktree
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct WorktreeInfo {
    /// Path to the worktree directory
    pub path: PathBuf,
    /// Branch name
    pub branch: Option<String>,
    /// HEAD commit SHA
    pub head: Option<String>,
    /// Whether this is the main worktree
    pub is_main: bool,
}

#[allow(dead_code)]
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

    /// List all worktrees for this repository
    pub fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>> {
        if !self.is_git_repo() {
            bail!("Not a git repository: {}", self.repo_path.display());
        }

        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["worktree", "list", "--porcelain"])
            .output()
            .wrap_err("Failed to execute git worktree list")?;

        if !output.status.success() {
            bail!(
                "git worktree list failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut worktrees = Vec::new();
        let mut current: Option<WorktreeInfo> = None;

        for line in stdout.lines() {
            if line.starts_with("worktree ") {
                // Save previous worktree if exists
                if let Some(wt) = current.take() {
                    worktrees.push(wt);
                }
                // Start new worktree
                let path = line.strip_prefix("worktree ").unwrap();
                current = Some(WorktreeInfo {
                    path: PathBuf::from(path),
                    branch: None,
                    head: None,
                    is_main: false,
                });
            } else if let Some(ref mut wt) = current {
                if line.starts_with("HEAD ") {
                    wt.head = Some(line.strip_prefix("HEAD ").unwrap().to_string());
                } else if line.starts_with("branch ") {
                    let branch = line
                        .strip_prefix("branch refs/heads/")
                        .unwrap_or(line.strip_prefix("branch ").unwrap_or(line));
                    wt.branch = Some(branch.to_string());
                } else if line == "bare" {
                    wt.is_main = true;
                }
            }
        }

        // Don't forget the last one
        if let Some(wt) = current {
            worktrees.push(wt);
        }

        // Mark the first worktree as main (it's always the primary one)
        if let Some(first) = worktrees.first_mut() {
            first.is_main = true;
        }

        Ok(worktrees)
    }

    /// Remove a worktree
    ///
    /// # Arguments
    /// * `path` - Path to the worktree to remove
    /// * `force` - Force removal even if dirty
    pub fn remove_worktree(&self, path: &Path, force: bool) -> Result<()> {
        if !self.is_git_repo() {
            bail!("Not a git repository: {}", self.repo_path.display());
        }

        let mut args = vec!["worktree", "remove", path.to_str().unwrap()];
        if force {
            args.push("--force");
        }

        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(&args)
            .output()
            .wrap_err("Failed to execute git worktree remove")?;

        if !output.status.success() {
            bail!(
                "git worktree remove failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        tracing::info!(path = %path.display(), "Removed git worktree");
        Ok(())
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

    /// Get the current branch name
    pub fn current_branch(&self) -> Result<String> {
        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .output()
            .wrap_err("Failed to get current branch")?;

        if !output.status.success() {
            bail!(
                "git rev-parse failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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

    /// Get git diff output for uncommitted changes
    ///
    /// Returns the diff with file stats (insertions/deletions per file).
    /// If there are no changes, returns an empty string.
    pub fn diff(&self) -> Result<String> {
        if !self.is_git_repo() {
            bail!("Not a git repository: {}", self.repo_path.display());
        }

        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["diff", "--stat", "--color=never"])
            .output()
            .wrap_err("Failed to execute git diff")?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
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

    /// Delete a local branch
    ///
    /// Use this after removing a worktree to clean up the associated branch.
    /// Uses -D flag to force deletion even if not fully merged.
    ///
    /// # Arguments
    /// * `branch` - Name of the branch to delete
    pub fn delete_branch(&self, branch: &str) -> Result<()> {
        if !self.is_git_repo() {
            bail!("Not a git repository: {}", self.repo_path.display());
        }

        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["branch", "-D", branch])
            .output()
            .wrap_err("Failed to execute git branch -D")?;

        if !output.status.success() {
            bail!(
                "git branch -D failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        tracing::info!(branch = %branch, "Deleted branch");
        Ok(())
    }

    /// Prune stale worktree references
    ///
    /// Cleans up administrative files for worktrees that no longer exist
    /// on the filesystem. Safe to run periodically.
    pub fn prune_worktrees(&self) -> Result<()> {
        if !self.is_git_repo() {
            bail!("Not a git repository: {}", self.repo_path.display());
        }

        let output = Command::new("git")
            .current_dir(&self.repo_path)
            .args(["worktree", "prune"])
            .output()
            .wrap_err("Failed to execute git worktree prune")?;

        if !output.status.success() {
            bail!(
                "git worktree prune failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        tracing::debug!("Pruned stale worktree references");
        Ok(())
    }

    /// Full cleanup: remove worktree, delete branch, and prune
    ///
    /// Complete cleanup of an agent's isolated workspace:
    /// 1. Force-remove the worktree directory
    /// 2. Delete the associated branch
    /// 3. Prune stale worktree references
    ///
    /// # Arguments
    /// * `path` - Path to the worktree to remove
    /// * `branch` - Name of the branch to delete
    pub fn cleanup_worktree(&self, path: &Path, branch: &str) -> Result<()> {
        // Force remove worktree (even if dirty)
        self.remove_worktree(path, true)?;

        // Delete the branch
        self.delete_branch(branch)?;

        // Prune any stale references
        self.prune_worktrees()?;

        tracing::info!(
            path = %path.display(),
            branch = %branch,
            "Full worktree cleanup complete"
        );
        Ok(())
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
    fn test_current_branch() {
        let (_tmp, git) = setup_test_repo();
        let branch = git.current_branch().unwrap();
        // Could be "main" or "master" depending on git config
        assert!(branch == "main" || branch == "master");
    }

    #[test]
    fn test_list_worktrees() {
        let (_tmp, git) = setup_test_repo();
        let worktrees = git.list_worktrees().unwrap();
        assert_eq!(worktrees.len(), 1);
        assert!(worktrees[0].is_main);
    }

    #[test]
    fn test_create_and_remove_worktree() {
        let (_tmp, git) = setup_test_repo();

        // Create worktree
        let worktree_path = git.create_worktree("test-branch").unwrap();
        assert!(worktree_path.exists());

        // List should show 2 worktrees now
        let worktrees = git.list_worktrees().unwrap();
        assert_eq!(worktrees.len(), 2);

        // Remove worktree
        git.remove_worktree(&worktree_path, false).unwrap();
        assert!(!worktree_path.exists());

        // Back to 1 worktree
        let worktrees = git.list_worktrees().unwrap();
        assert_eq!(worktrees.len(), 1);
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

    #[test]
    fn test_delete_branch() {
        let (_tmp, git) = setup_test_repo();

        // Create a new branch
        Command::new("git")
            .current_dir(&git.repo_path)
            .args(["branch", "test-delete-branch"])
            .output()
            .unwrap();

        // Delete the branch
        git.delete_branch("test-delete-branch").unwrap();

        // Verify branch is gone
        let output = Command::new("git")
            .current_dir(&git.repo_path)
            .args(["branch", "--list", "test-delete-branch"])
            .output()
            .unwrap();
        assert!(output.stdout.is_empty());
    }

    #[test]
    fn test_prune_worktrees() {
        let (_tmp, git) = setup_test_repo();

        // Prune should succeed even with nothing to prune
        git.prune_worktrees().unwrap();
    }

    #[test]
    fn test_cleanup_worktree() {
        let (_tmp, git) = setup_test_repo();

        // Create worktree with a branch
        let branch = "cleanup-test-branch";
        let worktree_path = git.create_worktree(branch).unwrap();
        assert!(worktree_path.exists());

        // Verify worktree and branch exist
        let worktrees = git.list_worktrees().unwrap();
        assert_eq!(worktrees.len(), 2);

        // Full cleanup
        git.cleanup_worktree(&worktree_path, branch).unwrap();

        // Verify worktree is gone
        assert!(!worktree_path.exists());

        // Verify back to 1 worktree
        let worktrees = git.list_worktrees().unwrap();
        assert_eq!(worktrees.len(), 1);

        // Verify branch is deleted
        let output = Command::new("git")
            .current_dir(&git.repo_path)
            .args(["branch", "--list", branch])
            .output()
            .unwrap();
        assert!(output.stdout.is_empty());
    }
}
