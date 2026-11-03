//! Maps git branch names to database names (spec P0.1).

use sha2::{Digest, Sha256};

/// Postgres truncates identifiers longer than 63 bytes.
pub const MAX_IDENT_LEN: usize = 63;
const SUFFIX: &str = "_db";

/// The plain database name for a git branch, e.g. `feature/auth-v2` → `feature_auth_v2_db`.
///
/// Long names are truncated and get a short hash so they stay unique.
pub fn db_name_for_branch(branch: &str) -> String {
    let base = sanitize(branch);
    let plain = format!("{base}{SUFFIX}");
    if plain.len() <= MAX_IDENT_LEN {
        plain
    } else {
        hashed_db_name(branch)
    }
}

/// The collision-proof form: `<sanitized, truncated>_<6 hex chars>_db`.
///
/// Used when the name is too long, or when the plain name is already taken by another branch.
pub fn hashed_db_name(branch: &str) -> String {
    let hash = short_hash(branch);
    let base = sanitize(branch);
    // room for "_" + hash + "_db"
    let room = MAX_IDENT_LEN - 1 - hash.len() - SUFFIX.len();
    let base = truncate_ident(&base, room);
    format!("{base}_{hash}{SUFFIX}")
}

/// Name for the `id`-th snapshot of a database: `<db>__s<id>`, kept within the length limit.
pub fn snapshot_db_name(db: &str, id: i64) -> String {
    suffixed(db, &format!("__s{id}"))
}

/// Temporary name used while resetting a database.
pub fn temp_db_name(db: &str) -> String {
    suffixed(db, "__tmp")
}

fn suffixed(db: &str, suffix: &str) -> String {
    let full = format!("{db}{suffix}");
    if full.len() <= MAX_IDENT_LEN {
        return full;
    }
    let hash = short_hash(db);
    let room = MAX_IDENT_LEN - 1 - hash.len() - suffix.len();
    format!("{}_{hash}{suffix}", truncate_ident(db, room))
}

/// Lowercase, keep `[a-z0-9_]`, collapse runs of `_`, trim `_`, prefix `b_` if it starts with a digit.
pub fn sanitize(branch: &str) -> String {
    let mut out = String::with_capacity(branch.len());
    for ch in branch.chars().flat_map(char::to_lowercase) {
        let c = if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            ch
        } else {
            '_'
        };
        if c == '_' && out.ends_with('_') {
            continue;
        }
        out.push(c);
    }
    let trimmed = out.trim_matches('_');
    let mut s = if trimmed.is_empty() {
        "branch".to_string()
    } else {
        trimmed.to_string()
    };
    if s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert_str(0, "b_");
    }
    s
}

fn truncate_ident(s: &str, max: usize) -> String {
    // Sanitized names are ASCII, so byte slicing is safe.
    let cut = &s[..s.len().min(max)];
    cut.trim_end_matches('_').to_string()
}

fn short_hash(s: &str) -> String {
    let digest = Sha256::digest(s.as_bytes());
    digest.iter().take(3).map(|b| format!("{b:02x}")).collect()
}

/// Quote a Postgres identifier.
pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Quote a SQL string literal.
pub fn quote_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_examples() {
        assert_eq!(db_name_for_branch("feature/auth-v2"), "feature_auth_v2_db");
        assert_eq!(db_name_for_branch("main"), "main_db");
        assert_eq!(db_name_for_branch("123-fix"), "b_123_fix_db");
    }

    #[test]
    fn collapses_and_trims() {
        assert_eq!(
            db_name_for_branch("--Feature//Foo..Bar--"),
            "feature_foo_bar_db"
        );
        assert_eq!(db_name_for_branch("///"), "branch_db");
    }

    #[test]
    fn long_names_fit_and_are_stable() {
        let long = format!("feature/{}", "x".repeat(100));
        let a = db_name_for_branch(&long);
        assert!(a.len() <= MAX_IDENT_LEN, "{a} is {} bytes", a.len());
        assert!(a.ends_with("_db"));
        assert_eq!(a, db_name_for_branch(&long));
    }

    #[test]
    fn hashed_names_differ_for_colliding_branches() {
        assert_eq!(
            db_name_for_branch("feature/Foo"),
            db_name_for_branch("feature-foo")
        );
        assert_ne!(hashed_db_name("feature/Foo"), hashed_db_name("feature-foo"));
    }

    #[test]
    fn snapshot_names_fit() {
        let db = db_name_for_branch(&"y".repeat(200));
        let s = snapshot_db_name(&db, 12345);
        assert!(s.len() <= MAX_IDENT_LEN);
        assert!(s.ends_with("__s12345"));
        assert_eq!(snapshot_db_name("dev_db", 4), "dev_db__s4");
        assert_eq!(temp_db_name("dev_db"), "dev_db__tmp");
    }

    #[test]
    fn quoting() {
        assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(quote_literal("it's"), "'it''s'");
    }
}
