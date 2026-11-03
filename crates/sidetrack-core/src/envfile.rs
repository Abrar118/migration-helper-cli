//! Read and rewrite the database name held by one variable in a `.env` file (spec P0.7).
//!
//! Only the matching line changes. Comments, ordering, `export`, quoting and the
//! rest of a connection URL are kept as they were.

use anyhow::{bail, Result};

#[derive(Debug, Clone, PartialEq)]
pub struct EnvEntry {
    pub line: usize,
    pub key: String,
    /// Value with quotes removed.
    pub value: String,
}

/// All `KEY=value` entries, in file order.
pub fn parse(content: &str) -> Vec<EnvEntry> {
    content
        .split('\n')
        .enumerate()
        .filter_map(|(i, line)| {
            let p = parse_line(line)?;
            Some(EnvEntry { line: i, key: p.key.to_string(), value: p.unquoted() })
        })
        .collect()
}

/// The effective (last) value of `key`.
pub fn get(content: &str, key: &str) -> Option<String> {
    parse(content).into_iter().rev().find(|e| e.key == key).map(|e| e.value)
}

/// Database name inside a value: the path of a URL, or the whole value.
pub fn db_name_in_value(value: &str) -> Option<String> {
    match url_path_span(value) {
        Some((start, end)) => {
            let name = &value[start..end];
            (!name.is_empty()).then(|| name.to_string())
        }
        None if value.contains("://") => None,
        None => {
            let v = value.trim();
            (!v.is_empty()).then(|| v.to_string())
        }
    }
}

/// Same value with the database name swapped.
pub fn replace_db_name(value: &str, new_db: &str) -> Result<String> {
    match url_path_span(value) {
        Some((start, end)) => Ok(format!("{}{}{}", &value[..start], new_db, &value[end..])),
        None if value.contains("://") => bail!("the URL has no database name in its path"),
        None => Ok(new_db.to_string()),
    }
}

/// Rewrite the last definition of `key` to point at `new_db`.
/// Returns the new file content and the database name it pointed at before.
pub fn set_db_name(content: &str, key: &str, new_db: &str) -> Result<(String, String)> {
    let mut lines: Vec<String> = content.split('\n').map(str::to_string).collect();
    let Some(idx) = lines.iter().rposition(|l| parse_line(l).is_some_and(|p| p.key == key))
    else {
        bail!("variable {key} is not defined");
    };
    let line = lines[idx].clone();
    let p = parse_line(&line).expect("matched above");
    let old_value = p.unquoted();
    let Some(old_db) = db_name_in_value(&old_value) else {
        bail!("{key} has no database name in it");
    };
    if old_db.contains('$') {
        bail!("{key} takes its database name from another variable ({old_db}); point sidetrack at that variable instead");
    }
    let new_value = replace_db_name(&old_value, new_db)?;
    let new_raw = match p.quote {
        Some(q) => format!("{q}{new_value}{q}"),
        None => new_value,
    };
    lines[idx] = format!("{}{}{}", &line[..p.value_start], new_raw, &line[p.value_end..]);
    Ok((lines.join("\n"), old_db))
}

struct ParsedLine<'a> {
    key: &'a str,
    raw: &'a str,
    quote: Option<char>,
    /// Byte range of `raw` (including quotes) in the line.
    value_start: usize,
    value_end: usize,
}

impl ParsedLine<'_> {
    fn unquoted(&self) -> String {
        match self.quote {
            Some(_) => self.raw[1..self.raw.len() - 1].to_string(),
            None => self.raw.to_string(),
        }
    }
}

