//! Schema summaries for switch plans: which tables and columns differ between the
//! database the app uses now and the one it's about to use.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// Table name → its columns as `"name type"`, in column order.
pub type Schema = BTreeMap<String, Vec<String>>;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SchemaDiff {
    pub from_db: String,
    pub to_db: String,
    pub added_tables: Vec<String>,
    pub removed_tables: Vec<String>,
    pub changed_tables: Vec<TableChange>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableChange {
    pub table: String,
    /// `"name type"`. A type change shows up as one removed and one added column.
    pub added_columns: Vec<String>,
    pub removed_columns: Vec<String>,
}

/// What changes for the app when it moves from `from` to `to`.
pub fn diff(from_db: &str, to_db: &str, from: &Schema, to: &Schema) -> SchemaDiff {
    let mut d = SchemaDiff {
        from_db: from_db.into(),
        to_db: to_db.into(),
        ..Default::default()
    };
    for (table, cols) in to {
        match from.get(table) {
            None => d.added_tables.push(table.clone()),
            Some(old) => {
                let (a, b): (BTreeSet<&String>, BTreeSet<&String>) =
                    (old.iter().collect(), cols.iter().collect());
                let added: Vec<String> = b.difference(&a).map(|s| s.to_string()).collect();
                let removed: Vec<String> = a.difference(&b).map(|s| s.to_string()).collect();
                if !added.is_empty() || !removed.is_empty() {
                    d.changed_tables.push(TableChange {
                        table: table.clone(),
                        added_columns: added,
                        removed_columns: removed,
                    });
                }
            }
        }
    }
    d.removed_tables = from
        .keys()
        .filter(|t| !to.contains_key(*t))
        .cloned()
        .collect();
    d
}

/// `public.users` → `users`; other schemas keep their prefix.
fn short(table: &str) -> &str {
    table.strip_prefix("public.").unwrap_or(table)
}

fn list(tables: &[String], max: usize) -> String {
    let mut names: Vec<&str> = tables.iter().map(|t| short(t)).take(max).collect();
    if tables.len() > max {
        names.push("…");
    }
    names.join(", ")
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

impl SchemaDiff {
    pub fn is_empty(&self) -> bool {
        self.added_tables.is_empty()
            && self.removed_tables.is_empty()
            && self.changed_tables.is_empty()
    }

    /// One line, plus per-table column changes when `verbose`.
    pub fn render(&self, verbose: bool) -> String {
        if self.is_empty() {
            return format!("  · schema: same tables and columns as {}\n", self.from_db);
        }
        let mut parts = vec![];
        if !self.added_tables.is_empty() {
            parts.push(format!(
                "+{} ({})",
                plural(self.added_tables.len(), "table"),
                list(&self.added_tables, 4)
            ));
        }
        if !self.removed_tables.is_empty() {
            parts.push(format!(
                "−{} ({})",
                plural(self.removed_tables.len(), "table"),
                list(&self.removed_tables, 4)
            ));
        }
        if !self.changed_tables.is_empty() {
            let names: Vec<String> = self
                .changed_tables
                .iter()
                .map(|c| c.table.clone())
                .collect();
            parts.push(format!(
                "{} changed ({})",
                plural(names.len(), "table"),
                list(&names, 4)
            ));
        }
        let mut out = format!("  · schema vs {}: {}\n", self.from_db, parts.join(" · "));
        if verbose {
            for c in &self.changed_tables {
                let mut cols: Vec<String> =
                    c.added_columns.iter().map(|x| format!("+{x}")).collect();
                cols.extend(c.removed_columns.iter().map(|x| format!("−{x}")));
                out.push_str(&format!("      {}: {}\n", short(&c.table), cols.join(", ")));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(tables: &[(&str, &[&str])]) -> Schema {
        tables
            .iter()
            .map(|(t, cols)| (t.to_string(), cols.iter().map(|c| c.to_string()).collect()))
            .collect()
    }

    #[test]
    fn diffs_tables_and_columns() {
        let from = schema(&[
            ("public.users", &["id integer", "email text"]),
            ("public.legacy", &["id integer"]),
        ]);
        let to = schema(&[
            (
                "public.users",
                &["id integer", "email character varying", "name text"],
            ),
            ("public.orders", &["id integer"]),
        ]);
        let d = diff("a_db", "b_db", &from, &to);
        assert_eq!(d.added_tables, ["public.orders"]);
        assert_eq!(d.removed_tables, ["public.legacy"]);
        assert_eq!(d.changed_tables.len(), 1);
        assert_eq!(
            d.changed_tables[0].added_columns,
            ["email character varying", "name text"]
        );
        assert_eq!(d.changed_tables[0].removed_columns, ["email text"]);

        let text = d.render(true);
        assert!(
            text.contains(
                "schema vs a_db: +1 table (orders) · −1 table (legacy) · 1 table changed (users)"
            ),
            "{text}"
        );
        assert!(
            text.contains("users: +email character varying, +name text, −email text"),
            "{text}"
        );
    }

    #[test]
    fn identical_and_long_lists() {
        let s = schema(&[("public.t", &["id integer"])]);
        assert!(diff("a", "b", &s, &s)
            .render(false)
            .contains("same tables and columns as a"));
        let many: Vec<String> = (1..=6).map(|i| format!("public.t{i}")).collect();
        assert_eq!(list(&many, 4), "t1, t2, t3, t4, …");
    }
}
