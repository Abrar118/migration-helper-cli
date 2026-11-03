//! `sidetrack.toml`: per-repo settings, safe to commit (no secrets).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "sidetrack.toml";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub database: DatabaseConfig,
    pub routing: RoutingConfig,
    pub migrations: MigrationsConfig,
    #[serde(default, skip_serializing_if = "ComposeConfig::is_default")]
    pub compose: ComposeConfig,
    #[serde(default, skip_serializing_if = "GitConfig::is_default")]
    pub git: GitConfig,
    #[serde(default, skip_serializing_if = "SnapshotsConfig::is_default")]
    pub snapshots: SnapshotsConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SnapshotsConfig {
    /// `gc` keeps this many of the newest *unlabelled* snapshots per DB branch and
    /// drops older ones. Labelled snapshots are always kept. Unset = keep all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep: Option<usize>,
}

impl SnapshotsConfig {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Only "postgres" in v1.
    pub engine: String,
    /// Compose service that runs the database.
    pub service: String,
    /// Database user that sidetrack connects as. Needs CREATEDB.
    pub user: String,
    /// Override the command used to run psql. Default:
    /// `docker compose exec -T <service> psql -U <user>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub psql: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingConfig {
    /// Only "env" in v1. "proxy" is planned for v2.
    pub mode: String,
    /// The env file the app reads, relative to the repo root.
    pub file: String,
    /// Variable that holds the database name (a URL or a bare name).
    pub variable: String,
    /// Compose services to recreate after the env file changes.
    #[serde(default)]
    pub restart: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MigrationsConfig {
    /// Detected tool, informational ("alembic", "django", "generic", …).
    pub tool: String,
    /// Directories (relative to the repo root) that hold one file or folder per migration.
    pub dirs: Vec<String>,
    /// Shell command that migrates the *active* database forward.
    pub command: String,
    /// Optional shell command that seeds an empty database after migrating.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<String>,
    /// Table the tool records applied migrations in. Defaults per tool
    /// (`alembic_version`, `django_migrations`, …); required for "generic".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_table: Option<String>,
    /// Column in `version_table` naming each applied migration (generic only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_column: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ComposeConfig {
    /// Extra `-f` files, relative to the repo root. Empty = compose defaults.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// Directory to run compose in, relative to the repo root. Default ".".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<String>,
}

impl ComposeConfig {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GitConfig {
    /// Branch that feature branches merge into; used by `gc` to find merged branches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_branch: Option<String>,
}

impl GitConfig {
    fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

impl Config {
    pub fn path(repo_root: &Path) -> PathBuf {
        repo_root.join(CONFIG_FILE)
    }

    pub fn exists(repo_root: &Path) -> bool {
        Self::path(repo_root).is_file()
    }

    pub fn load(repo_root: &Path) -> Result<Self> {
        let path = Self::path(repo_root);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {} (run `sidetrack init` first)", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn to_toml(&self) -> Result<String> {
        let body = toml::to_string_pretty(self)?;
        Ok(format!(
            "# sidetrack configuration — safe to commit, holds no secrets.\n\
             # Docs: docs/spec-v1.md\n\n{body}"
        ))
    }

    pub fn save(&self, repo_root: &Path) -> Result<()> {
        std::fs::write(Self::path(repo_root), self.to_toml()?)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.database.engine == "postgres",
            "database.engine = {:?} is not supported yet (only \"postgres\")",
            self.database.engine
        );
        anyhow::ensure!(
            self.routing.mode == "env",
            "routing.mode = {:?} is not supported yet (only \"env\")",
            self.routing.mode
        );
        anyhow::ensure!(
            !self.migrations.dirs.is_empty(),
            "migrations.dirs must not be empty"
        );
        Ok(())
    }

    pub fn compose_dir(&self, repo_root: &Path) -> PathBuf {
        match &self.compose.project_dir {
            Some(d) => repo_root.join(d),
            None => repo_root.to_path_buf(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn sample() -> Config {
        Config {
            database: DatabaseConfig {
                engine: "postgres".into(),
                service: "db".into(),
                user: "postgres".into(),
                psql: None,
            },
            routing: RoutingConfig {
                mode: "env".into(),
                file: ".env".into(),
                variable: "DATABASE_URL".into(),
                restart: vec!["api".into()],
            },
            migrations: MigrationsConfig {
                tool: "alembic".into(),
                dirs: vec!["alembic/versions".into()],
                command: "docker compose exec -T api alembic upgrade head".into(),
                seed: None,
                version_table: None,
                version_column: None,
            },
            compose: ComposeConfig::default(),
            git: GitConfig::default(),
            snapshots: SnapshotsConfig::default(),
        }
    }

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = sample();
        cfg.save(dir.path()).unwrap();
        let text = std::fs::read_to_string(Config::path(dir.path())).unwrap();
        assert!(
            !text.contains("[compose]"),
            "empty sections are omitted:\n{text}"
        );
        assert_eq!(Config::load(dir.path()).unwrap(), cfg);
    }

    #[test]
    fn rejects_unsupported_engine() {
        let mut cfg = sample();
        cfg.database.engine = "mysql".into();
        assert!(cfg.validate().is_err());
    }
}