fn parse_line(line: &str) -> Option<ParsedLine<'_>> {
    let body = line.strip_suffix('\r').unwrap_or(line);
    let lead = body.len() - body.trim_start().len();
    let mut rest_start = lead;
    if body[lead..].starts_with("export ") {
        rest_start += "export ".len();
        rest_start += body[rest_start..].len() - body[rest_start..].trim_start().len();
    }
    let rest = &body[rest_start..];
    if rest.is_empty() || rest.starts_with('#') {
        return None;
    }
    let eq = rest.find('=')?;
    let key = rest[..eq].trim();
    if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let after_eq = rest_start + eq + 1;
    let ws = body[after_eq..].len() - body[after_eq..].trim_start().len();
    let value_start = after_eq + ws;
    let v = &body[value_start..];
    let (raw_len, quote) = match v.chars().next() {
        Some(q @ ('"' | '\'')) => match v[1..].find(q) {
            Some(close) => (close + 2, Some(q)),
            None => (v.len(), None),
        },
        _ => {
            // Unquoted: an inline comment starts at " #".
            let end = v.find(" #").unwrap_or(v.len());
            (v[..end].trim_end().len(), None)
        }
    };
    Some(ParsedLine {
        key,
        raw: &v[..raw_len],
        quote,
        value_start,
        value_end: value_start + raw_len,
    })
}

/// Byte span of the database name in a URL like `postgresql://u:p@host:5432/app?x=1`.
fn url_path_span(value: &str) -> Option<(usize, usize)> {
    let scheme_end = value.find("://")? + 3;
    let rest = &value[scheme_end..];
    let query = rest.find(['?', '#']).unwrap_or(rest.len());
    // Start after the credentials, if any, so a '/' in a password doesn't confuse us.
    let auth_from = rest[..query].rfind('@').map(|i| i + 1).unwrap_or(0);
    let slash = rest[auth_from..query].find('/')? + auth_from;
    let start = scheme_end + slash + 1;
    Some((start, scheme_end + query))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_name_from_urls_and_bare_values() {
        let cases = [
            ("postgresql://u:p@db:5432/app_dev", Some("app_dev")),
            ("postgresql+psycopg2://u:p@db/app?sslmode=disable", Some("app")),
            ("postgres://u:p%2Fx@db:5432/app#frag", Some("app")),
            ("postgres://db:5432/", None),
            ("postgres://db:5432", None),
            ("app_dev", Some("app_dev")),
            ("", None),
        ];
        for (input, want) in cases {
            assert_eq!(db_name_in_value(input).as_deref(), want, "{input}");
        }
    }

    #[test]
    fn rewrite_keeps_everything_else() {
        let content = "# db settings\r\nexport DATABASE_URL=\"postgresql://u:p@db:5432/dev_db?sslmode=disable\"  # main\r\nOTHER=1\r\n";
        let (out, old) = set_db_name(content, "DATABASE_URL", "feature_auth_db").unwrap();
        assert_eq!(old, "dev_db");
        assert_eq!(
            out,
            "# db settings\r\nexport DATABASE_URL=\"postgresql://u:p@db:5432/feature_auth_db?sslmode=disable\"  # main\r\nOTHER=1\r\n"
        );
    }

    #[test]
    fn rewrite_bare_and_single_quoted() {
        let (out, old) = set_db_name("DB_NAME = 'app'\n", "DB_NAME", "x_db").unwrap();
        assert_eq!(old, "app");
        assert_eq!(out, "DB_NAME = 'x_db'\n");
        let (out, _) = set_db_name("DB_NAME=app # c\n", "DB_NAME", "x_db").unwrap();
        assert_eq!(out, "DB_NAME=x_db # c\n");
    }

    #[test]
    fn last_definition_wins() {
        let content = "DB=a\nDB=b\n";
        assert_eq!(get(content, "DB").as_deref(), Some("b"));
        let (out, old) = set_db_name(content, "DB", "c").unwrap();
        assert_eq!((out.as_str(), old.as_str()), ("DB=a\nDB=c\n", "b"));
    }

    #[test]
    fn refuses_indirection_and_missing() {
        assert!(set_db_name("URL=postgres://db/${DB_NAME}\n", "URL", "x").is_err());
        assert!(set_db_name("A=1\n", "URL", "x").is_err());
        assert!(get("# URL=x\n", "URL").is_none());
    }
}
