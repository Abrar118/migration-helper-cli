//! Local SQLite registry (spec P0.2): which git branch maps to which database.
//!
//! The registry is a record, not the truth: the database server is. Every command
//! reconciles the two before planning (see `planner::reconcile`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

pub type FileSet = BTreeSet<String>;

const SCHEMA_V1: &str = r#"
CREATE TABLE projects (
    id              INTEGER PRIMARY KEY,
    repo_root       TEXT NOT NULL UNIQUE,
    compose_project TEXT NOT NULL,
    engine          TEXT NOT NULL,
    db_service      TEXT NOT NULL,
    auto_approve    INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL
);
CREATE TABLE db_branches (
    id              INTEGER PRIMARY KEY,
    project_id      INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    git_branch      TEXT NOT NULL,
    db_name         TEXT NOT NULL,
    parent_id       INTEGER REFERENCES db_branches(id) ON DELETE SET NULL,
    base_commit     TEXT,
    migration_files TEXT NOT NULL DEFAULT '[]',
    adopted         INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL,
    last_used_at    INTEGER NOT NULL,
    UNIQUE (project_id, git_branch),
    UNIQUE (project_id, db_name)
);
CREATE TABLE snapshots (
    id              INTEGER PRIMARY KEY,
    branch_id       INTEGER NOT NULL REFERENCES db_branches(id) ON DELETE CASCADE,
    seq             INTEGER NOT NULL,
    label           TEXT,
    db_name         TEXT NOT NULL,
    commit_id       TEXT,
    migration_files TEXT NOT NULL DEFAULT '[]',
    size_bytes      INTEGER,
    created_at      INTEGER NOT NULL,
    UNIQUE (branch_id, seq)
);
CREATE TABLE operations (
    id          INTEGER PRIMARY KEY,
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,
    plan_json   TEXT NOT NULL,
    status      TEXT NOT NULL,
    error       TEXT,
    started_at  INTEGER NOT NULL,
    finished_at INTEGER
);
"#;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub repo_root: String,
    pub compose_project: String,
    pub engine: String,
    pub db_service: String,
    pub auto_approve: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DbBranch {
    pub id: i64,
    pub project_id: i64,
    pub git_branch: String,
    pub db_name: String,
    pub parent_id: Option<i64>,
    pub base_commit: Option<String>,
    /// Migration files known to be applied to this database.
    pub migration_files: FileSet,
    /// Existing database taken over at `init` (not created by sidetrack).
    pub adopted: bool,
    pub created_at: i64,
    pub last_used_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: i64,
    pub branch_id: i64,
    pub seq: i64,
    pub label: Option<String>,
    pub db_name: String,
    pub commit_id: Option<String>,
    pub migration_files: FileSet,
    pub size_bytes: Option<u64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewBranch {
    pub git_branch: String,
    pub db_name: String,
    pub parent_id: Option<i64>,
    pub base_commit: Option<String>,
    pub migration_files: FileSet,
    pub adopted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewSnapshot {
    pub branch_id: i64,
    pub seq: i64,
    pub label: Option<String>,
    pub db_name: String,
    pub commit_id: Option<String>,
    pub migration_files: FileSet,
    pub size_bytes: Option<u64>,
}

pub struct Registry {
    conn: Connection,
}

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// `$SIDETRACK_REGISTRY`, or the platform data dir (`~/.local/share/sidetrack/registry.sqlite`).
pub fn default_path() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("SIDETRACK_REGISTRY") {
        return Ok(PathBuf::from(p));
    }
    let base = dirs::data_dir().context("no data directory on this platform")?;
    Ok(base.join("sidetrack").join("registry.sqlite"))
}

fn files_to_json(files: &FileSet) -> String {
    serde_json::to_string(files).expect("string set serializes")
}

fn files_from_json(s: String) -> FileSet {
    serde_json::from_str(&s).unwrap_or_default()
}

impl Registry {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening registry {}", path.display()))?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(&format!("BEGIN; {SCHEMA_V1} PRAGMA user_version = 1; COMMIT;"))?;
        }
        Ok(Self { conn })
    }

    // ---- projects ----

    pub fn upsert_project(
        &self,
        repo_root: &str,
        compose_project: &str,
        engine: &str,
        db_service: &str,
    ) -> Result<Project> {
        self.conn.execute(
            "INSERT INTO projects (repo_root, compose_project, engine, db_service, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (repo_root) DO UPDATE SET
               compose_project = excluded.compose_project,
               engine = excluded.engine,
               db_service = excluded.db_service",
            params![repo_root, compose_project, engine, db_service, now()],
        )?;
        self.project_by_root(repo_root)?.context("project vanished after insert")
    }

    pub fn project_by_root(&self, repo_root: &str) -> Result<Option<Project>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, repo_root, compose_project, engine, db_service, auto_approve
                 FROM projects WHERE repo_root = ?1",
                [repo_root],
                |r| {
                    Ok(Project {
                        id: r.get(0)?,
                        repo_root: r.get(1)?,
                        compose_project: r.get(2)?,
                        engine: r.get(3)?,
                        db_service: r.get(4)?,
                        auto_approve: r.get(5)?,
                    })
                },
            )
            .optional()?)
    }

    pub fn projects(&self) -> Result<Vec<Project>> {
        let mut st = self.conn.prepare(
            "SELECT id, repo_root, compose_project, engine, db_service, auto_approve FROM projects ORDER BY id",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Project {
                id: r.get(0)?,
                repo_root: r.get(1)?,
                compose_project: r.get(2)?,
                engine: r.get(3)?,
                db_service: r.get(4)?,
                auto_approve: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn set_auto_approve(&self, project_id: i64, on: bool) -> Result<()> {
        self.conn.execute("UPDATE projects SET auto_approve = ?2 WHERE id = ?1", params![project_id, on])?;
        Ok(())
    }

    // ---- branches ----

    const BRANCH_COLS: &'static str = "id, project_id, git_branch, db_name, parent_id, base_commit, \
        migration_files, adopted, created_at, last_used_at";

    fn branch_row(r: &Row) -> rusqlite::Result<DbBranch> {
        Ok(DbBranch {
            id: r.get(0)?,
            project_id: r.get(1)?,
            git_branch: r.get(2)?,
            db_name: r.get(3)?,
            parent_id: r.get(4)?,
            base_commit: r.get(5)?,
            migration_files: files_from_json(r.get(6)?),
            adopted: r.get(7)?,
            created_at: r.get(8)?,
            last_used_at: r.get(9)?,
        })
    }

    pub fn branches(&self, project_id: i64) -> Result<Vec<DbBranch>> {
        let sql = format!(
            "SELECT {} FROM db_branches WHERE project_id = ?1 ORDER BY last_used_at DESC, id",
            Self::BRANCH_COLS
        );
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map([project_id], Self::branch_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn branch_by_git(&self, project_id: i64, git_branch: &str) -> Result<Option<DbBranch>> {
        let sql = format!("SELECT {} FROM db_branches WHERE project_id = ?1 AND git_branch = ?2", Self::BRANCH_COLS);
        Ok(self.conn.query_row(&sql, params![project_id, git_branch], Self::branch_row).optional()?)
    }

    pub fn branch_by_db(&self, project_id: i64, db_name: &str) -> Result<Option<DbBranch>> {
        let sql = format!("SELECT {} FROM db_branches WHERE project_id = ?1 AND db_name = ?2", Self::BRANCH_COLS);
        Ok(self.conn.query_row(&sql, params![project_id, db_name], Self::branch_row).optional()?)
    }

    pub fn branch(&self, id: i64) -> Result<Option<DbBranch>> {
        let sql = format!("SELECT {} FROM db_branches WHERE id = ?1", Self::BRANCH_COLS);
        Ok(self.conn.query_row(&sql, [id], Self::branch_row).optional()?)
    }

    pub fn insert_branch(&self, project_id: i64, b: &NewBranch) -> Result<DbBranch> {
        let t = now();
        self.conn.execute(
            "INSERT INTO db_branches (project_id, git_branch, db_name, parent_id, base_commit,
                migration_files, adopted, created_at, last_used_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![
                project_id,
                b.git_branch,
                b.db_name,
                b.parent_id,
                b.base_commit,
                files_to_json(&b.migration_files),
                b.adopted,
                t
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.branch(id)?.context("branch vanished after insert")
    }

    pub fn set_branch_files(&self, id: i64, files: &FileSet) -> Result<()> {
        self.conn.execute(
            "UPDATE db_branches SET migration_files = ?2 WHERE id = ?1",
            params![id, files_to_json(files)],
        )?;
        Ok(())
    }

    pub fn touch_branch(&self, id: i64) -> Result<()> {
        self.conn.execute("UPDATE db_branches SET last_used_at = ?2 WHERE id = ?1", params![id, now()])?;
        Ok(())
    }

    pub fn delete_branch(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM db_branches WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---- snapshots ----

    fn snapshot_row(r: &Row) -> rusqlite::Result<Snapshot> {
        Ok(Snapshot {
            id: r.get(0)?,
            branch_id: r.get(1)?,
            seq: r.get(2)?,
            label: r.get(3)?,
            db_name: r.get(4)?,
            commit_id: r.get(5)?,
            migration_files: files_from_json(r.get(6)?),
            size_bytes: r.get::<_, Option<i64>>(7)?.map(|v| v as u64),
            created_at: r.get(8)?,
        })
    }

    /// All snapshots of a project, newest first.
    pub fn snapshots(&self, project_id: i64) -> Result<Vec<Snapshot>> {
        let mut st = self.conn.prepare(
            "SELECT s.id, s.branch_id, s.seq, s.label, s.db_name, s.commit_id, s.migration_files,
                    s.size_bytes, s.created_at
             FROM snapshots s JOIN db_branches b ON b.id = s.branch_id
             WHERE b.project_id = ?1 ORDER BY s.created_at DESC, s.id DESC",
        )?;
        let rows = st.query_map([project_id], Self::snapshot_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn next_snapshot_seq(&self, branch_id: i64) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT coalesce(max(seq), 0) + 1 FROM snapshots WHERE branch_id = ?1",
            [branch_id],
            |r| r.get(0),
        )?)
    }

    pub fn insert_snapshot(&self, s: &NewSnapshot) -> Result<Snapshot> {
        self.conn.execute(
            "INSERT INTO snapshots (branch_id, seq, label, db_name, commit_id, migration_files,
                size_bytes, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                s.branch_id,
                s.seq,
                s.label,
                s.db_name,
                s.commit_id,
                files_to_json(&s.migration_files),
                s.size_bytes.map(|v| v as i64),
                now()
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(self.conn.query_row(
            "SELECT id, branch_id, seq, label, db_name, commit_id, migration_files, size_bytes, created_at
             FROM snapshots WHERE id = ?1",
            [id],
            Self::snapshot_row,
        )?)
    }

    pub fn delete_snapshot(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM snapshots WHERE id = ?1", [id])?;
        Ok(())
    }

    // ---- operation log ----

    pub fn start_operation(&self, project_id: i64, kind: &str, plan_json: &str) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO operations (project_id, kind, plan_json, status, started_at)
             VALUES (?1, ?2, ?3, 'running', ?4)",
            params![project_id, kind, plan_json, now()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn finish_operation(&self, id: i64, status: &str, error: Option<&str>) -> Result<()> {
        self.conn.execute(
            "UPDATE operations SET status = ?2, error = ?3, finished_at = ?4 WHERE id = ?1",
            params![id, status, error, now()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(names: &[&str]) -> FileSet {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn crud() {
        let dir = tempfile::tempdir().unwrap();
        let reg = Registry::open(&dir.path().join("sub/registry.sqlite")).unwrap();
        let p = reg.upsert_project("/repo", "shop", "postgres", "db").unwrap();
        assert_eq!(reg.upsert_project("/repo", "shop2", "postgres", "db").unwrap().id, p.id);
        assert!(!p.auto_approve);
        reg.set_auto_approve(p.id, true).unwrap();
        assert!(reg.project_by_root("/repo").unwrap().unwrap().auto_approve);

        let dev = reg
            .insert_branch(
                p.id,
                &NewBranch {
                    git_branch: "dev".into(),
                    db_name: "app_dev".into(),
                    parent_id: None,
                    base_commit: Some("abc".into()),
                    migration_files: files(&["001.sql"]),
                    adopted: true,
                },
            )
            .unwrap();
        assert_eq!(reg.branch_by_db(p.id, "app_dev").unwrap().unwrap().id, dev.id);
        assert_eq!(dev.migration_files, files(&["001.sql"]));

        reg.set_branch_files(dev.id, &files(&["001.sql", "002.sql"])).unwrap();
        assert_eq!(reg.branch_by_git(p.id, "dev").unwrap().unwrap().migration_files.len(), 2);

        assert_eq!(reg.next_snapshot_seq(dev.id).unwrap(), 1);
        let s = reg
            .insert_snapshot(&NewSnapshot {
                branch_id: dev.id,
                seq: 1,
                label: Some("before".into()),
                db_name: "app_dev__s1".into(),
                commit_id: None,
                migration_files: files(&["001.sql"]),
                size_bytes: Some(42),
            })
            .unwrap();
        assert_eq!(reg.next_snapshot_seq(dev.id).unwrap(), 2);
        assert_eq!(reg.snapshots(p.id).unwrap(), vec![s]);

        // Deleting a branch removes its snapshots.
        reg.delete_branch(dev.id).unwrap();
        assert!(reg.snapshots(p.id).unwrap().is_empty());

        let op = reg.start_operation(p.id, "switch", "{}").unwrap();
        reg.finish_operation(op, "ok", None).unwrap();
    }

    #[test]
    fn reopening_keeps_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("r.sqlite");
        Registry::open(&path).unwrap().upsert_project("/r", "x", "postgres", "db").unwrap();
        assert_eq!(Registry::open(&path).unwrap().projects().unwrap().len(), 1);
    }
}
