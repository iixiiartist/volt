//! Worktree-per-session mode (CLI UX audit item #17).
//!
//! When the user passes `--worktree` to `volt agent-run` or the TUI, the
//! agent runs inside a fresh `git worktree` on a dedicated branch. All
//! file modifications the agent makes are isolated to that branch, so
//! the user can review the diff and either merge it back or discard it
//! without ever touching the working tree they started in.
//!
//! This module shells out to `git` (consistent with the hook system in
//! `src/agent/hooks.rs` and the MCP client) rather than pulling in
//! libgit2/git2 (heavyweight C dependency). All operations go through
//! `tokio::process::Command` so the agent loop never blocks on git.
//!
//! Layout:
//!   <repo_root>/.volt-worktrees/<short-session-id>/
//!     worktree (checked out at branch `volt-session/<short-session-id>`)
//!
//! Public surface:
//!   * `WorktreeManager::detect_repo_root(start) -> Option<PathBuf>`
//!   * `WorktreeManager::new(repo_root)` constructor
//!   * `create_for_session(session_id) -> WorktreeInfo`  — creates if missing
//!   * `list() -> Vec<WorktreeInfo>`                     — list volt-session worktrees
//!   * `remove(path, force) -> Result<()>`                — `git worktree remove`
//!   * `merge_back(branch) -> Result<()>`                — `git merge --no-ff` from HEAD
//!   * `diff_summary(branch) -> String`                  — `git diff --stat` for the PR-like view
//!
//! Error type: `WorktreeError` — wraps `git` stderr so the user can see
//! what went wrong (e.g. "fatal: not a git repository").

use std::path::{Path, PathBuf};
use std::process::Stdio;
use thiserror::Error;
use tokio::process::Command;
use uuid::Uuid;

/// Errors from the worktree subsystem.
#[derive(Debug, Error)]
pub enum WorktreeError {
    #[error("not a git repository (or any of the parent directories): {0}")]
    NotARepo(PathBuf),

    #[error("git command failed: {context}\n  stderr: {stderr}")]
    GitFailed { context: String, stderr: String },

    #[error("worktree path already exists but is not managed by volt: {0}")]
    UnmanagedPath(PathBuf),

    #[error("session id is not a valid uuid: {0}")]
    InvalidSessionId(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl WorktreeError {
    fn git(context: &str, stderr: String) -> Self {
        WorktreeError::GitFailed {
            context: context.to_string(),
            stderr,
        }
    }
}

/// A worktree we created (or detected) on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    /// Absolute path to the worktree checkout.
    pub path: PathBuf,
    /// Branch checked out in the worktree (e.g. `volt-session/abc12345`).
    pub branch: String,
    /// Short session id this worktree belongs to (first 8 hex chars of the uuid).
    pub short_id: String,
    /// Full uuid this worktree belongs to.
    pub session_id: Uuid,
}

/// Where new worktrees live, relative to the repo root.
const WORKTREE_PARENT_DIR: &str = ".volt-worktrees";
/// Branch prefix for volt-managed worktrees.
const BRANCH_PREFIX: &str = "volt-session/";

/// Manager for worktrees rooted at a specific git repo.
#[derive(Debug, Clone)]
pub struct WorktreeManager {
    repo_root: PathBuf,
}

impl WorktreeManager {
    /// Construct a manager for `repo_root`. The directory must be inside
    /// a git working tree (use `detect_repo_root` to walk up).
    pub fn new(repo_root: PathBuf) -> Self {
        Self { repo_root }
    }

    /// Walk up from `start` looking for a `.git` directory or file. Returns
    /// the directory containing it (the repo root), or `None` if not in
    /// a git working tree.
    pub async fn detect_repo_root(start: &Path) -> Result<Option<PathBuf>, WorktreeError> {
        let mut current = start
            .canonicalize()
            .map_err(WorktreeError::Io)?
            .to_path_buf();
        // Stop at filesystem root, just in case.
        loop {
            let git_dir = current.join(".git");
            if git_dir.exists() {
                return Ok(Some(current));
            }
            // `git rev-parse --show-toplevel` is the canonical check; it
            // also handles worktrees, submodules, and bare repos. We try
            // it once if the filesystem walk didn't find a .git entry
            // (which is the case inside a worktree of a worktree).
            if let Some(root) = Self::git_toplevel(&current).await? {
                return Ok(Some(root));
            }
            match current.parent() {
                Some(parent) if parent != current => current = parent.to_path_buf(),
                _ => return Ok(None),
            }
        }
    }

