#![allow(clippy::absurd_extreme_comparisons)]

use radix_hygiene::{Budgets, ScanConfig, assert_budgets};
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
    let files = radix_hygiene::collect_production_files(&cfg);
    let counts = radix_hygiene::count_budgets(&files, cfg.subtract_self_expect);
    assert_budgets(counts, BUDGETS);
}

fn count_inline(src: &str) -> usize {
    radix_hygiene::count_inline_test_modules(&radix_hygiene::scrub_rust_source(src))
}

#[test]
fn alphabetic_char_literals_do_not_swallow_following_code() {
    let source = "fn probe(input: &str) {\n    let (date, rest) = input.split_once('T').or_else(|| input.split_once('t'))?;\n    let _ = rest;\n    let _ = date;\n}\n";
    let scrubbed = radix_hygiene::scrub_rust_source(source);

    assert_eq!(scrubbed.matches("let _ =").count(), 2);
}

#[test]
fn raw_strings_with_inner_quotes_do_not_desynchronise_scrubbing() {
    // A raw string holding an odd number of inner quotes used to flip the
    // string state, hiding every later assertion in the file.
    let source = "fn a() { let _s = r#\"say \"hi\" and \" more\"#; }\nfn b() { assert!(d.message.contains(\"x\")); }\n";
    let scrubbed = radix_hygiene::scrub_rust_source(source);

    assert!(scrubbed.contains(".message.contains("));
    assert!(!scrubbed.contains("say"));
    assert_eq!(scrubbed.len(), source.len());
}

#[test]
fn raw_strings_with_backslashes_and_byte_prefix_are_blanked() {
    let source = "fn a() { let _s = r\"a\\\"; let _t = br##\"x\"# y\"##; }\nfn b() { d.message.is_empty(); }\n";
    let scrubbed = radix_hygiene::scrub_rust_source(source);

    assert!(scrubbed.contains(".message.is_empty("));
    assert!(!scrubbed.contains(" y"));
    assert!(scrubbed.contains("let _t"));
}

#[test]
fn identifiers_ending_in_r_or_b_are_not_raw_string_prefixes() {
    let source = "fn a(bar: &str, car: u8) { let _x = bar; let _y = \"s\"; }\nfn b() { d.message.is_empty(); }\n";
    let scrubbed = radix_hygiene::scrub_rust_source(source);

    assert!(scrubbed.contains("bar: &str"));
    assert!(scrubbed.contains(".message.is_empty("));
}

#[test]
fn nested_block_comments_are_fully_blanked() {
    let source = "/* outer /* inner */ still comment */ fn a() { d.message.is_empty(); }\n";
    let scrubbed = radix_hygiene::scrub_rust_source(source);

    assert!(!scrubbed.contains("still"));
    assert!(scrubbed.contains(".message.is_empty("));
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
        count_inline(
            "#[cfg(all(test, feature = \"foo\"))]\n#[path = \"foo_test.rs\"]\nmod tests;\n"
        ),
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
        .expect("radix-hygiene lives under crates/");
    let mut violations = Vec::new();
    collect_diagnostic_text_assertions(workspace, workspace, &mut violations);

    assert!(
        violations.is_empty(),
        "non-renderer tests must assert diagnostic code/issue/args, not message/help text:\n{}",
        violations.join("\n")
    );
}

#[test]
fn helper_locals_are_not_diagnostic_text_fields() {
    assert!(reads_diagnostic_text_field(
        "assert!(d.message.contains(\"x\"))"
    ));
    assert!(reads_diagnostic_text_field("assert_eq!(err.help,None)"));
    assert!(!reads_diagnostic_text_field(
        "assert!(needs.helpers.contains(&helper))"
    ));
}

#[cfg(unix)]
#[test]
fn diagnostic_text_scan_skips_symlinks_and_agent_dirs() {
    use std::os::unix::fs::symlink;

    let base =
        std::env::temp_dir().join(format!("radix-hygiene-symlink-scan-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let root = base.join("workspace");
    let outside = base.join("outside");
    let offending = "#[test]\nfn t() { assert!(d.message.contains(\"x\")); }\n";

    fs::create_dir_all(root.join("crates/in/tests")).expect("create in-tree tests dir");
    fs::create_dir_all(root.join(".claude/worktrees/agent/tests")).expect("create .claude dir");
    fs::create_dir_all(outside.join("tests")).expect("create outside tests dir");
    fs::write(root.join("crates/in/tests/in_tree.rs"), offending).expect("write in-tree file");
    fs::write(
        root.join(".claude/worktrees/agent/tests/agent.rs"),
        offending,
    )
    .expect("write .claude file");
    fs::write(outside.join("tests/outside.rs"), offending).expect("write outside file");
    symlink(&outside, root.join("crates/linked")).expect("symlink outside directory");

    let mut violations = Vec::new();
    collect_diagnostic_text_assertions(&root, &root, &mut violations);
    let _ = fs::remove_dir_all(&base);

    assert!(
        violations
            .iter()
            .any(|line| line.starts_with("crates/in/tests/in_tree.rs")),
        "in-tree offender must still be found: {violations:?}"
    );
    assert!(
        violations
            .iter()
            .all(|line| line.starts_with("crates/in/tests/in_tree.rs")),
        "symlinked and .claude directories must not be scanned: {violations:?}"
    );
}

fn collect_diagnostic_text_assertions(root: &Path, dir: &Path, violations: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        // `DirEntry::file_type` does not follow symlinks: a symlinked
        // directory (for example a local `.claude/worktrees/hosts` link into a
        // sibling repository) is not part of this workspace and must not be
        // scanned.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| matches!(name, ".claude" | ".codex" | ".git" | "target"))
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
        let scrubbed = radix_hygiene::scrub_rust_source(&content);
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
            if reads_diagnostic_text_field(body) {
                violations.push(format!(
                    "{} contains {assertion} over diagnostic message/help field",
                    display_relative(root, path)
                ));
            }
            offset = end;
        }
    }
}

