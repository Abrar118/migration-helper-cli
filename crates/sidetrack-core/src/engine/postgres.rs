//! Postgres operations, run through `psql` (spec P0.4, P0.5).
//!
//! By default psql runs inside the database container (`docker compose exec`), so
//! the host needs no Postgres client, published port or password.

use std::path::PathBuf;
use std::process::Command;
use std::thread::sleep;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::compose::Compose;
use crate::naming::{quote_ident, quote_literal};
use crate::process::capture;

const SEP: char = '\u{1f}';
/// Maintenance database psql connects to.
const MAINTENANCE_DB: &str = "postgres";

#[derive(Debug, Clone)]
pub struct Postgres {
    argv: Vec<String>,
    dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DbInfo {
    pub name: String,
    pub size_bytes: u64,
    pub allow_connections: bool,
}

/// How a clone was made, for reporting.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloneMethod {
    /// Postgres 18 `file_copy_method = clone`. Copy-on-write (near-instant, no extra
    /// disk) when the filesystem supports reflinks; otherwise the kernel copies normally.
    Clone,
    /// `STRATEGY FILE_COPY` (Postgres 15+).
    FileCopy,
    /// Plain `TEMPLATE` copy (older servers).
    Template,
}

impl Postgres {
    /// psql via an arbitrary command prefix, e.g. `["psql", "-h", "localhost", "-U", "postgres"]`.
    pub fn new(argv: Vec<String>, dir: Option<PathBuf>) -> Self {
        Self { argv, dir }
    }

    /// psql inside a compose service.
    pub fn in_compose(compose: &Compose, service: &str, user: &str) -> Self {
        let mut argv = compose.command_argv();
        argv.extend(["exec", "-T", service, "psql", "-U", user].map(String::from));
        Self::new(argv, Some(compose.dir().to_path_buf()))
    }

    /// Run statements in one session; each `-c` is its own transaction.
    fn psql(&self, statements: &[String]) -> Result<String> {
        self.psql_in(MAINTENANCE_DB, statements)
    }

    /// Run statements in one session on database `db`.
    fn psql_in(&self, db: &str, statements: &[String]) -> Result<String> {
        let (prog, args) = self.argv.split_first().context("empty psql command")?;
        let mut cmd = Command::new(prog);
        cmd.args(args);
        if let Some(d) = &self.dir {
            cmd.current_dir(d);
        }
        cmd.args(["-X", "-q", "-A", "-t", "-v", "ON_ERROR_STOP=1", "-d", db]);
        cmd.arg("-F").arg(SEP.to_string());
        for s in statements {
            cmd.arg("-c").arg(s);
        }
        capture(&mut cmd)
    }

    fn query(&self, sql: &str) -> Result<Vec<Vec<String>>> {
        self.query_in(MAINTENANCE_DB, sql)
    }