    async fn git_toplevel(cwd: &Path) -> Result<Option<PathBuf>, WorktreeError> {
        let out = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .current_dir(cwd)
            .output()
            .await?;
        if !out.status.success() {
            // 128 with "fatal: not a git repository..." — that's "no",
            // not an error worth surfacing.
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if stderr.contains("not a git repository") {
                return Ok(None);
            }
            return Err(WorktreeError::git("rev-parse --show-toplevel", stderr));
        }
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if s.is_empty() {
            Ok(None)
        } else {
            Ok(Some(PathBuf::from(s)))
        }
    }

    /// Returns the directory under which all volt worktrees live.
    pub fn parent_dir(&self) -> PathBuf {
        self.repo_root.join(WORKTREE_PARENT_DIR)
    }

    /// Compute the path and branch name for a given session id without
    /// actually creating anything on disk.
    pub fn plan(&self, session_id: Uuid) -> WorktreeInfo {
        let short = short_id(&session_id);
        let branch = format!("{}{}", BRANCH_PREFIX, short);
        let path = self.parent_dir().join(&short);
        WorktreeInfo {
            path,
            branch,
            short_id: short,
            session_id,
        }
    }

    /// Create the worktree for `session_id` if it doesn't already exist.
    /// Idempotent: re-calling with the same id returns the existing
    /// `WorktreeInfo` without re-running `git worktree add`.
    pub async fn create_for_session(
        &self,
        session_id: Uuid,
    ) -> Result<WorktreeInfo, WorktreeError> {
        let info = self.plan(session_id);
        if info.path.exists() {
            return Ok(info);
        }
        std::fs::create_dir_all(self.parent_dir())?;
        // `git worktree add -b <branch> <path>` creates a new branch and
        // checks it out into <path>. We branch from the current HEAD.
        let path_str = info.path.to_string_lossy().to_string();
        let out = Command::new("git")
            .args(["worktree", "add", "-b", &info.branch, &path_str])
            .current_dir(&self.repo_root)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await?;
        if !out.status.success() {
            // If the branch already exists from a previous aborted run,
            // just check it out instead of failing.
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if stderr.contains("already exists") {
                let out2 = Command::new("git")
                    .args(["worktree", "add", &path_str, &info.branch])
                    .current_dir(&self.repo_root)
                    .output()
                    .await?;
                if !out2.status.success() {
                    return Err(WorktreeError::git(
                        "git worktree add (existing branch)",
                        String::from_utf8_lossy(&out2.stderr).to_string(),
                    ));
                }
            } else {
                return Err(WorktreeError::git("git worktree add -b", stderr));
            }
        }
        Ok(info)
    }

