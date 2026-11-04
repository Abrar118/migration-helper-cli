//! `sidetrack init` detection (spec P0.3). Pure: works from compose config views and
//! repo files, so it's testable without Docker. Results are always shown to the user
//! and written to an editable `sidetrack.toml`; nothing is guessed at runtime.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::compose::{ComposeModel, Service};
use crate::config::{
    ComposeConfig, Config, DatabaseConfig, GitConfig, MigrationsConfig, RoutingConfig,
};
use crate::envfile;

/// The three ways compose can print its config (see `compose::ConfigView`).
pub struct ComposeViews {
    pub resolved: ComposeModel,
    pub env_files: ComposeModel,
    pub raw: ComposeModel,
}

#[derive(Debug, Clone, Serialize)]
pub struct Detection {
    pub compose_project: String,
    pub db_service: String,
    pub db_image: String,
    pub user: Option<String>,
    /// Set when the user comes from a `POSTGRES_USER_FILE` secret, which must be read in the container.
    pub user_file: Option<String>,
    pub active_db: String,
    pub app_services: Vec<String>,
    pub routing: Option<Routing>,
    pub routing_problem: Option<String>,
    /// App services whose DB name is written directly in compose `environment:`.
    /// Env mode can't rewrite it; proxy mode doesn't need to.
    pub inline: Vec<Inline>,
    pub migrations: Migrations,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Routing {
    /// Relative to the repo root.
    pub file: String,
    pub variable: String,
    pub services: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Inline {
    pub service: String,
    pub variable: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Migrations {
    pub tool: String,
    pub dirs: Vec<String>,
    pub command: Option<String>,
    /// Compose service the command runs in, if any.
    pub service: Option<String>,
}

const POSTGRES_IMAGES: [&str; 4] = ["postgres", "postgis", "timescale", "supabase/postgres"];

fn image_name(image: &str) -> String {
    let no_digest = image.split('@').next().unwrap_or(image);
    // Drop a tag, but not a registry port ("host:5000/postgres").
    match no_digest.rfind(':') {
        Some(i) if !no_digest[i..].contains('/') => no_digest[..i].to_lowercase(),
        _ => no_digest.to_lowercase(),
    }
}

fn is_postgres_image(image: &str) -> bool {
    let name = image_name(image);
    POSTGRES_IMAGES.iter().any(|p| name.contains(p))
}

pub fn detect(root: &Path, compose_dir: &Path, views: &ComposeViews) -> Result<Detection> {
    let resolved = &views.resolved;
    let mut notes = vec![];

    // ---- database service
    let mut dbs: Vec<(&String, &Service)> = resolved
        .services
        .iter()
        .filter(|(_, s)| s.image.as_deref().is_some_and(is_postgres_image))
        .collect();
    // Several services can use a Postgres image (e.g. one only for its psql). The
    // server is the one configured like a server and that others depend on.
    let score = |name: &str, s: &Service| {
        let configured = s.environment.keys().any(|k| k.starts_with("POSTGRES_"));
        let depended_on = resolved
            .services
            .values()
            .any(|o| o.depends_on().iter().any(|d| d == name));
        2 * configured as u8 + depended_on as u8
    };
    dbs.sort_by_key(|(n, s)| std::cmp::Reverse(score(n, s)));
    let (db_service, db) = match dbs.as_slice() {
        [] => bail!("no Postgres service found in the compose config (looked for images named {POSTGRES_IMAGES:?})"),
        [one] => *one,
        [first, ..] => {
            notes.push(format!(
                "found {} Postgres services; using {} (set database.service to change it)",
                dbs.len(),
                first.0
            ));
            *first
        }
    };
    let db_service = db_service.clone();
    let user = db.env("POSTGRES_USER").map(str::to_string);
    let user_file = db.env("POSTGRES_USER_FILE").map(str::to_string);
    let effective_user = user.clone().unwrap_or_else(|| "postgres".to_string());
    let active_db = db
        .env("POSTGRES_DB")
        .map(str::to_string)
        .unwrap_or_else(|| effective_user.clone());

    // ---- app services: anything that depends on or talks to the DB service
    let app_services: Vec<String> = resolved
        .services
        .iter()
        .filter(|(name, _)| **name != db_service)
        .filter(|(_, s)| s.depends_on().contains(&db_service) || mentions_host(s, &db_service))
        .map(|(n, _)| n.clone())
        .collect();

    // ---- routing variable
    let (routing, routing_problem, inline) = find_routing(
        root,
        compose_dir,
        views,
        &app_services,
        &active_db,
        &mut notes,
    );

    // ---- migrations
    let migrations = detect_migrations(root, resolved, &db_service, &app_services, &mut notes)?;

    Ok(Detection {
        compose_project: resolved.name.clone(),
        db_service,
        db_image: db.image.clone().unwrap_or_default(),
        user,
        user_file,
        active_db,
        app_services,
        routing,
        routing_problem,
        inline,
        migrations,
        notes,
    })
}

fn mentions_host(s: &Service, host: &str) -> bool {
    s.environment.values().flatten().any(|v| {
        v == host
            || v.contains(&format!("@{host}:"))
            || v.contains(&format!("@{host}/"))
            || v.ends_with(&format!("@{host}"))
    })
}

/// Rank variables that could hold the DB name: URLs first, then *DB*/*DATABASE* names.
fn routing_score(key: &str, value: &str, active_db: &str) -> Option<u8> {
    if envfile::db_name_in_value(value).as_deref() != Some(active_db) {
        return None;
    }
    let k = key.to_uppercase();
    if ["USER", "PASSWORD", "PASS", "HOST", "PORT"]
        .iter()
        .any(|bad| k.contains(bad))
    {
        return None;
    }
    if value.contains("://") {
        Some(3)
    } else if k.contains("DATABASE")
        || k.ends_with("_DB")
        || k.contains("DB_NAME")
        || k == "PGDATABASE"
    {
        Some(2)
    } else {
        None
    }
}

fn find_routing(
    root: &Path,
    compose_dir: &Path,
    views: &ComposeViews,
    app_services: &[String],
    active_db: &str,
    notes: &mut Vec<String>,
) -> (Option<Routing>, Option<String>, Vec<Inline>) {
    // (file, variable) → services that read the DB name from it
    let mut sources: BTreeMap<(String, String), Vec<String>> = BTreeMap::new();
    let mut problems = vec![];
    let mut inline = vec![];

    for svc in app_services {
        let Some(resolved) = views.resolved.services.get(svc) else {
            continue;
        };
        let best = resolved
            .environment
            .iter()
            .filter_map(|(k, v)| Some((routing_score(k, v.as_deref()?, active_db)?, k)))
            .max_by_key(|(score, _)| *score)
            .map(|(_, k)| k.clone());
        let Some(var) = best else { continue };
        match locate_variable(root, compose_dir, views, svc, &var, active_db) {
            Ok(Some((file, variable))) => sources
                .entry((file, variable))
                .or_default()
                .push(svc.clone()),
            Ok(None) => {
                problems.push(format!(
                    "{svc}: {var} is written directly in the compose file; move it to an untracked \
                     .env file so sidetrack can change it, or use `sidetrack init --proxy`"
                ));
                inline.push(Inline {
                    service: svc.clone(),
                    variable: var,
                });
            }
            Err(e) => problems.push(format!("{svc}: {e}")),
        }
    }

    let Some(((file, variable), services)) = sources.iter().max_by_key(|(_, s)| s.len()) else {
        let msg = if problems.is_empty() {
            format!("no app service has a variable holding the database name {active_db}")
        } else {
            problems.join("; ")
        };
        return (None, Some(msg), inline);
    };
    for ((f, v), s) in &sources {
        if (f, v) != (file, variable) {
            notes.push(format!(
                "{} read the DB name from {v} in {f}; v1 only rewrites {variable} in {file}, so they would stay on the old database",
                s.join(", ")
            ));
        }
    }
    notes.extend(problems);
    (
        Some(Routing {
            file: file.clone(),
            variable: variable.clone(),
            services: services.clone(),
        }),
        None,
        inline,
    )
}

/// Find which env file (and variable) a service's `var` really comes from.
/// `None` when it's written directly in the compose file.
fn locate_variable(
    root: &Path,
    compose_dir: &Path,
    views: &ComposeViews,
    svc: &str,
    var: &str,
    active_db: &str,
) -> Result<Option<(String, String)>> {
    let env_view = views
        .env_files
        .services
        .get(svc)
        .context("service missing from compose config")?;

    // Defined inline in `environment:`? Then it may reference a variable from the project .env.
    if env_view.environment.contains_key(var) {
        let raw = views
            .raw
            .services
            .get(svc)
            .and_then(|s| s.env(var))
            .unwrap_or_default()
            .to_string();
        let Some(reference) = variable_reference(&raw) else {
            return Ok(None);
        };
        let dotenv = compose_dir.join(".env");
        let content = std::fs::read_to_string(&dotenv).with_context(|| {
            format!(
                "{var} uses ${{{reference}}}, but {} can't be read",
                dotenv.display()
            )
        })?;
        let value = envfile::get(&content, &reference)
            .with_context(|| format!("{var} uses ${{{reference}}}, which isn't defined in .env"))?;
        if envfile::db_name_in_value(&value).as_deref() != Some(active_db) {
            bail!(
                "{var} builds the DB name from ${{{reference}}} in a way sidetrack can't rewrite"
            );
        }
        return Ok(Some((relative(root, &dotenv), reference)));
    }

    // Otherwise it comes from an env_file; later files win.
    for f in env_view.env_file.iter().rev() {
        let path = PathBuf::from(&f.path);
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Some(value) = envfile::get(&content, var) {
            if envfile::db_name_in_value(&value).as_deref() == Some(active_db) {
                return Ok(Some((relative(root, &path), var.to_string())));
            }
            bail!(
                "{var} in {} gets its DB name indirectly; point it at the DB name directly",
                f.path
            );
        }
    }
    bail!("couldn't find where {var} is defined")
}

/// The variable a raw compose value takes its DB name from: `${X}`, `$X`,
/// or a URL whose database part is `${X}`.
fn variable_reference(raw: &str) -> Option<String> {
    let part = envfile::db_name_in_value(raw).unwrap_or_else(|| raw.to_string());
    let whole = raw.trim();
    for candidate in [whole, part.as_str()] {
        if let Some(name) = candidate
            .strip_prefix("${")
            .and_then(|r| r.strip_suffix('}'))
        {
            let name = name.split([':', '-', '?']).next().unwrap_or(name);
            return Some(name.to_string());
        }
        if let Some(name) = candidate.strip_prefix('$') {
            if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

// ---------------------------------------------------------------- migrations

const SKIP_DIRS: [&str; 10] = [
    ".git",
    "node_modules",
    "target",
    ".venv",
    "venv",
    "__pycache__",
    "dist",
    "build",
    ".tox",
    "vendor",
];

/// Repo files up to a few levels deep, skipping dependency and build folders.
fn walk(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut stack = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if p.is_dir() && depth < max_depth && !SKIP_DIRS.contains(&name.as_str()) {
                stack.push((p.clone(), depth + 1));
            }
            out.push(p);
        }
    }
    out.sort();
    out
}

struct Found {
    tool: &'static str,
    dirs: Vec<String>,
    /// File or folder that marks where the tool runs from.
    anchor: PathBuf,
    /// Command run inside the anchor's directory.
    command: String,
}

fn detect_migrations(
    root: &Path,
    resolved: &ComposeModel,
    db_service: &str,
    app_services: &[String],
    notes: &mut Vec<String>,
) -> Result<Migrations> {
    let files = walk(root, 4);
    let rel = |p: &Path| relative(root, p);
    let named = |n: &'static str| {
        files
            .iter()
            .filter(move |p| p.file_name().is_some_and(|f| f == n))
    };

    let found = if let Some(ini) = named("alembic.ini").next() {
        let dir = ini.parent().unwrap_or(root);
        let text = std::fs::read_to_string(ini).unwrap_or_default();
        let script = text
            .lines()
            .find_map(|l| {
                l.trim()
                    .strip_prefix("script_location")
                    .map(|r| r.trim_start_matches([' ', '=']))
            })
            .map(|s| s.trim().replace("%(here)s/", "").replace("%(here)s", "."))
            .unwrap_or_else(|| "alembic".into());
        Some(Found {
            tool: "alembic",
            dirs: vec![rel(&dir
                .join(script.trim_start_matches("./"))
                .join("versions"))],
            anchor: ini.clone(),
            command: "alembic upgrade head".into(),
        })
    } else if let Some(manage) = named("manage.py").next() {
        let mut dirs: Vec<String> = files
            .iter()
            .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n == "migrations"))
            .filter(|p| {
                std::fs::read_dir(p)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .any(|e| {
                        let n = e.file_name().to_string_lossy().into_owned();
                        n.starts_with("0001_") && n.ends_with(".py")
                    })
            })
            .map(|p| rel(p))
            .collect();
        dirs.sort();
        (!dirs.is_empty()).then(|| Found {
            tool: "django",
            dirs,
            anchor: manage.clone(),
            command: "python manage.py migrate".into(),
        })
    } else if let Some(schema) = named("schema.prisma").next() {
        let dir = schema.parent().unwrap_or(root);
        Some(Found {
            tool: "prisma",
            dirs: vec![rel(&dir.join("migrations"))],
            anchor: dir.parent().unwrap_or(root).join("package.json"),
            command: "npx prisma migrate deploy".into(),
        })
    } else if root.join("db/migrate").is_dir() {
        Some(Found {
            tool: "rails",
            dirs: vec!["db/migrate".into()],
            anchor: root.join("Gemfile"),
            command: "bin/rails db:migrate".into(),
        })
    } else if let Some(dir) = files.iter().find(|p| {
        p.is_dir()
            && p.ends_with("db/migration")
            && std::fs::read_dir(p)
                .into_iter()
                .flatten()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().starts_with('V'))
    }) {
        Some(Found {
            tool: "flyway",
            dirs: vec![rel(dir)],
            anchor: dir.clone(),
            command: "flyway migrate".into(),
        })
    } else if let Some(knex) = files.iter().find(|p| {
        p.file_name()
            .is_some_and(|f| f.to_string_lossy().starts_with("knexfile."))
    }) {
        let dir = knex.parent().unwrap_or(root);
        Some(Found {
            tool: "knex",
            dirs: vec![rel(&dir.join("migrations"))],
            anchor: knex.clone(),
            command: "npx knex migrate:latest".into(),
        })
    } else {
        files
            .iter()
            .find(|p| p.is_dir() && p.file_name().is_some_and(|n| n == "migrations"))
            .map(|d| Found {
                tool: "generic",
                dirs: vec![rel(d)],
                anchor: d.clone(),
                command: String::new(),
            })
    };

