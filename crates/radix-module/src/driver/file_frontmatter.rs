//! Generic TOML frontmatter for `.fab` and `.proba` source files.
//!
//! Frontmatter is driver-level metadata peeled before lexing. Any valid TOML
//! document is accepted; `faber` and other tooling read known keys via accessors.

use serde::Serialize;
use std::ops::Deref;

/// Parsed `+++` frontmatter — a generic TOML table.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FileFrontmatter(toml::Table);

impl FileFrontmatter {
    #[must_use]
    pub fn new() -> Self {
        Self(toml::Table::new())
    }

    /// Rehydrate a frontmatter table from a previously serialized TOML table
    /// (used by the FHIR package loader to restore entry frontmatter that
    /// survived package serialization as TOML text).
    #[must_use]
    pub fn from_table(table: toml::Table) -> Self {
        Self(table)
    }

    #[must_use]
    pub fn as_table(&self) -> &toml::Table {
        &self.0
    }

    #[must_use]
    pub fn group(&self) -> Option<&str> {
        self.get_str("group")
    }

    #[must_use]
    pub fn sectio(&self) -> Option<&str> {
        self.get_str("sectio")
    }

    #[must_use]
    pub fn locale(&self) -> Option<&str> {
        self.get_str("locale")
    }

    /// Returns `Some(Ok(&str))` when present and a string,
    /// `Some(Err(()))` when present but not a string,
    /// `None` when absent.
    #[must_use]
    pub fn locale_result(&self) -> Option<Result<&str, ()>> {
        match self.0.get("locale") {
            Some(toml::Value::String(s)) => Some(Ok(s.as_str())),
            Some(_) => Some(Err(())),
            None => None,
        }
    }

    #[must_use]
    pub fn build_target(&self) -> Option<&str> {
        self.table_str("build", "target")
    }

    #[must_use]
    pub fn build_kind(&self) -> Option<&str> {
        self.table_str("build", "kind")
    }

    #[must_use]
    pub fn package_name(&self) -> Option<&str> {
        self.table_str("package", "name")
    }

    #[must_use]
    pub fn package_version(&self) -> Option<&str> {
        self.table_str("package", "version")
    }

    #[must_use]
    pub fn paths_source(&self) -> Option<&str> {
        self.table_str("paths", "source")
    }

    #[must_use]
    pub fn paths_entry(&self) -> Option<&str> {
        self.table_str("paths", "entry")
    }

    #[must_use]
    pub fn dependentia_via(&self, name: &str) -> Option<&str> {
        self.nested_table_str(&["dependentia", name], "via")
    }

    #[must_use]
    pub fn probanda_tags(&self) -> Vec<String> {
        self.table("probanda")
            .and_then(|table| table.get("tags"))
            .and_then(|value| value.as_array())
            .map(|tags| {
                tags.iter()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[must_use]
    pub fn probanda_first_tag(&self) -> Option<String> {
        self.probanda_tags().into_iter().next()
    }

    fn get_str(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(toml::Value::as_str)
    }

    fn table(&self, key: &str) -> Option<&toml::Table> {
        self.0.get(key).and_then(toml::Value::as_table)
    }

    fn table_str(&self, table_key: &str, field: &str) -> Option<&str> {
        self.table(table_key)
            .and_then(|table| table.get(field))
            .and_then(toml::Value::as_str)
    }

    fn nested_table_str(&self, path: &[&str], field: &str) -> Option<&str> {
        let mut table = self.table(path.first()?)?;
        for segment in &path[1..] {
            table = table.get(*segment)?.as_table()?;
        }
        table.get(field)?.as_str()
    }
}

impl Deref for FileFrontmatter {
    type Target = toml::Table;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for FileFrontmatter {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

/// Deserialize frontmatter TOML text into a generic table.
///
/// # Errors
///
/// Returns a TOML deserialization error when the input is not valid TOML.
/// Empty or whitespace-only text returns an empty [`FileFrontmatter`] without error.
pub fn parse_file_frontmatter(text: &str) -> Result<FileFrontmatter, toml::de::Error> {
    if text.trim().is_empty() {
        return Ok(FileFrontmatter::new());
    }
    toml::from_str(text).map(FileFrontmatter)
}

#[cfg(test)]
#[path = "file_frontmatter_test.rs"]
mod tests;
