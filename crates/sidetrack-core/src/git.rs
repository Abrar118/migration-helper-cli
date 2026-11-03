//! Thin wrapper over the `git` CLI. Using the CLI keeps behaviour identical to the
//! user's own git (config, hooks path, worktrees).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use crate::process::{capture, capture_opt};

#[derive(Debug, Clone)]
pub struct Git {
    root: PathBuf,
}

impl Git {
    /// Find the repository containing `dir`.
    pub fn discover(dir: &Path) -> Result<Self> {
        let out = capture(Command::new("git").arg("-C").arg(dir).args(["rev-parse", "--show-toplevel"]))
            .context("not inside a git repository")?;
        Ok(Self { root: PathBuf::from(out.trim()) })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn cmd(&self) -> Command {
        let mut c = Command::new("git");
        c.current_dir(&self.root);
        c
    }

    fn run(&self, args: &[&str]) -> Result<String> {
        Ok(capture(self.cmd().args(args))?.trim().to_string())
    }

    fn run_opt(&self, args: &[&str]) -> Result<Option<String>> {
        Ok(capture_opt(self.cmd().args(args))?.map(|s| s.trim().to_string()))
    }

    /// Current branch, or `None` for a detached HEAD.
    pub fn current_branch(&self) -> Result<Option<String>> {
        self.run_opt(&["symbolic-ref", "--short", "-q", "HEAD"])
    }

    pub fn head_commit(&self) -> Result<String> {
        self.run(&["rev-parse", "HEAD"])
    }

    /// Full commit id for a revision, or `None` if it doesn't exist.
    pub fn resolve(&self, rev: &str) -> Result<Option<String>> {
        self.run_opt(&["rev-parse", "--verify", "-q", &format!("{rev}^{{commit}}")])
    }

    pub fn merge_base(&self, a: &str, b: &str) -> Result<Option<String>> {
        self.run_opt(&["merge-base", a, b])
    }

    pub fn branch_exists(&self, name: &str) -> Result<bool> {
        Ok(self.run_opt(&["show-ref", "--verify", "-q", &format!("refs/heads/{name}")])?.is_some())
    }

    pub fn local_branches(&self) -> Result<Vec<String>> {
        let out = self.run(&["for-each-ref", "--format=%(refname:short)", "refs/heads/"])?;
        Ok(out.lines().map(str::to_string).filter(|l| !l.is_empty()).collect())
    }

    /// Local branches fully merged into `target`.
    pub fn merged_into(&self, target: &str) -> Result<Vec<String>> {
        let out = self.run(&["branch", "--format=%(refname:short)", "--merged", target])?;
        Ok(out.lines().map(str::to_string).filter(|l| !l.is_empty() && l != target).collect())
    }

    /// Names of the entries directly inside `dir` at `rev` (relative to `dir`).
    pub fn tree_entries(&self, rev: &str, dir: &str) -> Result<BTreeSet<String>> {
        let dir = dir.trim_end_matches('/');
        let out = self.run(&["ls-tree", "--name-only", rev, "--", &format!("{dir}/")])?;
        Ok(out
            .lines()
            .filter_map(|l| l.strip_prefix(dir).map(|r| r.trim_start_matches('/').to_string()))
            .filter(|n| !n.is_empty())
            .collect())
    }

    pub fn is_tracked(&self, path: &str) -> Result<bool> {
        Ok(self.run_opt(&["ls-files", "--error-unmatch", "--", path])?.is_some())
    }

    /// Absolute path of a file inside the git dir (`rev-parse --git-path`).
    pub fn git_path(&self, name: &str) -> Result<PathBuf> {
        let p = PathBuf::from(self.run(&["rev-parse", "--git-path", name])?);
        Ok(if p.is_absolute() { p } else { self.root.join(p) })
    }

    pub fn hooks_dir(&self) -> Result<PathBuf> {
        self.git_path("hooks")
    }

    /// A rebase, merge, cherry-pick, revert or bisect in progress, if any.
    pub fn operation_in_progress(&self) -> Result<Option<&'static str>> {
        let checks = [
            ("rebase-merge", "rebase"),
            ("rebase-apply", "rebase"),
            ("MERGE_HEAD", "merge"),
            ("CHERRY_PICK_HEAD", "cherry-pick"),
            ("REVERT_HEAD", "revert"),
            ("BISECT_LOG", "bisect"),
        ];
        for (file, op) in checks {
            if self.git_path(file)?.exists() {
                return Ok(Some(op));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    /// A throwaway repo with one empty commit on `main`.
    pub fn repo() -> (tempfile::TempDir, Git) {
        let dir = tempfile::tempdir().unwrap();
        let g = |args: &[&str]| {
            let ok = Command::new("git").current_dir(dir.path()).args(args).output().unwrap();
            assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
        };
        g(&["init", "-q", "-b", "main"]);
        g(&["config", "user.email", "t@example.com"]);
        g(&["config", "user.name", "t"]);
        g(&["config", "commit.gpgsign", "false"]);
        g(&["commit", "-q", "--allow-empty", "-m", "init"]);
        let git = Git::discover(dir.path()).unwrap();
        (dir, git)
    }

    pub fn git(repo: &Git, args: &[&str]) -> String {
        capture(Command::new("git").current_dir(repo.root()).args(args)).unwrap().trim().to_string()
    }

    /// Write a file and commit it.
    pub fn commit_file(repo: &Git, path: &str, msg: &str) {
        let full = repo.root().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, msg).unwrap();
        git(repo, &["add", path]);
        git(repo, &["commit", "-q", "-m", msg]);
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;

    #[test]
    fn branches_and_trees() {
        let (_d, repo) = repo();
        assert_eq!(repo.current_branch().unwrap().as_deref(), Some("main"));
        commit_file(&repo, "migrations/001_a.sql", "a");
        git(&repo, &["checkout", "-q", "-b", "feature/x"]);
        commit_file(&repo, "migrations/002_b.sql", "b");

        let names: Vec<_> = repo.tree_entries("HEAD", "migrations").unwrap().into_iter().collect();
        assert_eq!(names, ["001_a.sql", "002_b.sql"]);
        assert_eq!(repo.tree_entries("main", "migrations/").unwrap().len(), 1);
        assert!(repo.tree_entries("main", "nope").unwrap().is_empty());

        assert!(repo.branch_exists("feature/x").unwrap());
        assert!(!repo.branch_exists("feature/y").unwrap());
        let main = repo.resolve("main").unwrap().unwrap();
        assert_eq!(repo.merge_base("main", "feature/x").unwrap().unwrap(), main);
        assert!(repo.merged_into("feature/x").unwrap().contains(&"main".to_string()));
        assert!(repo.operation_in_progress().unwrap().is_none());

        git(&repo, &["checkout", "-q", "--detach"]);
        assert_eq!(repo.current_branch().unwrap(), None);
    }
}
