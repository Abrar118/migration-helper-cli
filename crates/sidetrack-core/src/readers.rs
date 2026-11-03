//! Read what a migration tool says is applied, from its own version table.
//!
//! Without a reader, sidetrack infers a database's migrations from its records
//! and git (see `migrations.rs`). With one, it asks the database: Alembic's
//! `alembic_version`, Django's `django_migrations`, Flyway's
//! `flyway_schema_history`, Rails' `schema_migrations`, Prisma's
//! `_prisma_migrations`, Knex's `knex_migrations`, or a configured table.
//!
//! Rows are mapped back to migration files (`dir/name` keys, as in
//! `migrations.rs`). Rows that match no file sidetrack knows are "unknown":
//! the database holds migrations from a branch that isn't around.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::config::MigrationsConfig;
use crate::registry::FileSet;

/// How to turn version-table rows into migration files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Each row names one migration file (full name, stem, or a prefix such as a Rails version).
    Rows,
    /// Rows are `app/name` (Django). Apps without a migrations folder in the repo
    /// (Django's own `auth`, `admin`, …) are ignored.
    Django,
    /// Rows are the current head revision(s); applied = their ancestors (Alembic).
    AlembicHeads,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reader {
    pub table: String,
    /// One-column query listing the rows.
    pub query: String,
    pub kind: Kind,
}

/// What a database's version table says.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    /// Migration files the database has applied.
    pub files: FileSet,
    /// Rows that match no known migration file.
    pub unknown: Vec<String>,
}

/// Everything known about migration files across the branches sidetrack tracks.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub files: FileSet,
    /// Alembic only: revision id → (file key, down revisions).
    pub alembic: BTreeMap<String, (String, Vec<String>)>,
}

fn quote_table(table: &str) -> String {
    table
        .split('.')
        .map(crate::naming::quote_ident)
        .collect::<Vec<_>>()
        .join(".")
}

/// The reader for a migrations config, if its tool keeps a version table.
pub fn reader_for(cfg: &MigrationsConfig) -> Option<Reader> {
    let (default_table, column, filter, kind) = match cfg.tool.as_str() {
        "alembic" => ("alembic_version", "version_num", "", Kind::AlembicHeads),
        "django" => ("django_migrations", "app || '/' || name", "", Kind::Django),
        "flyway" => (
            "flyway_schema_history",
            "script",
            " WHERE success",
            Kind::Rows,
        ),
        "rails" => ("schema_migrations", "version", "", Kind::Rows),
        "prisma" => (
            "_prisma_migrations",
            "migration_name",
            " WHERE finished_at IS NOT NULL AND rolled_back_at IS NULL",
            Kind::Rows,
        ),
        "knex" => ("knex_migrations", "name", "", Kind::Rows),
        _ => ("", "", "", Kind::Rows),
    };
    let table = cfg
        .version_table
        .clone()
        .unwrap_or_else(|| default_table.to_string());
    let column = match &cfg.version_column {
        Some(c) => crate::naming::quote_ident(c),
        None => column.to_string(),
    };
    if table.is_empty() || column.is_empty() {
        return None;
    }
    Some(Reader {
        query: format!(
            "SELECT ({column})::text FROM {}{filter}",
            quote_table(&table)
        ),
        table,
        kind,
    })
}

fn basename(key: &str) -> &str {
    key.rsplit('/').next().unwrap_or(key)
}

fn stem(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 => &name[..i],
        _ => name,
    }
}

/// Does migration file `name` correspond to version-table row `row`?
fn matches_row(name: &str, row: &str) -> bool {
    let row = basename(row);
    name == row || stem(name) == row || name.starts_with(&format!("{row}_"))
}

