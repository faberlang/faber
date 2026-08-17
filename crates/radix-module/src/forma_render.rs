//! Internal consumer-side placeholder rendering for captured `forma` templates.
//!
//! This module proves the capture invariant: template text stays raw and rendering
//! policy lives with the consumer. It is not user-facing Faber syntax.

/// SQL placeholder style for consumer rendering tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlDialect {
    Postgres,
    Sqlite,
}

/// Minimal captured `forma` value for renderer tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedForma {
    pub template: String,
    pub params: Vec<String>,
}

/// Render `§` and `§N` holes using consumer placeholder policy.
#[must_use]
pub fn render_sql(template: &str, dialect: SqlDialect) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    let mut auto_index = 1usize;
    let mut seen_indexed: Vec<usize> = Vec::new();

    while let Some(ch) = chars.next() {
        if ch != '§' {
            out.push(ch);
            continue;
        }

        let mut index_text = String::new();
        while let Some(next) = chars.peek().copied() {
            if !next.is_ascii_digit() {
                break;
            }
            index_text.push(next);
            chars.next();
        }

        let placeholder = if index_text.is_empty() {
            let current = auto_index;
            auto_index += 1;
            sql_placeholder(current, dialect)
        } else {
            let index = index_text.parse::<usize>().unwrap_or(0);
            if !seen_indexed.contains(&index) {
                seen_indexed.push(index);
            }
            let ordinal = seen_indexed
                .iter()
                .position(|value| *value == index)
                .unwrap_or(0)
                + 1;
            sql_placeholder(ordinal, dialect)
        };
        out.push_str(&placeholder);
    }

    out
}

fn sql_placeholder(ordinal: usize, dialect: SqlDialect) -> String {
    match dialect {
        SqlDialect::Postgres => format!("${ordinal}"),
        SqlDialect::Sqlite => format!("?{ordinal}"),
    }
}

#[cfg(test)]
#[path = "forma_render_test.rs"]
mod tests;
