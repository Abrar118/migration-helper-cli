//! Wrapper over `docker compose`. We never parse compose YAML ourselves: compose
//! resolves overrides, profiles, `include`, `env_file` and interpolation for us.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::process::{capture, run};

#[derive(Debug, Clone)]
pub struct Compose {
    dir: PathBuf,
    files: Vec<PathBuf>,
}

/// How much compose should resolve when printing the config.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConfigView {
    /// Final values: env files merged in, `${VAR}` substituted.
    Resolved,
    /// `env_file` entries kept as paths, inline `environment` only.
    EnvFiles,
    /// `${VAR}` references left as written.
    Raw,
}

impl Compose {
    pub fn new(dir: impl Into<PathBuf>, files: Vec<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            files,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `docker compose [-f …]` running in the project directory.
    pub fn command(&self) -> Command {
        let mut c = Command::new("docker");
        c.current_dir(&self.dir).arg("compose");
        for f in &self.files {
            c.arg("-f").arg(f);
        }
        c
    }

    /// The same prefix as a list of strings (for building psql commands).
    pub fn command_argv(&self) -> Vec<String> {
        let mut v = vec!["docker".to_string(), "compose".to_string()];
        for f in &self.files {
            v.push("-f".into());
            v.push(f.to_string_lossy().into_owned());
        }
        v
    }

    pub fn config(&self, view: ConfigView) -> Result<ComposeModel> {
        let mut c = self.command();
        c.args(["config", "--format", "json"]);
        match view {
            ConfigView::Resolved => {}
            ConfigView::EnvFiles => {
                c.arg("--no-env-resolution");
            }
            ConfigView::Raw => {
                c.arg("--no-interpolate");
            }
        }
        let out = capture(&mut c)?;
        ComposeModel::from_json(&out)
    }

    pub fn running_services(&self) -> Result<Vec<String>> {
        let out = capture(
            self.command()
                .args(["ps", "--services", "--status", "running"]),
        )?;
        Ok(out
            .lines()
            .map(str::to_string)
            .filter(|l| !l.is_empty())
            .collect())
    }

    pub fn stop(&self, services: &[String]) -> Result<()> {
        if services.is_empty() {
            return Ok(());
        }
        run(self.command().arg("stop").args(services))
    }

    /// Recreate services so they pick up env file changes. `restart` would not.
    pub fn recreate(&self, services: &[String]) -> Result<()> {
        if services.is_empty() {
            return Ok(());
        }
        run(self
            .command()
            .args(["up", "-d", "--no-deps", "--force-recreate"])
            .args(services))
    }

    /// Run a command in a running service and capture stdout.
    pub fn exec(&self, service: &str, args: &[&str]) -> Result<String> {
        capture(self.command().args(["exec", "-T", service]).args(args))
    }
}

/// Is the Docker daemon reachable?
pub fn docker_available() -> bool {
    Command::new("docker")
        .args(["info", "--format", "{{.ServerVersion}}"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ComposeModel {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub services: BTreeMap<String, Service>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Service {
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, Option<String>>,
    #[serde(default)]
    pub env_file: Vec<EnvFileRef>,
    #[serde(default)]
    pub depends_on: Value,
    #[serde(default)]
    pub build: Option<Build>,
    #[serde(default)]
    pub volumes: Vec<Volume>,
    #[serde(default)]
    pub working_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnvFileRef {
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Build {
    #[serde(default)]
    pub context: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Volume {
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
}

impl ComposeModel {
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json).context("parsing `docker compose config` output")
    }
}

impl Service {
    pub fn env(&self, key: &str) -> Option<&str> {
        self.environment.get(key).and_then(|v| v.as_deref())
    }

    /// Names of services this one depends on (list or map form).
    pub fn depends_on(&self) -> Vec<String> {
        match &self.depends_on {
            Value::Array(items) => items
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
            Value::Object(map) => map.keys().cloned().collect(),
            _ => vec![],
        }
    }

    /// Host directories this service can see, with their path inside the container
    /// when known. Bind mounts come first (exact container path), then the build context.
    pub fn host_paths(&self) -> Vec<(PathBuf, Option<String>)> {
        let mut out = vec![];
        for v in &self.volumes {
            if v.kind.as_deref() == Some("bind") {
                if let Some(src) = &v.source {
                    out.push((PathBuf::from(src), v.target.clone()));
                }
            }
        }
        if let Some(ctx) = self.build.as_ref().and_then(|b| b.context.clone()) {
            out.push((PathBuf::from(ctx), None));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{
      "name": "shop",
      "services": {
        "api": {
          "environment": {"DATABASE_URL": "postgresql://app:s@db:5432/app_dev", "EMPTY": null},
          "env_file": [{"path": "/repo/.env.local"}],
          "build": {"context": "/repo/api", "dockerfile": "Dockerfile"},
          "volumes": [{"type": "bind", "source": "/repo/api", "target": "/app"}, {"type": "volume", "source": "data", "target": "/data"}],
          "depends_on": {"db": {"condition": "service_started"}}
        },
        "db": {"image": "postgres:16-alpine", "environment": {"POSTGRES_USER": "app"}},
        "worker": {"depends_on": ["db"]}
      }
    }"#;

    #[test]
    fn parses_config_json() {
        let m = ComposeModel::from_json(JSON).unwrap();
        assert_eq!(m.name, "shop");
        let api = &m.services["api"];
        assert_eq!(
            api.env("DATABASE_URL"),
            Some("postgresql://app:s@db:5432/app_dev")
        );
        assert_eq!(api.env("EMPTY"), None);
        assert_eq!(api.env_file[0].path, "/repo/.env.local");
        assert_eq!(api.depends_on(), ["db"]);
        assert_eq!(m.services["worker"].depends_on(), ["db"]);
        assert_eq!(
            api.host_paths().len(),
            2,
            "build context + bind mount, not the named volume"
        );
        assert_eq!(
            m.services["db"].image.as_deref(),
            Some("postgres:16-alpine")
        );
    }
}
