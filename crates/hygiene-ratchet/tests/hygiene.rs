#![allow(clippy::absurd_extreme_comparisons)]

use hygiene_ratchet::{assert_budgets, Budgets, ScanConfig};
use std::fs;
use std::path::Path;

fn config() -> ScanConfig {
    ScanConfig {
        source_roots: vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("src")],
        exclude_path_suffixes: Vec::new(),
        subtract_self_expect: true,
        check_companion_convention: false,
    }
}

const BUDGETS: Budgets = Budgets {
    unwrap: 0,
    expect: 0,
    panic: 0,
    unreachable: 0,
    todo: 0,
    unimplemented: 0,
    let_underscore: 0,
    inline_test_modules: 0,
    test_attr_in_production: 0,
};

#[test]
fn production_hygiene_budgets() {
    let cfg = config();
    let files = hygiene_ratchet::collect_production_files(&cfg);
    let counts = hygiene_ratchet::count_budgets(&files, cfg.subtract_self_expect);
    assert_budgets(counts, BUDGETS);
}

fn count_inline(src: &str) -> usize {
    hygiene_ratchet::count_inline_test_modules(&hygiene_ratchet::scrub_rust_source(src))
}

#[test]
fn alphabetic_char_literals_do_not_swallow_following_code() {
    let source = "fn probe(input: &str) {\n    let (date, rest) = input.split_once('T').or_else(|| input.split_once('t'))?;\n    let _ = rest;\n    let _ = date;\n}\n";
    let scrubbed = hygiene_ratchet::scrub_rust_source(source);

    assert_eq!(scrubbed.matches("let _ =").count(), 2);
}

#[test]
fn inline_test_modules_count_any_cfg_test_gated_name() {
    assert_eq!(count_inline("#[cfg(test)]\nmod tests {\n}\n"), 1);
    assert_eq!(count_inline("#[cfg(test)]\nmod smoke_tests {\n}\n"), 1);
    assert_eq!(count_inline("#[cfg(test)]\nmod names_unit {\n}\n"), 1);
    assert_eq!(
        count_inline("#[cfg(test)]\npub(crate) mod library_home_test_support {\n}\n"),
        1
    );
}

#[test]
fn inline_test_modules_count_cfg_all_test() {
    assert_eq!(
        count_inline("#[cfg(all(test, feature = \"foo\"))]\nmod tests {\n}\n"),
        1
    );
    assert_eq!(
        count_inline("#[cfg(all(test, not(feature = \"foo\")))]\nmod smoke_tests {\n}\n"),
        1
    );
    assert_eq!(
        count_inline(
            "#[cfg(all(\n    test,\n    any(feature = \"a\", feature = \"b\")\n))]\nmod tests {\n}\n"
        ),
        1
    );
}

#[test]
fn inline_test_modules_count_stacked_allow_attrs() {
    assert_eq!(
        count_inline("#[cfg(test)]\n#[allow(unused_imports)]\nmod names_unit {\n}\n"),
        1
    );
    assert_eq!(
        count_inline(
            "#[cfg(test)]\n#[allow(\n    clippy::float_cmp,\n    clippy::cast_possible_truncation\n)]\nmod tests {\n}\n"
        ),
        1
    );
}

#[test]
fn inline_test_modules_count_each_module_not_file() {
    let src = "#[cfg(test)]\nmod names_unit {\n}\n#[cfg(test)]\nmod validate_unit {\n}\n";
    assert_eq!(count_inline(src), 2);
}

#[test]
fn inline_test_modules_ignore_companion_semicolon_wiring() {
    assert_eq!(
        count_inline("#[cfg(test)]\n#[path = \"foo_test.rs\"]\nmod tests;\n"),
        0
    );
    assert_eq!(
        count_inline("#[cfg(all(test, feature = \"foo\"))]\n#[path = \"foo_test.rs\"]\nmod tests;\n"),
        0
    );
    assert_eq!(count_inline("#[cfg(test)]\nmod test_support;\n"), 0);
}

