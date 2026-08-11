//! Shared path-dependency extraction for core-support validation.
//!
//! Both assembly-time validation (Fix A) and materialization-time verification
//! (Fix B) need to extract `path = "..."` dependencies from bundled `Cargo.toml`
//! files and check that the targets resolve. This module owns that extraction so
//! the two consumers share one parser.
//!
//! Uses a line-based scanner instead of the `toml` crate so it works in
//! `build.rs` (which does not have `toml` as a dependency).

use std::path::{Path, PathBuf};

/// One path dependency declared in a `Cargo.toml` `[dependencies]` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathDep {
    /// The Cargo.toml file that declared the dependency (relative to the root).
    pub manifest: PathBuf,
    /// The raw path string as declared in the manifest.
    pub raw_path: String,
}

/// Extract every runtime/build `path = "..."` dependency from a `Cargo.toml`
/// source string.
///
/// `manifest_relative` is the path of the Cargo.toml relative to the archive
/// root (e.g. `"faber-runtime/Cargo.toml"`). The returned `PathDep::raw_path`
/// values are the declared path strings, unresolved.
///
/// Uses a line-based scan that handles both `key = { path = "..." }` (inline
/// table) and multiline table forms. This intentionally avoids the `toml` crate
/// so the function works in `build.rs`.
#[must_use]
pub fn extract_path_deps(manifest_relative: &str, source: &str) -> Vec<PathDep> {
    let mut deps = Vec::new();
    let mut in_dependencies = false;
    for line in source.lines() {
        let trimmed = strip_toml_comment(line).trim();

        // Track whether we're inside a Cargo dependency table.
        if trimmed.starts_with('[') {
            let table = trimmed.trim_matches(|ch| ch == '[' || ch == ']');
            in_dependencies = matches!(table, "dependencies" | "build-dependencies")
                || table.ends_with(".dependencies")
                || table.ends_with(".build-dependencies");
            continue;
        }
        if !in_dependencies {
            continue;
        }

        // Look for path = "..." in the line
        if let Some(path) = extract_path_value(trimmed) {
            deps.push(PathDep {
                manifest: PathBuf::from(manifest_relative),
                raw_path: path,
            });
        }
    }
    deps
}

fn strip_toml_comment(line: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        match quote {
            Some('"') if escaped => {
                escaped = false;
            }
            Some('"') if ch == '\\' => {
                escaped = true;
            }
            Some(current) if ch == current => {
                quote = None;
            }
            Some(_) => {}
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
            }
            None if ch == '#' => {
                return &line[..idx];
            }
            None => {}
        }
    }
    line
}

/// Extract the value of `path = "..."` from a single line, if present.
fn extract_path_value(line: &str) -> Option<String> {
    // Find `path = "..."` or `path = '...'` anywhere in dependency table
    // entries, including inline table forms such as `dep = { path = "../dep" }`.
    for (idx, _) in line.match_indices("path") {
        let before_ok = line[..idx]
            .chars()
            .next_back()
            .is_none_or(|ch| !(ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'));
        if !before_ok {
            continue;
        }
        let after_key = line[idx + "path".len()..].trim_start();
        let Some(after_eq) = after_key.strip_prefix('=') else {
            continue;
        };
        let after_eq = after_eq.trim_start();
        let quote = after_eq.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let rest = &after_eq[1..];
        let end = rest.find(quote)?;
        return Some(rest[..end].to_owned());
    }
    None
}

/// Resolve a `PathDep`'s raw path relative to its manifest's parent directory.
///
/// Returns the normalized relative path, or `None` if the resolution escapes
/// above the manifest's directory (too many `..` components).
#[must_use]
pub fn resolve_against(dep: &PathDep) -> Option<PathBuf> {
    let manifest_dir = dep.manifest.parent().unwrap_or(Path::new(""));
    let resolved = manifest_dir.join(&dep.raw_path);
    normalize_relative(&resolved)
}

/// Normalize a relative path by collapsing `..` and `.` components.
/// Returns `None` if the path escapes its root (goes above the base).
fn normalize_relative(path: &Path) -> Option<PathBuf> {
    let mut stack: Vec<std::path::Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(name) => stack.push(std::path::Component::Normal(name)),
            std::path::Component::ParentDir => {
                stack.pop()?;
            }
            std::path::Component::CurDir => {}
            _ => return None,
        }
    }
    let mut result = PathBuf::new();
    for component in stack {
        result.push(component.as_os_str());
    }
    Some(result)
}