    fn query_in(&self, db: &str, sql: &str) -> Result<Vec<Vec<String>>> {
        let out = self.psql_in(db, &[sql.to_string()])?;
        Ok(out
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.split(SEP).map(str::to_string).collect())
            .collect())
    }

    fn scalar(&self, sql: &str) -> Result<String> {
        let rows = self.query(sql)?;
        rows.into_iter()
            .next()
            .and_then(|r| r.into_iter().next())
            .with_context(|| format!("no result for `{sql}`"))
    }

    /// Rows of a migration tool's version table, for several databases in one psql
    /// session. `None` for a database means the table doesn't exist there (nothing
    /// applied yet). Databases that refuse connections (frozen snapshots) must not
    /// be passed: a failed `\connect` ends the session.
    pub fn version_rows(
        &self,
        dbs: &[String],
        table: &str,
        query: &str,
    ) -> Result<std::collections::BTreeMap<String, Option<Vec<String>>>> {
        let mut out = std::collections::BTreeMap::new();
        if dbs.is_empty() {
            return Ok(out);
        }
        let (prog, args) = self.argv.split_first().context("empty psql command")?;
        let mut cmd = Command::new(prog);
        cmd.args(args);
        if let Some(d) = &self.dir {
            cmd.current_dir(d);
        }
        // A psql script: only query the table where it exists (`\gset` + `\if`, psql 10+).
        cmd.args([
            "-X",
            "-q",
            "-A",
            "-t",
            "-v",
            "ON_ERROR_STOP=1",
            "-d",
            MAINTENANCE_DB,
        ]);
        cmd.arg("-F").arg(SEP.to_string());
        cmd.args(["-f", "-"]);
        let mut script = String::new();
        for db in dbs {
            script.push_str(&format!(
                "\\connect {db}\n\
                 SELECT '@db', current_database();\n\
                 SELECT to_regclass({table}) IS NOT NULL AS st_has \\gset\n\
                 \\if :st_has\n\
                 SELECT '@has', 't';\n\
                 SELECT '@row', q.* FROM ({query}) q;\n\
                 \\else\n\
                 SELECT '@has', 'f';\n\
                 \\endif\n",
                db = quote_ident(db),
                table = quote_literal(table),
            ));
        }
        let text = crate::process::capture_with_input(&mut cmd, &script)?;
        let mut current: Option<String> = None;
        for line in text.lines() {
            let mut parts = line.splitn(2, SEP);
            match (parts.next(), parts.next()) {
                (Some("@db"), Some(name)) => current = Some(name.to_string()),
                (Some("@has"), Some(has)) => {
                    if let Some(db) = &current {
                        out.insert(db.clone(), (has == "t").then(Vec::new));
                    }
                }
                (Some("@row"), Some(row)) => {
                    if let Some(Some(rows)) = current.as_ref().and_then(|db| out.get_mut(db)) {
                        rows.push(row.to_string());
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }

    /// Tables and columns of `db` (user schemas only).
    pub fn schema(&self, db: &str) -> Result<crate::schema::Schema> {
        let rows = self.query_in(
            db,
            "SELECT table_schema || '.' || table_name, column_name || ' ' || data_type \
             FROM information_schema.columns \
             WHERE table_schema NOT IN ('pg_catalog', 'information_schema') \
             ORDER BY table_schema, table_name, ordinal_position",
        )?;
        let mut out = crate::schema::Schema::new();
        for r in rows {
            if let [table, column] = r.as_slice() {
                out.entry(table.clone()).or_default().push(column.clone());
            }
        }
        Ok(out)
    }

    /// e.g. 160004 for 16.4.
    pub fn server_version_num(&self) -> Result<u32> {
        self.scalar("SHOW server_version_num")?
            .trim()
            .parse()
            .context("parsing server_version_num")
    }

    /// All user databases (not template0/template1).
    pub fn list_databases(&self) -> Result<Vec<DbInfo>> {
        let rows = self.query(
            "SELECT datname, pg_database_size(datname), datallowconn FROM pg_database \
             WHERE datname NOT IN ('template0', 'template1') ORDER BY datname",
        )?;
        rows.into_iter()
            .map(|r| {
                anyhow::ensure!(r.len() == 3, "unexpected row from pg_database: {r:?}");
                Ok(DbInfo {
                    name: r[0].clone(),
                    size_bytes: r[1].parse().unwrap_or(0),
                    allow_connections: r[2] == "t",
                })
            })
            .collect()
    }

    pub fn exists(&self, db: &str) -> Result<bool> {
        let n = self.scalar(&format!(
            "SELECT count(*) FROM pg_database WHERE datname = {}",
            quote_literal(db)
        ))?;
        Ok(n.trim() == "1")
    }

    /// Other sessions connected to `db`, as `application_name` (or "unknown").
    pub fn connections(&self, db: &str) -> Result<Vec<String>> {
        let rows = self.query(&format!(
            "SELECT coalesce(nullif(application_name, ''), 'unknown') FROM pg_stat_activity \
             WHERE datname = {} AND pid <> pg_backend_pid()",
            quote_literal(db)
        ))?;
        Ok(rows
            .into_iter()
            .filter_map(|r| r.into_iter().next())
            .collect())
    }

    /// Open sessions for every database, as `application_name` (or "unknown").
    pub fn connections_by_db(&self) -> Result<std::collections::BTreeMap<String, Vec<String>>> {
        let rows = self.query(
            "SELECT datname, coalesce(nullif(application_name, ''), 'unknown') FROM pg_stat_activity \
             WHERE datname IS NOT NULL AND pid <> pg_backend_pid()",
        )?;
        let mut out = std::collections::BTreeMap::<String, Vec<String>>::new();
        for r in rows {
            if let [db, app] = r.as_slice() {
                out.entry(db.clone()).or_default().push(app.clone());
            }
        }
        Ok(out)
    }

    pub fn terminate_connections(&self, db: &str) -> Result<u32> {
        let n = self.scalar(&format!(
            "SELECT count(pg_terminate_backend(pid)) FROM pg_stat_activity \
             WHERE datname = {} AND pid <> pg_backend_pid()",
            quote_literal(db)
        ))?;
        Ok(n.trim().parse().unwrap_or(0))
    }

    /// Copy `source` into a new database `target`.
    ///
    /// Postgres refuses while anything is connected to `source`, so leftover
    /// connections are closed and the copy is retried a few times.
    pub fn clone_database(&self, source: &str, target: &str) -> Result<CloneMethod> {
        let version = self.server_version_num()?;
        let mut last_err = None;
        for attempt in 0..3 {
            if attempt > 0 {
                sleep(Duration::from_millis(300 * attempt));
            }
            self.terminate_connections(source)?;
            match self.try_clone(version, source, target) {
                Ok(m) => return Ok(m),
                Err(e) if format!("{e:#}").contains("is being accessed by other users") => {
                    last_err = Some(e);
                }
                Err(e) => return Err(e),
            }
        }
        Err(last_err.expect("loop ran")).context(format!("{source} kept getting new connections"))
    }

    fn try_clone(&self, version: u32, source: &str, target: &str) -> Result<CloneMethod> {
        let (src, dst) = (quote_ident(source), quote_ident(target));
        if version >= 180000 {
            // Clone via copy_file_range / reflink where the platform supports it.
            let stmts = [
                "SET file_copy_method = clone".to_string(),
                format!("CREATE DATABASE {dst} TEMPLATE {src} STRATEGY FILE_COPY"),
            ];
            if self.psql(&stmts).is_ok() {
                return Ok(CloneMethod::Clone);
            }
            // A failed clone can leave nothing behind, but make sure before retrying.
            if self.exists(target)? {
                return Ok(CloneMethod::Clone);
            }
        }
        if version >= 150000 {
            self.psql(&[format!(
                "CREATE DATABASE {dst} TEMPLATE {src} STRATEGY FILE_COPY"
            )])?;
            Ok(CloneMethod::FileCopy)
        } else {
            self.psql(&[format!("CREATE DATABASE {dst} TEMPLATE {src}")])?;
            Ok(CloneMethod::Template)
        }
    }

    pub fn create_empty(&self, name: &str) -> Result<()> {
        self.psql(&[format!("CREATE DATABASE {}", quote_ident(name))])
            .map(|_| ())
    }

    pub fn drop_database(&self, name: &str) -> Result<()> {
        let q = quote_ident(name);
        if self.server_version_num()? >= 130000 {
            self.psql(&[format!("DROP DATABASE IF EXISTS {q} WITH (FORCE)")])?;
        } else {
            self.terminate_connections(name)?;
            self.psql(&[format!("DROP DATABASE IF EXISTS {q}")])?;
        }
        Ok(())
    }

    pub fn rename_database(&self, from: &str, to: &str) -> Result<()> {
        self.terminate_connections(from)?;
        self.psql(&[format!(
            "ALTER DATABASE {} RENAME TO {}",
            quote_ident(from),
            quote_ident(to)
        )])
        .map(|_| ())
    }

    /// Freeze (false) or unfreeze (true) a database. Frozen snapshots can still be copied.
    pub fn set_allow_connections(&self, name: &str, allow: bool) -> Result<()> {
        self.psql(&[format!(
            "ALTER DATABASE {} ALLOW_CONNECTIONS {allow}",
            quote_ident(name)
        )])
        .map(|_| ())
    }

    /// Run SQL inside a specific database (used by tests and seeding helpers).
    pub fn execute_in(&self, db: &str, sql: &str) -> Result<String> {
        let (prog, args) = self.argv.split_first().context("empty psql command")?;
        let mut cmd = Command::new(prog);
        cmd.args(args);
        if let Some(d) = &self.dir {
            cmd.current_dir(d);
        }
        cmd.args([
            "-X",
            "-q",
            "-A",
            "-t",
            "-v",
            "ON_ERROR_STOP=1",
            "-d",
            db,
            "-c",
            sql,
        ]);
        capture(&mut cmd)
    }
}

/// Human-readable size.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(
            format_bytes(2 * 1024 * 1024 * 1024 + 100 * 1024 * 1024),
            "2.1 GB"
        );
    }

    /// Set SIDETRACK_TEST_PG to a psql command prefix, e.g.
    /// `docker exec -i st-pg psql -U postgres`.
    fn test_pg() -> Option<Postgres> {
        let argv = std::env::var("SIDETRACK_TEST_PG").ok()?;
        Some(Postgres::new(
            argv.split_whitespace().map(String::from).collect(),
            None,
        ))
    }

    #[test]
    fn engine_operations_against_real_postgres() {
        let Some(pg) = test_pg() else {
            eprintln!("skipped: SIDETRACK_TEST_PG not set");
            return;
        };
        let (a, b, c) = ("st_test_a_db", "st_test_b_db", "st_test_c_db");
        for d in [a, b, c] {
            pg.drop_database(d).unwrap();
        }
        pg.create_empty(a).unwrap();
        pg.execute_in(a, "CREATE TABLE t (id int); INSERT INTO t VALUES (1), (2)")
            .unwrap();

        let method = pg.clone_database(a, b).unwrap();
        eprintln!("clone method: {method:?}");
        assert_eq!(
            pg.execute_in(b, "SELECT count(*) FROM t").unwrap().trim(),
            "2"
        );

        pg.set_allow_connections(b, false).unwrap();
        let info = pg.list_databases().unwrap();
        let bi = info.iter().find(|d| d.name == b).unwrap();
        assert!(!bi.allow_connections);
        assert!(bi.size_bytes > 0);

        // A frozen database can still be the source of a copy.
        pg.clone_database(b, c).unwrap();
        pg.rename_database(c, "st_test_renamed_db").unwrap();
        assert!(pg.exists("st_test_renamed_db").unwrap());
        assert!(!pg.exists(c).unwrap());

        for d in [a, b, "st_test_renamed_db"] {
            pg.drop_database(d).unwrap();
            assert!(!pg.exists(d).unwrap());
        }
    }

    #[test]
    fn version_rows_across_databases() {
        let Some(pg) = test_pg() else {
            eprintln!("skipped: SIDETRACK_TEST_PG not set");
            return;
        };
        let (with, without) = ("st_test_versions_db", "st_test_noversions_db");
        for d in [with, without] {
            pg.drop_database(d).unwrap();
            pg.create_empty(d).unwrap();
        }
        pg.execute_in(with, "CREATE TABLE alembic_version (version_num varchar(32)); INSERT INTO alembic_version VALUES ('b2'), ('c3')")
            .unwrap();
        let rows = pg
            .version_rows(
                &[with.to_string(), without.to_string()],
                "alembic_version",
                "SELECT (version_num)::text FROM \"alembic_version\"",
            )
            .unwrap();
        assert_eq!(rows[with], Some(vec!["b2".to_string(), "c3".to_string()]));
        assert_eq!(rows[without], None);
        for d in [with, without] {
            pg.drop_database(d).unwrap();
        }
    }
}