#[test]
fn inline_test_modules_ignore_not_test_and_literals() {
    assert_eq!(count_inline("#[cfg(not(test))]\nmod tests {\n}\n"), 0);
    assert_eq!(count_inline("mod tests {\n}\n"), 0);
    assert_eq!(
        count_inline(r##"fn probe() { let _ = "#[cfg(test)] mod tests {"; }"##),
        0
    );
}

#[test]
fn non_renderer_tests_do_not_assert_diagnostic_text() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("hygiene-ratchet lives under crates/");
    let mut violations = Vec::new();
    collect_diagnostic_text_assertions(workspace, workspace, &mut violations);

    assert!(
        violations.is_empty(),
        "non-renderer tests must assert diagnostic code/issue/args, not message/help text:\n{}",
        violations.join("\n")
    );
}

fn collect_diagnostic_text_assertions(root: &Path, dir: &Path, violations: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| matches!(name, ".codex" | ".git" | "target"))
            {
                continue;
            }
            collect_diagnostic_text_assertions(root, &path, violations);
            continue;
        }
        if !is_rust_test_file(&path) || diagnostic_text_assertion_allowlisted(root, &path) {
            continue;
        }

        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let scrubbed = hygiene_ratchet::scrub_rust_source(&content);
        let compact = scrubbed
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>();
        for pattern in [
            "assertion_message(",
            ".message.contains(",
            ".message.starts_with(",
            ".message.ends_with(",
            ".message.is_empty(",
            ".message.as_str(",
            ".message.clone(",
            ".help.contains(",
            ".help.as_deref(",
            "assert!(.message",
            "assert!(.help",
            "assert!(!.message",
            "assert!(!.help",
            "assert_eq!(.message",
            "assert_eq!(.help",
            "assert_ne!(.message",
            "assert_ne!(.help",
            "assert_eq!(diagnostic.message",
            "assert_eq!(diagnostic.help",
            "assert_eq!(diag.message",
            "assert_eq!(diag.help",
            "assert_ne!(diagnostic.message",
            "assert_ne!(diagnostic.help",
            "assert_ne!(diag.message",
            "assert_ne!(diag.help",
            ".message==",
            ".message!=",
            ".help==",
            ".help!=",
        ] {
            if compact.contains(pattern) {
                violations.push(format!(
                    "{} contains {pattern}",
                    display_relative(root, &path)
                ));
            }
        }
        collect_message_field_assertions(root, &path, &compact, violations);
        collect_diagnostic_shape_field_assertions(root, &path, &content, violations);
        collect_render_text_assertions(root, &path, &content, violations);
    }
}

fn collect_message_field_assertions(
    root: &Path,
    path: &Path,
    compact: &str,
    violations: &mut Vec<String>,
) {
    for assertion in ["assert!(", "assert_eq!(", "assert_ne!("] {
        let mut offset = 0;
        while let Some(relative_start) = compact[offset..].find(assertion) {
            let start = offset + relative_start;
            let remaining = &compact[start..];
            let end = remaining
                .find(");")
                .map_or(compact.len(), |relative_end| start + relative_end + 2);
            let body = &compact[start..end];
            if body.contains(".message") || body.contains(".help") {
                violations.push(format!(
                    "{} contains {assertion} over diagnostic message/help field",
                    display_relative(root, path)
                ));
            }
            offset = end;
        }
    }
}

fn collect_diagnostic_shape_field_assertions(
    root: &Path,
    path: &Path,
    content: &str,
    violations: &mut Vec<String>,
) {
    for assertion in ["assert!(", "assert_eq!(", "assert_ne!("] {
        let mut offset = 0;
        while let Some(relative_start) = content[offset..].find(assertion) {
            let start = offset + relative_start;
            let remaining = &content[start..];
            let end = remaining
                .find(");")
                .map_or(content.len(), |relative_end| start + relative_end + 2);
            let body = &content[start..end];
            if contains_diagnostic_shape_assertion(body) && contains_diagnostic_prose_word(body) {
                violations.push(format!(
                    "{} contains {assertion} over diagnostic shape prose",
                    display_relative(root, path)
                ));
            }
            offset = end;
        }
    }
}