    let Some(found) = found else {
        notes.push(
            "no migrations folder found; set migrations.dirs and migrations.command by hand".into(),
        );
        return Ok(Migrations {
            tool: "generic".into(),
            dirs: vec![],
            command: None,
            service: None,
        });
    };

    if found.command.is_empty() {
        notes.push(format!(
            "{} looks like a migrations folder but the tool is unknown; set migrations.command",
            found.dirs[0]
        ));
        return Ok(Migrations {
            tool: found.tool.into(),
            dirs: found.dirs,
            command: None,
            service: None,
        });
    }

    // Flyway often runs as its own one-off container.
    if found.tool == "flyway" {
        if let Some((name, _)) = resolved.services.iter().find(|(_, s)| {
            s.image
                .as_deref()
                .is_some_and(|i| image_name(i).contains("flyway"))
        }) {
            return Ok(Migrations {
                tool: "flyway".into(),
                dirs: found.dirs,
                command: Some(format!("docker compose run --rm {name} migrate")),
                service: Some(name.clone()),
            });
        }
    }

    // Which service can see the migration files? Prefer app services.
    let anchor_dir = if found.anchor.is_dir() {
        found.anchor.clone()
    } else {
        found.anchor.parent().unwrap_or(root).to_path_buf()
    };
    let mut order: Vec<&String> = app_services.iter().collect();
    order.extend(
        resolved
            .services
            .keys()
            .filter(|n| *n != db_service && !app_services.contains(n)),
    );
    for name in order {
        let svc = &resolved.services[name];
        for (host, container) in svc.host_paths() {
            if !anchor_dir.starts_with(&host) {
                continue;
            }
            let inner = match container {
                Some(target) => {
                    let sub = anchor_dir.strip_prefix(&host).unwrap_or(Path::new(""));
                    Some(
                        PathBuf::from(target)
                            .join(sub)
                            .to_string_lossy()
                            .trim_end_matches('/')
                            .to_string(),
                    )
                }
                None => None,
            };
            let cmd = match (found.tool, &inner) {
                ("alembic", Some(dir)) => format!("alembic -c {dir}/alembic.ini upgrade head"),
                (_, Some(dir)) if svc.working_dir.as_deref() != Some(dir.as_str()) => {
                    format!("sh -c 'cd {dir} && {}'", found.command)
                }
                _ => found.command.clone(),
            };
            return Ok(Migrations {
                tool: found.tool.into(),
                dirs: found.dirs,
                command: Some(format!("docker compose exec -T {name} {cmd}")),
                service: Some(name.clone()),
            });
        }
    }
    notes.push(format!(
        "no compose service mounts {}; the migration command will run on your machine",
        relative(root, &anchor_dir)
    ));
    Ok(Migrations {
        tool: found.tool.into(),
        dirs: found.dirs,
        command: Some(found.command),
        service: None,
    })
}