impl Reader {
    /// Interpret version-table rows against the known migration files.
    pub fn interpret(&self, rows: &[String], catalog: &Catalog) -> Reading {
        let rows: Vec<&String> = rows
            .iter()
            .filter(|r| !r.is_empty() && !r.starts_with("<<"))
            .collect();
        let mut out = Reading::default();
        match self.kind {
            Kind::Rows => {
                for row in rows {
                    let hits: Vec<&String> = catalog
                        .files
                        .iter()
                        .filter(|k| matches_row(basename(k), row))
                        .collect();
                    if hits.is_empty() {
                        out.unknown.push(row.clone());
                    }
                    out.files.extend(hits.into_iter().cloned());
                }
            }
            Kind::Django => {
                // App label = the folder that holds `migrations/`.
                let app_of = |key: &str| key.rsplit('/').nth(2).map(str::to_string);
                let apps: BTreeSet<String> =
                    catalog.files.iter().filter_map(|k| app_of(k)).collect();
                for row in rows {
                    let Some((app, name)) = row.split_once('/') else {
                        continue;
                    };
                    if !apps.contains(app) {
                        continue; // Django's own apps, third-party packages
                    }
                    let hit = catalog
                        .files
                        .iter()
                        .find(|k| app_of(k).as_deref() == Some(app) && stem(basename(k)) == name);
                    match hit {
                        Some(k) => {
                            out.files.insert(k.clone());
                        }
                        None => out.unknown.push(row.clone()),
                    }
                }
            }
            Kind::AlembicHeads => {
                let mut stack: Vec<String> = rows.into_iter().cloned().collect();
                let mut seen = BTreeSet::new();
                while let Some(rev) = stack.pop() {
                    if !seen.insert(rev.clone()) {
                        continue;
                    }
                    match catalog.alembic.get(&rev) {
                        Some((key, downs)) => {
                            out.files.insert(key.clone());
                            stack.extend(downs.iter().cloned());
                        }
                        None => out.unknown.push(rev),
                    }
                }
                out.unknown.sort();
            }
        }
        out
    }
}

/// `revision` and `down_revision` from an Alembic migration file.
///
/// Handles `revision = 'x'`, `revision: str = "x"`, `down_revision = None`,
/// tuples for merge revisions, and tuples split over several lines.
pub fn parse_alembic(src: &str) -> Option<(String, Vec<String>)> {
    let mut revision = None;
    let mut down = None;
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim_start();
        let name = line
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .next()
            .unwrap_or("");
        if (name == "revision" || name == "down_revision")
            && line[name.len()..].trim_start().starts_with([':', '='])
        {
            if let Some(eq) = line.find('=') {
                let mut rhs = line[eq + 1..].to_string();
                // Multi-line tuple: keep reading until the parenthesis closes.
                while rhs.contains('(') && !rhs.contains(')') && i + 1 < lines.len() {
                    i += 1;
                    rhs.push_str(lines[i]);
                }
                let strings = quoted_strings(&rhs);
                if name == "revision" {
                    revision = strings.into_iter().next();
                } else {
                    down = Some(strings);
                }
            }
        }
        i += 1;
    }
    Some((revision?, down.unwrap_or_default()))
}