fn contains_diagnostic_shape_assertion(assertion: &str) -> bool {
    assertion.contains("diagnostics.contains(\"shape=")
        || assertion.contains("diagnostics.contains(&format!(\"shape=")
        || assertion.contains("error.shape")
}

fn collect_render_text_assertions(
    root: &Path,
    path: &Path,
    content: &str,
    violations: &mut Vec<String>,
) {
    let lines = content.lines().collect::<Vec<_>>();
    for (index, line) in lines.iter().enumerate() {
        if !contains_rendered_diagnostic_text(line) {
            continue;
        }
        let allowed = index.checked_sub(2).map_or(0, |start| start)..=index;
        if allowed
            .filter_map(|line_index| lines.get(line_index))
            .any(|line| line.contains("ALLOW_DIAGNOSTIC_RENDER_TEXT"))
        {
            continue;
        }
        violations.push(format!(
            "{}:{} contains rendered diagnostic text assertion",
            display_relative(root, path),
            index + 1
        ));
    }
}

fn contains_rendered_diagnostic_text(line: &str) -> bool {
    line.contains("contains(\"error[")
        || line.contains("contains(\"warning[")
        || (line.contains(".contains(\"") && contains_diagnostic_code_token(line))
        || contains_diagnostic_shape_prose_assertion(line)
        || (line.contains("contains(\"") && contains_subprocess_diagnostic_phrase(line))
        || contains_e2e_diagnostic_metadata_text(line)
}

fn contains_diagnostic_shape_prose_assertion(line: &str) -> bool {
    let asserts_diagnostic_shape = line.contains("diagnostics.contains(\"shape=")
        || line.contains("diagnostics.contains(&format!(\"shape=")
        || line.contains("assert_eq!(error.shape,");
    asserts_diagnostic_shape && contains_diagnostic_prose_word(line)
}

fn contains_diagnostic_prose_word(line: &str) -> bool {
    [
        " requires ",
        " must ",
        " cannot ",
        " unknown ",
        " invalid ",
        " unsupported",
        " not supported",
        " not implemented",
        " mismatch",
        " reserved",
        " outside",
        " only ",
        " expected ",
        " failed",
    ]
    .iter()
    .any(|phrase| line.contains(phrase))
}

fn contains_diagnostic_code_token(line: &str) -> bool {
    [
        "LEX", "PARSE", "SEM", "WARN", "LOCALE", "PKG", "CODEGEN", "MIR", "AIR", "ABI",
    ]
    .iter()
    .any(|prefix| line.contains(prefix) && line.contains(|ch: char| ch.is_ascii_digit()))
}

fn contains_subprocess_diagnostic_phrase(line: &str) -> bool {
    [
        "import cycle detected",
        "package MIR does not yet support library imports",
        "unsafe archive entry",
        "archive package root must contain",
    ]
    .iter()
    .any(|phrase| line.contains(phrase))
}

fn contains_e2e_diagnostic_metadata_text(line: &str) -> bool {
    let trimmed = line.trim_start();
    if !(line.contains("reason_contains:")
        || line.contains(".fab\",")
        || (trimmed.starts_with('"') && trimmed.ends_with("\",")))
    {
        return false;
    }

    [
        "@ radix lane",
        "runnable CLI code generation is only implemented",
        "`faber:` kernel imports are script-mode only",
        "atomic element type must",
        "expression type mismatch",
        "unknown type",
        "unknown method",
        "protecta` is reserved",
        "rumpe outside breakable block",
        "tensor and sparsa conversio requires",
        "sparsa element type must",
        "tensor arithmetic requires",
        "f16, f32, and f64 are fractus widths",
        "Rust target does not support",
        "are not supported for this target",
    ]
    .iter()
    .any(|phrase| line.contains(phrase))
}