/// Whether `body` reads a diagnostic `message` or `help` field. The field name
/// must end at a boundary: a `helpers` local or method is not the `help`
/// field, and counting it hides real assertions behind a false report.
fn reads_diagnostic_text_field(body: &str) -> bool {
    for field in [".message", ".help"] {
        let mut offset = 0;
        while let Some(found) = body[offset..].find(field) {
            let end = offset + found + field.len();
            if body[end..]
                .chars()
                .next()
                .is_none_or(|ch| !(ch.is_alphanumeric() || ch == '_'))
            {
                return true;
            }
            offset = end;
        }
    }
    false
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
        "f16, f32, f64, and bf16 are fractus widths",
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
            .expect("radix-hygiene lives under crates/"),
        path,
    )
    .starts_with("crates/corpus-matrix/src/exempla_e2e/")
    {
        return true;
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    name.ends_with("_test.rs") || name.ends_with(".test.rs") || name == "test_gate.rs"
}

/// Files exempt from `non_renderer_tests_do_not_assert_diagnostic_text`.
///
/// The list only shrinks. Every entry states the structured field that is
/// missing (or the renderer contract it pins); an entry whose file no longer
/// triggers the scan is deleted, not kept as a precaution.
fn diagnostic_text_assertion_allowlisted(root: &Path, path: &Path) -> bool {
    let rel = display_relative(root, path);
    rel == "crates/radix-hygiene/tests/hygiene.rs"
        // Renderer contract: diagnostic render tests and pack-template
        // interpolation (issue-vs-code fallback, localized row resolution).
        || rel == "crates/radix-module/src/diagnostics/render_test.rs"
        || rel == "crates/radix-module/src/locale_test.rs"
        // Renderer contract (locale-pack surface): the queue/stack and lista
        // operator tests render the SEM011 template through the installed en
        // and la packs and pin the localized prose, the same pack-template
        // contract as `crates/radix-module/src/locale_test.rs`.
        || rel == "crates/radix-semantic/src/passes/typecheck/queue_stack_test.rs"
        || rel == "crates/radix-semantic/src/passes/typecheck/lista_operator_test.rs"
        // Renderer contract (PKG001 empty-message regression): pins that the
        // interface_export_unrepresentable template resolves to non-empty prose
        // in default `faber check` output; the machine identity is asserted via
        // issue() in the same test.
        || rel == "crates/radix-program/src/bounded_param_pkg001_test.rs"
        // Renderer contract (locale-pack surface): these tests render
        // diagnostics through an installed pack via `render_diagnostic_text`
        // and pin the resolved prose (keyword binding substituted for the
        // Latin spelling, placeholder completeness); the machine identity is
        // asserted via code/args in the same tests.
        || rel == "crates/radix-semantic/tests/pack_placeholder_completeness.rs"
        || rel == "crates/radix-module/src/driver/frontmatter_locale_test.rs"
        // Renderer contract (CLI output mode): pins both default expanded
        // diagnostics and the `--no-diagnostics` compact rendering.
        || rel == "crates/faber/src/commands/check_test.rs"
        // Renderer contract: `faber explain` / reference-pack resolution pins
        // the template and help text it returns.
        || rel == "crates/faber/src/diagnostic_explain_test.rs"
        || rel == "crates/faber/src/reference_test.rs"
        || rel == "crates/faber/src/reference_parse_test.rs"
        // exempla LLVM e2e classifier: `classify_builder_failure` buckets an
        // `llvm_emission_failed` diagnostic as unsupported by searching its
        // message for the category slug. Missing field: the capability
        // category as a diagnostic arg on the llvm_emission_failed diagnostic
        // (radix-program/src/llvm.rs).
        || rel == "crates/corpus-matrix/src/exempla_e2e/llvm.rs"
        // radix-air reverse-AD call graph: `CallGraphCycleError` carries
        // `functions` and `message` only. Missing field: a kind/issue
        // distinguishing cross-package call, mutual-recursion cycle and
        // unreachable-function errors. (The test moved with the AIR→MIR
        // bridge to `radix-air-bridge` in 63dd2e9f96; the carve-out follows it.)
        || rel == "crates/radix-air-bridge/src/reverse_ad_test.rs"
        // Device-program planning and kernel decomposition: `PlanMappingError`
        // carries `message` only. Missing field: a stable issue per plan
        // rejection (no kernel plan, binding maximum, conflicting recipes,
        // scalar layout, window exceeded, control flow).
        || rel == "crates/radix-mir/src/device_program_plans_test.rs"
        || rel == "crates/radix-mir/src/kernel_decomposition_test.rs"
        // HostError `Display` contract: `{code}: {message}` rendering, pinned
        // in its own file; every other HostError test asserts code and args,
        // except the native session fault carve-out below.
        || rel == "crates/radix-device-core/src/error_display_test.rs"
        // Native CPU session kernel fault: the wrapper composes the entry,
        // lane and fault reason into the fault text (`fatal_message`,
        // radix-device-cpu/src/native.rs) and the executor boundary is
        // prose-typed (`KernelResult = Result<(), String>`), so the session
        // test reads them from the message. Missing field: entry/lane/reason
        // args on the kernel-fault `HostError` (`device_kernel_fault`).
        || rel == "crates/radix-device-cpu/src/native_test.rs"
}

fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}
