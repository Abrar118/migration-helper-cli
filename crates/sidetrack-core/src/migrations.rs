//! Migration file sets and base selection (spec P0.6).
//!
//! sidetrack never parses migrations. It only compares *which* migration files
//! exist, read from git, so it works with any tool that keeps one file or folder
//! per migration (Alembic, Django, Flyway, Prisma, Rails, Knex, golang-migrate, …).

use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::git::Git;
use crate::registry::FileSet;

/// Entries that live in migration folders but aren't migrations.
pub fn is_migration_entry(name: &str) -> bool {
    !(name.starts_with('.')
        || name == "__pycache__"
        || name == "__init__.py"
        || name.ends_with(".pyc")
        || name == "migration_lock.toml")
}

fn key(dir: &str, name: &str) -> String {
    format!("{}/{}", dir.trim_end_matches('/'), name)
}

/// Migration files committed at `rev`, as `dir/name`.
pub fn files_at(git: &Git, rev: &str, dirs: &[String]) -> Result<FileSet> {
    let mut out = FileSet::new();
    for dir in dirs {
        for name in git.tree_entries(rev, dir)? {
            if is_migration_entry(&name) {
                out.insert(key(dir, &name));
            }
        }
    }
    Ok(out)
}

/// Migration files on disk right now, including uncommitted ones.
pub fn files_in_worktree(root: &Path, dirs: &[String]) -> Result<FileSet> {
    let mut out = FileSet::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if is_migration_entry(&name) {
                out.insert(key(dir, &name));
            }
        }
    }
    Ok(out)
}

/// Files for the checked-out branch: committed at HEAD plus anything new on disk.
pub fn current_files(git: &Git, dirs: &[String]) -> Result<FileSet> {
    let mut set = files_at(git, "HEAD", dirs)?;
    set.extend(files_in_worktree(git.root(), dirs)?);
    Ok(set)
}

/// Something a new DB branch could be copied from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub db_name: String,
    pub kind: CandidateKind,
    /// The git branch it belongs to.
    pub git_branch: String,
    /// Migration files it *may* contain (recorded plus anything its git branch now
    /// has). Used to decide compatibility: over-counting is the safe direction.
    pub files: FileSet,
    /// Migration files it is *known* to contain. Used to work out what still has to
    /// run: under-counting is the safe direction (the tool skips what's applied).
    pub applied: FileSet,
    /// Tie-breaker: newer wins.
    pub recency: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKind {
    Branch,
    Snapshot,
}

/// Pick the best base for a branch whose migration files are `target`.
///
/// A candidate is compatible when it holds no migration the target doesn't
/// know (its files are a subset of the target's). Among those, the one with the
/// most files wins — it needs the fewest migrations run forward.
pub fn select_base<'a>(target: &FileSet, candidates: &'a [Candidate]) -> Option<&'a Candidate> {
    candidates
        .iter()
        .filter(|c| c.files.is_subset(target))
        .max_by_key(|c| (c.files.len(), c.recency))
}

/// Migrations the target has that the database doesn't, in name order.
pub fn pending(target: &FileSet, applied: &FileSet) -> Vec<String> {
    target.difference(applied).cloned().collect()
}

/// Migrations the database has that the target doesn't know about.
pub fn unknown(target: &FileSet, applied: &FileSet) -> Vec<String> {
    applied.difference(target).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testutil::*;

    fn set(names: &[&str]) -> FileSet {
        names.iter().map(|s| s.to_string()).collect()
    }

    fn cand(db: &str, files: &[&str], recency: i64) -> Candidate {
        Candidate {
            db_name: db.into(),
            kind: CandidateKind::Branch,
            git_branch: db.trim_end_matches("_db").into(),
            files: set(files),
            applied: set(files),
            recency,
        }
    }

    #[test]
    fn picks_largest_compatible() {
        let target = set(&["m/1", "m/2", "m/3"]);
        let cands = [
            cand("main_db", &["m/1"], 5),
            cand("dev_db", &["m/1", "m/2"], 1),
            cand("other_db", &["m/1", "m/2", "m/X"], 9), // has a migration target doesn't know
        ];
        assert_eq!(select_base(&target, &cands).unwrap().db_name, "dev_db");
    }

    #[test]
    fn ties_go_to_most_recent() {
        let target = set(&["m/1"]);
        let cands = [cand("a_db", &["m/1"], 1), cand("b_db", &["m/1"], 2)];
        assert_eq!(select_base(&target, &cands).unwrap().db_name, "b_db");
    }

    #[test]
    fn nothing_compatible() {
        assert!(select_base(&set(&["m/1"]), &[cand("x_db", &["m/2"], 0)]).is_none());
        assert_eq!(pending(&set(&["a", "b"]), &set(&["a"])), ["b"]);
        assert_eq!(unknown(&set(&["a"]), &set(&["a", "z"])), ["z"]);
    }

    /// Spec acceptance: a branch forked from dev *before* dev gained migration X
    /// must never get a base that contains X.
    #[test]
    fn dev_moved_on_after_fork() {
        let (_d, repo) = repo();
        let dirs = vec!["migrations".to_string()];
        git(&repo, &["checkout", "-q", "-b", "dev"]);
        commit_file(&repo, "migrations/001_users.sql", "1");
        git(&repo, &["checkout", "-q", "-b", "feature/c"]);
        commit_file(&repo, "migrations/002_c.sql", "c");
        git(&repo, &["checkout", "-q", "dev"]);
        commit_file(&repo, "migrations/003_x.sql", "x");
        git(&repo, &["checkout", "-q", "feature/c"]);

        let target = current_files(&repo, &dirs).unwrap();
        assert_eq!(
            target,
            set(&["migrations/001_users.sql", "migrations/002_c.sql"])
        );

        let dev_now = files_at(&repo, "dev", &dirs).unwrap();
        let cands = [
            Candidate {
                files: dev_now.clone(),
                applied: dev_now,
                recency: 10,
                ..cand("dev_db", &[], 0)
            },
            Candidate {
                kind: CandidateKind::Snapshot,
                files: set(&["migrations/001_users.sql"]),
                applied: set(&["migrations/001_users.sql"]),
                recency: 1,
                ..cand("dev_db__s1", &[], 0)
            },
        ];
        let base = select_base(&target, &cands).unwrap();
        assert_eq!(base.db_name, "dev_db__s1");
        assert_eq!(pending(&target, &base.applied), ["migrations/002_c.sql"]);
    }

    #[test]
    fn worktree_includes_uncommitted_and_skips_noise() {
        let (_d, repo) = repo();
        let dirs = vec!["alembic/versions".to_string()];
        commit_file(&repo, "alembic/versions/a1_init.py", "a");
        let v = repo.root().join("alembic/versions");
        std::fs::write(v.join("b2_new.py"), "b").unwrap();
        std::fs::create_dir_all(v.join("__pycache__")).unwrap();
        std::fs::write(v.join("__init__.py"), "").unwrap();
        assert_eq!(
            current_files(&repo, &dirs).unwrap(),
            set(&["alembic/versions/a1_init.py", "alembic/versions/b2_new.py"])
        );
    }
}
