//! Integration tests for the worktree-per-session CLI (item #17).
//!
//! These tests exercise the worktree subsystem end-to-end against real
//! `git` invocations. They are hermetic: each test creates a throwaway
//! tempdir, runs `git init` + a commit, then verifies the public
//! `WorktreeManager` API against that repo.

use std::path::PathBuf;
use std::process::Command as StdCommand;
use tempfile::TempDir;
use volt::commands::worktree::WorktreeManager;

fn make_repo(label: &str) -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("tempdir");
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
    eprintln!("[{}] test repo at: {}", label, path.display());
    (tmp, path)
}

#[tokio::test]
async fn full_lifecycle_create_modify_merge_clean() {
    let (_tmp, repo) = make_repo("lifecycle");
    let mgr = WorktreeManager::new(repo.clone());
    let id = uuid::Uuid::new_v4();
    let info = mgr.create_for_session(id).await.expect("create worktree");

    // 1) Modify a tracked file in the worktree and commit.
    // On Windows, `git commit` normalises line endings to CRLF in the
    // index, so the merged content has CRLF. Use CRLF in the input too
    // so the assertion is portable.
    let new_content = if cfg!(windows) {
        "hello world\r\n"
    } else {
        "hello world\n"
    };
    std::fs::write(info.path.join("README.md"), new_content).unwrap();
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

    // 2) Status shows the change.
    let summary = mgr.diff_summary(&info.branch).await.expect("diff summary");
    assert!(summary.contains("README.md"), "summary: {}", summary);

    // 3) List shows our worktree.
    let list = mgr.list().await.expect("list");
    assert!(
        list.iter().any(|i| i.branch == info.branch),
        "list: {:?}",
        list
    );

    // 4) Merge back to main.
    mgr.merge_back(&info.branch).await.expect("merge");
    assert!(repo.join("README.md").exists());
    let merged_content = std::fs::read_to_string(repo.join("README.md")).unwrap();
    assert_eq!(merged_content, new_content);

    // 5) Clean up.
    mgr.remove(&info.path, true).await.expect("remove worktree");
    mgr.delete_branch(&info.branch, true)
        .await
        .expect("delete branch");
    let list_after = mgr.list().await.expect("list after");
    assert!(
        !list_after.iter().any(|i| i.branch == info.branch),
        "branch should be gone: {:?}",
        list_after
    );
}

#[tokio::test]
async fn two_concurrent_sessions_get_distinct_worktrees() {
    let (_tmp, repo) = make_repo("concurrent");
    let mgr = WorktreeManager::new(repo.clone());
    let id_a = uuid::Uuid::new_v4();
    let id_b = uuid::Uuid::new_v4();
    let a = mgr.create_for_session(id_a).await.expect("create a");
    let b = mgr.create_for_session(id_b).await.expect("create b");
    assert_ne!(a.path, b.path);
    assert_ne!(a.branch, b.branch);
    // Write to A only; B should not see it.
    std::fs::write(a.path.join("A.txt"), "from a").unwrap();
    assert!(!b.path.join("A.txt").exists());
}

#[tokio::test]
async fn create_for_session_is_idempotent_on_repeat() {
    let (_tmp, repo) = make_repo("idempotent");
    let mgr = WorktreeManager::new(repo);
    let id = uuid::Uuid::new_v4();
    let i1 = mgr.create_for_session(id).await.unwrap();
    let i2 = mgr.create_for_session(id).await.unwrap();
    assert_eq!(i1.path, i2.path);
    assert_eq!(i1.branch, i2.branch);
}

#[tokio::test]
async fn short_id_is_eight_chars() {
    let (_tmp, repo) = make_repo("short");
    let mgr = WorktreeManager::new(repo);
    let id = uuid::Uuid::new_v4();
    let info = mgr.plan(id);
    assert_eq!(info.short_id.len(), 8);
    // It should be hex (the first 8 chars of a v4 uuid are).
    assert!(info.short_id.chars().all(|c| c.is_ascii_hexdigit()));
}

#[tokio::test]
async fn parent_dir_is_dot_volt_worktrees() {
    let (_tmp, repo) = make_repo("parent");
    let mgr = WorktreeManager::new(repo.clone());
    let parent = mgr.parent_dir();
    assert!(
        parent
            .file_name()
            .map(|n| n == ".volt-worktrees")
            .unwrap_or(false),
        "expected .volt-worktrees, got: {}",
        parent.display()
    );
    let parent_of_parent = parent.parent().expect("parent of parent dir");
    let canonical_repo = repo.canonicalize().unwrap_or(repo);
    // Strip the `\\?\` UNC prefix on Windows for comparison.
    let parent_str = parent_of_parent.to_string_lossy().to_string();
    let canonical_str = canonical_repo.to_string_lossy().to_string();
    let parent_normalized = parent_str.strip_prefix(r"\\?\").unwrap_or(&parent_str);
    let canonical_normalized = canonical_str
        .strip_prefix(r"\\?\")
        .unwrap_or(&canonical_str);
    assert_eq!(parent_normalized, canonical_normalized);
}

#[tokio::test]
async fn plan_does_not_touch_disk() {
    let (_tmp, repo) = make_repo("plan");
    let mgr = WorktreeManager::new(repo.clone());
    let before = std::fs::read_dir(&repo).unwrap().count();
    let _info = mgr.plan(uuid::Uuid::new_v4());
    let after = std::fs::read_dir(&repo).unwrap().count();
    assert_eq!(before, after, "plan() should not create anything");
}