/// Check whether `target` falls within any of the `roots`.
///
/// Dead-code allowance (lib/bin crate bodies): this is exercised by the
/// core-support assembler (`assembler::validate_path_dependencies`, called
/// from `build.rs` and `core_support_test`), but the assembler module is
/// compiled only into the build script and the test harness — never into the
/// lib/bin bodies themselves — so no non-test caller exists inside the crate.
#[must_use]
#[allow(dead_code)]
pub fn is_within_roots(target: &Path, roots: &[String]) -> bool {
    let target_str = target.to_string_lossy();
    roots.iter().any(|root| {
        target_str.as_ref() == root.as_str()
            || target_str.starts_with(&format!("{root}/"))
            || target_str.starts_with(&format!("{root}\\"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_path_deps_from_simple_manifest() {
        let source = r#"
[dependencies]
faber-runtime = { path = "../faber-runtime" }
tokio = "1"
radix-runtime-contract = { path = "../radix/crates/radix-runtime-contract" }
"#;
        let deps = extract_path_deps("hosts/crates/host-kernel/Cargo.toml", source);
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].raw_path, "../faber-runtime");
        assert_eq!(deps[1].raw_path, "../radix/crates/radix-runtime-contract");
    }

    #[test]
    fn extracts_path_deps_from_target_and_build_tables() {
        let source = r#"
[build-dependencies]
builder = { path = "../builder" }

[target.'cfg(unix)'.dependencies]
unix-only = { path = "../unix-only" }

[package.metadata]
fixture = { path = "../not-a-dep" }
"#;
        let deps = extract_path_deps("hosts/crates/host-kernel/Cargo.toml", source);
        assert_eq!(
            deps.iter()
                .map(|dep| dep.raw_path.as_str())
                .collect::<Vec<_>>(),
            vec!["../builder", "../unix-only"]
        );
    }

    #[test]
    fn ignores_commented_path_dependencies() {
        let source = r#"
[dependencies]
# disabled = { path = "../disabled" }
active = { path = "../active" } # trailing comment
"#;
        let deps = extract_path_deps("hosts/crates/host-kernel/Cargo.toml", source);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].raw_path, "../active");
    }

    #[test]
    fn ignores_dev_path_dependencies() {
        let source = r#"
[dependencies]
runtime = { path = "../runtime" }

[dev-dependencies]
fixture = { path = "../fixture" }

[target.'cfg(test)'.dev-dependencies]
target_fixture = { path = "../target-fixture" }
"#;
        let deps = extract_path_deps("faber-runtime/Cargo.toml", source);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].raw_path, "../runtime");
    }

    #[test]
    fn resolves_relative_paths_correctly() {
        let dep = PathDep {
            manifest: PathBuf::from("faber-runtime/Cargo.toml"),
            raw_path: "../radix/crates/radix-runtime-contract".to_owned(),
        };
        // The fixture path always resolves below the manifest root; assert the
        // full result so a resolution failure reports the observed Option.
        let resolved = resolve_against(&dep);
        assert_eq!(
            resolved,
            Some(PathBuf::from("radix/crates/radix-runtime-contract"))
        );
    }

    #[test]
    fn detects_within_roots() {
        let roots = vec!["radix/crates/radix-runtime-contract".to_owned()];
        assert!(is_within_roots(
            Path::new("radix/crates/radix-runtime-contract"),
            &roots
        ));
        assert!(is_within_roots(
            Path::new("radix/crates/radix-runtime-contract/src/lib.rs"),
            &roots
        ));
        assert!(!is_within_roots(Path::new("faber-runtime"), &roots));
    }

    #[test]
    fn returns_empty_for_manifest_without_deps() {
        let deps = extract_path_deps("foo/Cargo.toml", "[package]\nname = \"foo\"\n");
        assert!(deps.is_empty());
    }
}