fn is_rust_test_file(path: &Path) -> bool {
    if path.extension().is_none_or(|ext| ext != "rs") {
        return false;
    }
    if path
        .components()
        .any(|component| component.as_os_str().to_string_lossy() == "tests")
    {
        return true;
    }
    if display_relative(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("hygiene-ratchet lives under crates/"),
        path,
    )
    .starts_with("crates/exempla/src/exempla_e2e/")
    {
        return true;
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    name.ends_with("_test.rs") || name.ends_with(".test.rs") || name == "test_gate.rs"
}

fn diagnostic_text_assertion_allowlisted(root: &Path, path: &Path) -> bool {
    let rel = display_relative(root, path);
    rel == "crates/hygiene-ratchet/tests/hygiene.rs"
        || rel == "crates/radix/src/diagnostics/render_test.rs"
        // Renderer-contract: pack template interpolation, issue-vs-code
        // fallback, and localized row resolution. Followed the crate split
        // from crates/radix/src/locale_test.rs.
        || rel == "crates/radix-module/src/locale_test.rs"
        || rel == "crates/faber/src/diagnostic_explain_test.rs"
        || rel == "crates/faber/src/explain_test.rs"
        || rel == "crates/faber-cli/src/reference_test.rs"
        || rel == "crates/faber/src/reference_parse_test.rs"
        || rel == "crates/faber/src/package_text_contract_test.rs"
        // Recomposition layout (S1-U2): the faber-cli crate moved to
        // crates/faber; the reference-test allowlist entry follows.
        || rel == "crates/faber/src/reference_test.rs"
        // S1-U2 recomposition import: faber product test files assert rendered
        // diagnostic text (message/help field assertions inherited from the
        // faber-cli surface); scheduled for issue/code migration.
        || rel == "crates/faber/src/commands/format_test.rs"
        || rel == "crates/faber/src/commands/run_test.rs"
        || rel == "crates/faber/src/package_test.rs"
        || rel == "crates/faber/src/package/cargo_test.rs"
        || rel == "crates/faber/src/package/device/device_test.rs"
        || rel == "crates/faber/src/package/device/legality_test.rs"
        || rel == "crates/faber/src/package/fhir_test.rs"
        || rel == "crates/faber/src/package/frontmatter_integration_test.rs"
        || rel == "crates/faber/src/package/frontmatter_test.rs"
        || rel == "crates/faber/src/package/host_factory_test.rs"
        || rel == "crates/faber/src/package/locale_test.rs"
        || rel == "crates/faber/src/package/lockfile_test.rs"
        || rel == "crates/faber/src/package/manifest_test.rs"
        || rel == "crates/faber/src/package/mir/nominal_test.rs"
        || rel == "crates/faber/src/package/ngab1_host_partition_test.rs"
        || rel == "crates/faber/src/package/proba_integration_test.rs"
        || rel == "crates/faber/src/package/product/product_test.rs"
        // Pre-existing e2e harness / device-emit render-text assertions.
        || rel == "crates/exempla/src/exempla_e2e/llvm.rs"
        || rel == "crates/radix-mir-metal/src/emit/tests/device.rs"
        // Residual text assertions scheduled for issue/code migration.
        || rel == "crates/radix-mir/tests/validated_mir_reject.rs"
        || rel == "crates/radix/src/driver/mod_test.rs"
        || rel == "crates/radix-air/src/reverse_ad_test.rs"
        // Pre-3R S3-era residual text assertions (device-program plan +
        // device-artifact surfaces), allowlisted for issue/code migration.
        || rel == "crates/radix-mir/src/device_program_plans_test.rs"
        || rel == "crates/radix-mir-llvm/tests/device_artifact.rs"
        // MIR-surface text assertions awaiting the standardized-error-messages
        // factory goal (docs/factory/standardized-error-messages/): the
        // underlying error types carry no stable code/issue yet. Remove each
        // entry as its tests migrate to code/issue assertions — the allowlist
        // only shrinks.
        || rel == "crates/radix-mir/src/kernel_decomposition_test.rs"
        || rel == "crates/radix-mir/src/static_shape_fold_test.rs"
        || rel == "crates/radix-mir-llvm/tests/device_mode.rs"
}

fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}