impl Detection {
    /// Turn findings into a config. `user` must be known (resolve `user_file` first).
    /// With `proxy`, a DB name written directly in compose is fine: the app keeps
    /// its URL and the proxy routes it.
    pub fn to_config(&self, main_branch: Option<String>, proxy: bool) -> Result<Config> {
        let (file, variable, mut restart) = match &self.routing {
            Some(r) => (r.file.clone(), r.variable.clone(), r.services.clone()),
            None if proxy && !self.inline.is_empty() => Default::default(),
            None => bail!(
                "{}",
                self.routing_problem
                    .as_deref()
                    .unwrap_or("no routing variable found")
            ),
        };
        if proxy {
            for i in &self.inline {
                if !restart.contains(&i.service) {
                    restart.push(i.service.clone());
                }
            }
        }
        anyhow::ensure!(
            !self.migrations.dirs.is_empty(),
            "no migration folder found; set migrations.dirs"
        );
        let command = self
            .migrations
            .command
            .clone()
            .context("no migration command found; set migrations.command")?;
        let user = self.user.clone().unwrap_or_else(|| "postgres".into());
        let mut cfg = Config {
            database: DatabaseConfig {
                engine: "postgres".into(),
                service: self.db_service.clone(),
                user,
                psql: None,
            },
            routing: RoutingConfig {
                mode: "env".into(),
                file,
                variable,
                restart,
                database: None,
                listen: None,
                upstream: None,
                autostart: None,
            },
            migrations: MigrationsConfig {
                tool: self.migrations.tool.clone(),
                dirs: self.migrations.dirs.clone(),
                command,
                seed: None,
                version_table: None,
                version_column: None,
            },
            compose: ComposeConfig::default(),
            git: GitConfig { main_branch },
            snapshots: Default::default(),
        };
        if proxy {
            cfg.routing.use_proxy(&self.active_db);
        }
        Ok(cfg)
    }