fn quoted_strings(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut chars = s.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if c == '#' {
            break;
        }
        if c == '\'' || c == '"' {
            let body = &s[start + 1..];
            if let Some(end) = body.find(c) {
                out.push(body[..end].to_string());
                for _ in 0..=body[..end].chars().count() {
                    chars.next();
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(tool: &str) -> MigrationsConfig {
        MigrationsConfig {
            tool: tool.into(),
            dirs: vec![],
            command: String::new(),
            seed: None,
            version_table: None,
            version_column: None,
        }
    }

    fn catalog(files: &[&str]) -> Catalog {
        Catalog {
            files: files.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn rows(r: &[&str]) -> Vec<String> {
        r.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn readers_per_tool() {
        assert_eq!(
            reader_for(&cfg("alembic")).unwrap().query,
            "SELECT (version_num)::text FROM \"alembic_version\""
        );
        assert!(reader_for(&cfg("prisma"))
            .unwrap()
            .query
            .contains("rolled_back_at IS NULL"));
        assert!(reader_for(&cfg("generic")).is_none());
        let mut c = cfg("generic");
        c.version_table = Some("public.schema_migrations".into());
        c.version_column = Some("name".into());
        assert_eq!(
            reader_for(&c).unwrap().query,
            "SELECT (\"name\")::text FROM \"public\".\"schema_migrations\""
        );
    }

    #[test]
    fn rows_match_names_stems_and_prefixes() {
        let cat = catalog(&[
            "db/migrate/20240101120000_create_users.rb",
            "db/migration/V1__init.sql",
            "prisma/migrations/20240102_add_posts",
            "migrations/001_users.sql",
        ]);
        let reader = Reader {
            table: "t".into(),
            query: String::new(),
            kind: Kind::Rows,
        };
        let r = reader.interpret(
            &rows(&[
                "20240101120000",
                "V1__init.sql",
                "20240102_add_posts",
                "001_users.sql",
                "999_gone.sql",
                "<< Flyway Baseline >>",
            ]),
            &cat,
        );
        assert_eq!(r.files.len(), 4);
        assert_eq!(r.unknown, ["999_gone.sql"]);
    }

    #[test]
    fn django_ignores_apps_outside_the_repo() {
        let cat = catalog(&[
            "shop/migrations/0001_initial.py",
            "shop/migrations/0002_price.py",
        ]);
        let reader = reader_for(&cfg("django")).unwrap();
        let r = reader.interpret(
            &rows(&["auth/0001_initial", "shop/0001_initial", "shop/0003_gone"]),
            &cat,
        );
        assert_eq!(
            r.files,
            ["shop/migrations/0001_initial.py".to_string()]
                .into_iter()
                .collect()
        );
        assert_eq!(r.unknown, ["shop/0003_gone"]);
    }

    #[test]
    fn alembic_walks_down_revisions() {
        let mut cat = Catalog::default();
        for (rev, key, downs) in [
            ("a1", "v/a1_init.py", vec![]),
            ("b2", "v/b2_users.py", vec!["a1"]),
            ("c3", "v/c3_side.py", vec!["a1"]),
            ("d4", "v/d4_merge.py", vec!["b2", "c3"]),
            ("e5", "v/e5_later.py", vec!["d4"]),
        ] {
            cat.files.insert(key.into());
            cat.alembic.insert(
                rev.into(),
                (key.into(), downs.into_iter().map(String::from).collect()),
            );
        }
        let reader = reader_for(&cfg("alembic")).unwrap();
        let r = reader.interpret(&rows(&["d4"]), &cat);
        assert_eq!(r.files.len(), 4, "{:?}", r.files);
        assert!(!r.files.contains("v/e5_later.py"));
        assert!(r.unknown.is_empty());

        let r = reader.interpret(&rows(&["zz9"]), &cat);
        assert!(r.files.is_empty());
        assert_eq!(r.unknown, ["zz9"]);
    }

    #[test]
    fn parses_alembic_files() {
        let classic = "\"\"\"add users\n\nRevision ID: b2\n\"\"\"\nrevision = 'b2'\ndown_revision = 'a1'\nbranch_labels = None\n";
        assert_eq!(
            parse_alembic(classic),
            Some(("b2".into(), vec!["a1".into()]))
        );

        let typed = "revision: str = \"b2\"\ndown_revision: Union[str, None] = None\n";
        assert_eq!(parse_alembic(typed), Some(("b2".into(), vec![])));

        let merge = "revision = 'd4'\ndown_revision = (\n    'b2',\n    'c3',\n)\n";
        assert_eq!(
            parse_alembic(merge),
            Some(("d4".into(), vec!["b2".into(), "c3".into()]))
        );

        let tuple = "revision = \"d4\"  # merge\ndown_revision = ('b2', \"c3\")\n";
        assert_eq!(
            parse_alembic(tuple),
            Some(("d4".into(), vec!["b2".into(), "c3".into()]))
        );

        assert_eq!(parse_alembic("print('hello')\n"), None);
        // `revision_x = ...` and docstring mentions aren't assignments.
        assert_eq!(
            parse_alembic("revisions = 'no'\nRevision ID: x\nrevision = 'ok'\n"),
            Some(("ok".into(), vec![]))
        );
    }
}