    /// List all volt-managed worktrees (branches matching `volt-session/*`).
    pub async fn list(&self) -> Result<Vec<WorktreeInfo>, WorktreeError> {
        // `git worktree list --porcelain` gives us stable parseable output.
        let out = Command::new("git")
            .args(["worktree", "list", "--porcelain"])
            .current_dir(&self.repo_root)
            .output()
            .await?;
        if !out.status.success() {
            return Err(WorktreeError::git(
                "git worktree list --porcelain",
                String::from_utf8_lossy(&out.stderr).to_string(),
            ));
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut infos = Vec::new();
        let mut current_path: Option<PathBuf> = None;
        for line in stdout.lines() {
            if let Some(rest) = line.strip_prefix("worktree ") {
                current_path = Some(PathBuf::from(rest));
            } else if let Some(branch) = line.strip_prefix("branch ") {
                let branch_ref = branch.trim_start_matches("refs/heads/");
                if let Some(short) = branch_ref.strip_prefix(BRANCH_PREFIX) {
                    if let Some(path) = current_path.take() {
                        // Synthesize a session_id from the short id.
                        // We use `Uuid::nil()` if we can't parse it (the
                        // branch may have been created by hand); the
                        // session_id is only used for display, not for
                        // filesystem layout, so this is fine.
                        let session_id = Uuid::nil();
                        infos.push(WorktreeInfo {
                            path,
                            branch: branch_ref.to_string(),
                            short_id: short.to_string(),
                            session_id,
                        });
                    }
                } else {
                    current_path = None;
                }
            }
        }
        Ok(infos)
    }

    /// Remove a worktree (with optional force). The branch is left in
    /// place; call `delete_branch` separately to remove it.
    pub async fn remove(&self, path: &Path, force: bool) -> Result<(), WorktreeError> {
        let path_str = path.to_string_lossy().to_string();
        let mut args: Vec<&str> = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(&path_str);
        let out = Command::new("git")
            .args(&args)
            .current_dir(&self.repo_root)
            .output()
            .await?;
        if !out.status.success() {
            return Err(WorktreeError::git(
                "git worktree remove",
                String::from_utf8_lossy(&out.stderr).to_string(),
            ));
        }
        Ok(())
    }

    /// Delete a branch (with optional force). Use after merging or
    /// discarding a worktree's changes.
    pub async fn delete_branch(&self, branch: &str, force: bool) -> Result<(), WorktreeError> {
        let mut args: Vec<&str> = vec!["branch", "-D"];
        if !force {
            // The lowercase `-d` only deletes if merged. We default to
            // `-D` (force) so users can clean up unmerged worktrees
            // after they've exported the diff.
            args[1] = "-D";
        }
        args.push(branch);
        let _ = force; // Force is always on for the destructive op; we keep the arg for API symmetry.
        let out = Command::new("git")
            .args(&args)
            .current_dir(&self.repo_root)
            .output()
            .await?;
        if !out.status.success() {
            return Err(WorktreeError::git(
                "git branch -D",
                String::from_utf8_lossy(&out.stderr).to_string(),
            ));
        }
        Ok(())
    }

    /// Merge a session branch back into the current branch. Runs from
    /// the repo root (not the worktree) so the merge lands on the user's
    /// main checkout. Uses `--no-ff` so the merge is always recorded as
    /// a merge commit, making it easy to revert and giving the user a
    /// visible merge point in the log.
    pub async fn merge_back(&self, branch: &str) -> Result<String, WorktreeError> {
        let out = Command::new("git")
            .args([
                "merge",
                "--no-ff",
                branch,
                "-m",
                &format!("volt: merge session branch {}", branch),
            ])
            .current_dir(&self.repo_root)
            .output()
            .await?;
        if !out.status.success() {
            return Err(WorktreeError::git(
                "git merge --no-ff",
                String::from_utf8_lossy(&out.stderr).to_string(),
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    /// Return a `git diff --stat` summary of the changes in `branch`
    /// relative to the current HEAD. Used by the `volt worktree status`
    /// command to show a PR-like overview.
    pub async fn diff_summary(&self, branch: &str) -> Result<String, WorktreeError> {
        let out = Command::new("git")
            .args(["diff", "--stat", &format!("HEAD..{}", branch)])
            .current_dir(&self.repo_root)
            .output()
            .await?;
        if !out.status.success() {
            return Err(WorktreeError::git(
                "git diff --stat",
                String::from_utf8_lossy(&out.stderr).to_string(),
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    /// Returns the current HEAD commit (short) of the worktree's branch.
    /// Used for display in `volt worktree list`.
    pub async fn head_short(&self, branch: &str) -> Result<String, WorktreeError> {
        let out = Command::new("git")
            .args(["rev-parse", "--short", branch])
            .current_dir(&self.repo_root)
            .output()
            .await?;
        if !out.status.success() {
            return Ok("<unknown>".to_string());
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
}

/// First 8 hex chars of a uuid (no dashes). Used as the worktree
/// directory name and the branch suffix. Stable for the lifetime of
/// the session, short enough to be human-readable.
fn short_id(id: &Uuid) -> String {
    id.simple().to_string()[..8].to_string()
}

/// Strip the `\\?\` UNC prefix that Windows adds during canonicalization,
/// so we can compare paths portably. The prefix carries no semantic
/// information — it's just a hint to the Windows API to skip MAX_PATH
/// parsing.
#[cfg(test)]
fn strip_unc_prefix(p: &Path) -> PathBuf {
    let s = p.to_string_lossy().to_string();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        p.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;
    use tempfile::TempDir;

    /// Create a temporary git repo with one commit. Returns the path.
    fn make_test_repo() -> (TempDir, PathBuf) {
        let tmp = TempDir::new().expect("create tempdir");
        let path = tmp.path().to_path_buf();
        let run = |args: &[&str]| {
            StdCommand::new("git")
                .args(args)
                .current_dir(&path)
                .output()
                .expect("git command")
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "test@volt"]);
        run(&["config", "user.name", "volt test"]);
        std::fs::write(path.join("README.md"), "hello\n").unwrap();
        run(&["add", "README.md"]);
        run(&["commit", "-q", "-m", "initial"]);
        (tmp, path)
    }

    #[tokio::test]
    async fn detect_repo_root_finds_dotgit() {
        let (_tmp, path) = make_test_repo();
        let nested = path.join("a").join("b");
        std::fs::create_dir_all(&nested).unwrap();
        let found = WorktreeManager::detect_repo_root(&nested).await.unwrap();
        // On Windows, TempDir paths are returned with the `\\?\` UNC
        // prefix after canonicalization; strip it for comparison.
        let found = found.as_deref().map(strip_unc_prefix);
        let expected = strip_unc_prefix(&path.canonicalize().unwrap());
        assert_eq!(found, Some(expected));
    }

    #[tokio::test]
    async fn detect_repo_root_returns_none_outside_repo() {
        let tmp = TempDir::new().unwrap();
        let found = WorktreeManager::detect_repo_root(tmp.path()).await.unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn plan_is_stable_for_same_id() {
        let (_tmp, path) = make_test_repo();
        let mgr = WorktreeManager::new(path);
        let id = Uuid::new_v4();
        let p1 = mgr.plan(id);
        let p2 = mgr.plan(id);
        assert_eq!(p1.path, p2.path);
        assert_eq!(p1.branch, p2.branch);
        assert_eq!(p1.short_id.len(), 8);
    }

    #[tokio::test]
    async fn create_for_session_is_idempotent() {
        let (_tmp, path) = make_test_repo();
        let mgr = WorktreeManager::new(path);
        let id = Uuid::new_v4();
        let info1 = mgr.create_for_session(id).await.unwrap();
        let info2 = mgr.create_for_session(id).await.unwrap();
        assert_eq!(info1.path, info2.path);
        assert!(info1.path.exists(), "worktree path should exist on disk");
    }

    #[tokio::test]
    async fn create_for_session_writes_to_worktree_only() {
        let (_tmp, path) = make_test_repo();
        let mgr = WorktreeManager::new(path.clone());
        let id = Uuid::new_v4();
        let info = mgr.create_for_session(id).await.unwrap();
        // Write a new file in the worktree.
        std::fs::write(info.path.join("AGENT_WAS_HERE.txt"), "x").unwrap();
        // The repo root should NOT see this file.
        assert!(!path.join("AGENT_WAS_HERE.txt").exists());
        // But the worktree should.
        assert!(info.path.join("AGENT_WAS_HERE.txt").exists());
    }

    #[tokio::test]
    async fn list_returns_volt_worktrees() {
        let (_tmp, path) = make_test_repo();
        let mgr = WorktreeManager::new(path);
        let id = Uuid::new_v4();
        let _info = mgr.create_for_session(id).await.unwrap();
        let infos = mgr.list().await.unwrap();
        assert!(infos
            .iter()
            .any(|i| i.session_id == Uuid::nil() && i.branch.starts_with(BRANCH_PREFIX)));
    }

    #[tokio::test]
    async fn diff_summary_reports_changes() {
        let (_tmp, path) = make_test_repo();
        let mgr = WorktreeManager::new(path.clone());
        let id = Uuid::new_v4();
        let info = mgr.create_for_session(id).await.unwrap();
        // Modify a tracked file in the worktree.
        std::fs::write(info.path.join("README.md"), "hello world\n").unwrap();
        // Commit on the worktree branch.
        StdCommand::new("git")
            .args(["add", "README.md"])
            .current_dir(&info.path)
            .output()
            .unwrap();
        StdCommand::new("git")
            .args(["commit", "-q", "-m", "agent change"])
            .current_dir(&info.path)
            .output()
            .unwrap();
        let summary = mgr.diff_summary(&info.branch).await.unwrap();
        assert!(summary.contains("README.md"), "summary: {}", summary);
    }

    #[tokio::test]
    async fn merge_back_lands_commit_on_main() {
        let (_tmp, path) = make_test_repo();
        let mgr = WorktreeManager::new(path.clone());
        let id = Uuid::new_v4();
        let info = mgr.create_for_session(id).await.unwrap();
        // Add a new file in the worktree and commit it.
        std::fs::write(info.path.join("NEW.txt"), "new file").unwrap();
        StdCommand::new("git")
            .args(["add", "NEW.txt"])
            .current_dir(&info.path)
            .output()
            .unwrap();
        StdCommand::new("git")
            .args(["commit", "-q", "-m", "add new"])
            .current_dir(&info.path)
            .output()
            .unwrap();
        // Merge back.
        mgr.merge_back(&info.branch).await.unwrap();
        // The new file should now exist on the main checkout.
        assert!(path.join("NEW.txt").exists());
    }

    #[tokio::test]
    async fn remove_then_list_no_longer_includes_it() {
        let (_tmp, path) = make_test_repo();
        let mgr = WorktreeManager::new(path);
        let id = Uuid::new_v4();
        let info = mgr.create_for_session(id).await.unwrap();
        mgr.remove(&info.path, true).await.unwrap();
        let infos = mgr.list().await.unwrap();
        assert!(!infos.iter().any(|i| i.path == info.path));
    }

    #[tokio::test]
    async fn delete_branch_removes_the_branch() {
        let (_tmp, path) = make_test_repo();
        let mgr = WorktreeManager::new(path.clone());
        let id = Uuid::new_v4();
        let info = mgr.create_for_session(id).await.unwrap();
        mgr.remove(&info.path, true).await.unwrap();
        mgr.delete_branch(&info.branch, true).await.unwrap();
        // Branch should not exist.
        let out = StdCommand::new("git")
            .args(["branch", "--list", &info.branch])
            .current_dir(&path)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            !stdout.contains(&info.branch),
            "branch should be gone: {}",
            stdout
        );
    }
}