    pub fn render(&self, proxy: bool) -> String {
        let mut s = String::new();
        s.push_str(&format!("Compose project:  {}\n", self.compose_project));
        s.push_str(&format!(
            "Database:         {} ({})\n",
            self.db_service, self.db_image
        ));
        s.push_str(&format!(
            "  user:           {}\n",
            self.user.as_deref().unwrap_or(if self.user_file.is_some() {
                "(from secret file)"
            } else {
                "postgres (default)"
            })
        ));
        s.push_str(&format!("  current DB:     {}\n", self.active_db));
        s.push_str(&format!(
            "App services:     {}\n",
            if self.app_services.is_empty() {
                "none".into()
            } else {
                self.app_services.join(", ")
            }
        ));
        match (&self.routing, &self.routing_problem) {
            (Some(r), _) => s.push_str(&format!(
                "DB name lives in: {} in {}  (recreate on switch: {})\n",
                r.variable,
                r.file,
                r.services.join(", ")
            )),
            (None, _) if proxy && !self.inline.is_empty() => {
                let vars: Vec<String> = self
                    .inline
                    .iter()
                    .map(|i| format!("{} ({})", i.variable, i.service))
                    .collect();
                s.push_str(&format!(
                    "DB name lives in: {} in the compose file  (fine in proxy mode)\n",
                    vars.join(", ")
                ));
            }
            (None, Some(p)) => s.push_str(&format!("DB name lives in: ✗ {p}\n")),
            (None, None) => {}
        }
        s.push_str(&format!(
            "Migrations:       {} in {}\n",
            self.migrations.tool,
            if self.migrations.dirs.is_empty() {
                "?".into()
            } else {
                self.migrations.dirs.join(", ")
            }
        ));
        s.push_str(&format!(
            "  command:        {}\n",
            self.migrations.command.as_deref().unwrap_or("?")
        ));
        for n in &self.notes {
            s.push_str(&format!("  ! {n}\n"));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(json: &str) -> ComposeModel {
        ComposeModel::from_json(json).unwrap()
    }

    fn write(root: &Path, path: &str, content: &str) {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    /// The spec's acceptance case: Postgres `db`, Alembic only inside `api`,
    /// DATABASE_URL in an env_file.
    #[test]
    fn alembic_in_container_with_env_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let r = root.display();
        write(
            root,
            "api/alembic.ini",
            "[alembic]\nscript_location = %(here)s/migrations\n",
        );
        write(root, "api/migrations/versions/a1_init.py", "");
        write(
            root,
            ".env.local",
            "DATABASE_URL=postgresql://app:s@db:5432/app_dev\n",
        );

        let resolved = model(&format!(
            r#"{{"name":"shop","services":{{
            "db":{{"image":"postgres:16-alpine","environment":{{"POSTGRES_USER":"app","POSTGRES_DB":"app_dev"}}}},
            "api":{{"build":{{"context":"{r}/api"}},"volumes":[{{"type":"bind","source":"{r}/api","target":"/app"}}],
                   "depends_on":{{"db":{{}}}},"working_dir":"/app",
                   "environment":{{"DATABASE_URL":"postgresql://app:s@db:5432/app_dev","POSTGRES_USER":"app"}}}}}}}}"#
        ));
        let env_files = model(&format!(
            r#"{{"name":"shop","services":{{
            "db":{{"image":"postgres:16-alpine"}},
            "api":{{"env_file":[{{"path":"{r}/.env.local"}}],"environment":{{"POSTGRES_USER":"app"}}}}}}}}"#
        ));
        let views = ComposeViews {
            resolved,
            env_files: env_files.clone(),
            raw: env_files,
        };

        let d = detect(root, root, &views).unwrap();
        assert_eq!(d.db_service, "db");
        assert_eq!(d.active_db, "app_dev");
        assert_eq!(d.app_services, ["api"]);
        let routing = d.routing.as_ref().unwrap();
        assert_eq!(
            (routing.file.as_str(), routing.variable.as_str()),
            (".env.local", "DATABASE_URL")
        );
        assert_eq!(d.migrations.tool, "alembic");
        assert_eq!(d.migrations.dirs, ["api/migrations/versions"]);
        assert_eq!(
            d.migrations.command.as_deref(),
            Some("docker compose exec -T api alembic -c /app/alembic.ini upgrade head")
        );
        let cfg = d.to_config(Some("main".into()), false).unwrap();
        assert_eq!(cfg.database.user, "app");
        assert_eq!(cfg.routing.restart, ["api"]);
        // Proxy mode keeps the env file, so worktrees can get their own port.
        let cfg = d.to_config(Some("main".into()), true).unwrap();
        assert_eq!(cfg.routing.database.as_deref(), Some("app_dev"));
        assert_eq!(
            (cfg.routing.file.as_str(), cfg.routing.variable.as_str()),
            (".env.local", "DATABASE_URL")
        );
    }

    #[test]
    fn interpolated_from_project_dotenv() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, ".env", "DB_NAME=shop\n");
        write(root, "migrations/001.sql", "");
        let resolved = model(
            r#"{"name":"x","services":{
            "db":{"image":"postgis/postgis:16","environment":{"POSTGRES_DB":"shop"}},
            "web":{"environment":{"DATABASE_URL":"postgres://u@db:5432/shop"}}}}"#,
        );
        let raw = model(
            r#"{"name":"x","services":{
            "db":{"image":"postgis/postgis:16"},
            "web":{"environment":{"DATABASE_URL":"postgres://u@db:5432/${DB_NAME:-shop}"}}}}"#,
        );
        let views = ComposeViews {
            resolved,
            env_files: raw.clone(),
            raw,
        };
        let d = detect(root, root, &views).unwrap();
        let routing = d.routing.unwrap();
        assert_eq!(
            (routing.file.as_str(), routing.variable.as_str()),
            (".env", "DB_NAME")
        );
        assert_eq!(d.migrations.tool, "generic");
        assert!(d.migrations.command.is_none());
    }

    #[test]
    fn literal_in_compose_file_is_a_problem() {
        let dir = tempfile::tempdir().unwrap();
        let resolved = model(
            r#"{"name":"x","services":{
            "db":{"image":"postgres"},
            "web":{"depends_on":["db"],"environment":{"DATABASE_URL":"postgres://postgres@db/postgres"}}}}"#,
        );
        let views = ComposeViews {
            resolved: resolved.clone(),
            env_files: resolved.clone(),
            raw: resolved,
        };
        write(dir.path(), "migrations/001.sql", "");
        let mut d = detect(dir.path(), dir.path(), &views).unwrap();
        d.migrations.command = Some("./migrate.sh".into());
        assert!(d.routing.is_none());
        assert!(d
            .routing_problem
            .as_deref()
            .unwrap()
            .contains("written directly in the compose file"));
        assert!(d.to_config(None, false).is_err());

        // Proxy mode doesn't need to change it.
        assert!(d
            .render(true)
            .contains("DATABASE_URL (web) in the compose file"));
        let cfg = d.to_config(None, true).unwrap();
        cfg.validate().unwrap();
        assert!(cfg.routing.is_proxy());
        assert_eq!(cfg.routing.database.as_deref(), Some("postgres"));
        assert_eq!(cfg.routing.restart, ["web"]);
        assert!(cfg.routing.file.is_empty());
    }

    #[test]
    fn django_multi_app_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "manage.py", "");
        write(root, "shop/migrations/0001_initial.py", "");
        write(root, "users/migrations/0001_initial.py", "");
        write(root, "users/migrations/__init__.py", "");
        let resolved = model(r#"{"name":"x","services":{"db":{"image":"postgres:15"}}}"#);
        let mut notes = vec![];
        let m = detect_migrations(root, &resolved, "db", &[], &mut notes).unwrap();
        assert_eq!(m.tool, "django");
        assert_eq!(m.dirs, ["shop/migrations", "users/migrations"]);
        assert_eq!(m.command.as_deref(), Some("python manage.py migrate"));
        assert!(!notes.is_empty(), "notes that it runs on the host");
    }

    #[test]
    fn picks_the_configured_postgres_among_several() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "migrations/001.sql", "");
        // "api" also uses the postgres image (for psql), and sorts first by name.
        let resolved = model(
            r#"{"name":"x","services":{
            "api":{"image":"postgres:16-alpine","depends_on":["db"],
                   "environment":{"DATABASE_URL":"postgres://app@db:5432/app_dev"}},
            "db":{"image":"postgres:16-alpine","environment":{"POSTGRES_USER":"app","POSTGRES_DB":"app_dev"}}}}"#,
        );
        let views = ComposeViews {
            resolved: resolved.clone(),
            env_files: resolved.clone(),
            raw: resolved,
        };
        let d = detect(dir.path(), dir.path(), &views).unwrap();
        assert_eq!(d.db_service, "db");
        assert!(
            d.notes.iter().any(|n| n.contains("using db")),
            "{:?}",
            d.notes
        );
    }

    #[test]
    fn image_matching() {
        assert!(is_postgres_image("postgres:16-alpine"));
        assert!(is_postgres_image("registry.local:5000/postgres"));
        assert!(is_postgres_image("timescale/timescaledb-ha:pg16"));
        assert!(!is_postgres_image("mysql:8"));
        assert_eq!(
            variable_reference("${DB_NAME:-x}").as_deref(),
            Some("DB_NAME")
        );
        assert_eq!(
            variable_reference("postgres://u@db/$DB").as_deref(),
            Some("DB")
        );
        assert_eq!(variable_reference("postgres://u@db/app"), None);
    }
}
