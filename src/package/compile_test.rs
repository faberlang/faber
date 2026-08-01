use super::*;

#[test]
fn rust_runtime_plan_surfaces_missing_manifest_after_validation() {
    let dir = crate::package::test_support::test_temp_dir("plan-missing-manifest");
    std::fs::create_dir_all(dir.join("src")).expect("create src");
    std::fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "plan-missing"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    std::fs::write(dir.join("src/main.fab"), "incipit { }\n").expect("write entry");

    let config = Config::default();
    let resolver = crate::package::library_resolver_from_config(&config);
    let package = crate::package::analyze_package(&config, &dir).expect("analyze package");
    std::fs::remove_file(dir.join("faber.toml")).expect("delete manifest after analysis");

    let Err(diag) = rust_runtime_plan_for_package(&package, &resolver) else {
        panic!("missing manifest after validation must be a diagnostic, not a silent default");
    };
    assert!(diag
        .args
        .contains(&radix::diagnostics::DiagnosticArg::new(
            "issue",
            "package_manifest_missing_after_validation"
        )));
}

#[test]
fn compile_package_deny_warnings_suppresses_output_after_promotion() {
    let dir = crate::package::test_support::test_temp_dir("package-deny-warnings");
    std::fs::create_dir_all(dir.join("src")).expect("create src");
    std::fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "package-deny-warnings"

[paths]
source = "src"
entry = "main.fab"
"#,
    )
    .expect("write manifest");
    std::fs::write(
        dir.join("src/main.fab"),
        "incipit { fixum numerus unused ← 1 }\n",
    )
    .expect("write entry");

    let config = Config::default().with_warn_policy(radix::driver::WarnPolicy {
        deny_all_warnings: true,
        deny_codes: vec![],
    });
    let result = compile_package(&config, &dir);

    assert!(
        result.output.is_none(),
        "promoted package warnings must stop generated output"
    );
    assert!(
        result.diagnostics.iter().any(Diagnostic::is_error),
        "expected at least one promoted warning diagnostic"
    );
}

// ── ensure_go_import edge cases ────────────────────────────────────────────

#[test]
fn ensure_go_import_ignores_matching_string_literals() {
    let code = r#"package main

func main() {
	println("os")
}
"#;

    let ensured = ensure_go_import(code, "os");

    assert!(ensured.contains("import \"os\""));
    assert!(ensured.contains("println(\"os\")"));
}

#[test]
fn ensure_go_import_adds_to_existing_import_block() {
    let code = r#"package main

import (
	"fmt"
)

func main() {
	fmt.Println("hello")
}
"#;
    let ensured = ensure_go_import(code, "os");
    assert!(ensured.contains("import ("));
    assert!(ensured.contains("\"os\""));
    assert!(ensured.contains("\"fmt\""));
}

#[test]
fn ensure_go_import_converts_single_import_to_block() {
    let code = r#"package main

import "fmt"

func main() {
	fmt.Println("hello")
}
"#;
    let ensured = ensure_go_import(code, "os");
    assert!(ensured.contains("import ("));
    assert!(ensured.contains("\"fmt\""));
    assert!(ensured.contains("\"os\""));
    assert!(!ensured.contains("import \"fmt\""));
}

#[test]
fn ensure_go_import_inserts_after_package_main_when_no_imports() {
    let code = r"package main

func main() {}
";
    let ensured = ensure_go_import(code, "fmt");
    assert!(ensured.starts_with("package main"));
    assert!(ensured.contains("import \"fmt\""));
}

#[test]
fn ensure_go_import_skips_when_already_present_in_block() {
    let code = r#"package main

import (
	"os"
)

func main() {}
"#;
    let ensured = ensure_go_import(code, "os");
    assert_eq!(ensured.matches("\"os\"").count(), 1);
}

#[test]
fn ensure_go_import_handles_no_package_main() {
    let code = r"func main() {}";
    let ensured = ensure_go_import(code, "fmt");
    assert_eq!(ensured, code);
}

#[test]
fn ensure_go_import_preserves_trailing_newline() {
    let code = "package main\n\nfunc main() {}\n";
    let ensured = ensure_go_import(code, "os");
    assert!(ensured.ends_with('\n'));
}

// ── go_imports ─────────────────────────────────────────────────────────────

#[test]
fn go_imports_extracts_from_block() {
    let code = r#"
import (
	"fmt"
	"os"
)
"#;
    let imports = go_imports(code);
    assert_eq!(imports, vec!["fmt", "os"]);
}

#[test]
fn go_imports_extracts_single_line() {
    let code = r#"import "fmt""#;
    let imports = go_imports(code);
    assert_eq!(imports, vec!["fmt"]);
}

#[test]
fn go_imports_returns_empty_when_no_imports() {
    let code = "package main\nfunc main() {}";
    let imports = go_imports(code);
    assert!(imports.is_empty());
}

#[test]
fn go_imports_handles_empty_code() {
    let imports = go_imports("");
    assert!(imports.is_empty());
}

#[test]
fn go_imports_extracts_multiple_imports() {
    let code = r#"
import (
	"fmt"
	"os"
	mylib "github.com/example/mylib"
	_ "embed"
)
"#;
    let imports = go_imports(code);
    assert!(imports.contains(&"fmt".to_owned()));
    assert!(imports.contains(&"os".to_owned()));
}

#[test]
fn go_imports_ignores_commented_imports() {
    let code = r#"
// import (
// 	"fmt"
// )
"#;
    let imports = go_imports(code);
    assert!(imports.is_empty());
}

#[test]
fn go_imports_handles_blank_lines_in_block() {
    let code = r#"
import (
	"fmt"

	"os"
)
"#;
    let imports = go_imports(code);
    assert_eq!(imports, vec!["fmt", "os"]);
}

// ── go_import_path ─────────────────────────────────────────────────────────

#[test]
fn go_import_path_extracts_simple_quoted() {
    assert_eq!(go_import_path("\"fmt\""), Some("fmt"));
}

#[test]
fn go_import_path_extracts_aliased_import() {
    assert_eq!(go_import_path("mylib \"fmt\""), Some("fmt"));
}

#[test]
fn go_import_path_returns_none_for_unquoted() {
    assert_eq!(go_import_path("fmt"), None);
}

#[test]
fn go_import_path_returns_none_for_empty() {
    assert_eq!(go_import_path(""), None);
}

#[test]
fn go_import_path_handles_path_with_slashes() {
    assert_eq!(
        go_import_path("\"github.com/example/pkg\""),
        Some("github.com/example/pkg")
    );
}

// ── sorted_export_names ────────────────────────────────────────────────────

#[test]
fn sorted_export_names_returns_sorted_deduped_list() {
    let names = vec![
        "z".to_owned(),
        "a".to_owned(),
        "b".to_owned(),
        "a".to_owned(),
    ];
    let result = sorted_export_names(names);
    assert_eq!(result, vec!["a", "b", "z"]);
}

#[test]
fn sorted_export_names_handles_empty_list() {
    let result: Vec<String> = sorted_export_names(Vec::new());
    assert!(result.is_empty());
}

#[test]
fn sorted_export_names_deduplicates_exact_duplicates() {
    let names = vec!["x".to_owned(), "x".to_owned(), "x".to_owned()];
    let result = sorted_export_names(names);
    assert_eq!(result, vec!["x"]);
}

#[test]
fn sorted_export_names_preserves_case_sensitive_order() {
    let names = vec!["B".to_owned(), "a".to_owned(), "C".to_owned()];
    let result = sorted_export_names(names);
    assert_eq!(result, vec!["B", "C", "a"]);
}

// ── normalize_path_buf ─────────────────────────────────────────────────────

#[test]
fn normalize_path_buf_returns_path_as_is_when_canonicalize_fails() {
    let non_existent = Path::new("/tmp/does-not-exist-faber-test-42");
    let result = normalize_path_buf(non_existent);
    assert_eq!(result, non_existent);
}

// ── allow_go_cli_dashed_rest_operands ──────────────────────────────────────

#[test]
fn allow_go_cli_dashed_rest_operands_injects_false_guard() {
    let code = r#"if strings.HasPrefix(arg, "-") {"#;
    let result = allow_go_cli_dashed_rest_operands(code);
    assert_eq!(result, r#"if strings.HasPrefix(arg, "-") && false {"#);
}

#[test]
fn allow_go_cli_dashed_rest_operands_passes_through_unmatched_code() {
    let code = r#"func main() { fmt.Println("hello") }"#;
    let result = allow_go_cli_dashed_rest_operands(code);
    assert_eq!(result, code);
}

#[test]
fn ensure_go_import_handles_empty_code() {
    let ensured = ensure_go_import("", "fmt");
    assert!(ensured.is_empty());
}

#[test]
fn go_import_path_returns_none_for_whitespace_only() {
    assert_eq!(go_import_path("   "), None);
}

// ── Stage 4: normalized lookup indexes (FBR-P2-006, FBR-P2-007) ───────────

#[test]
fn go_multi_unit_local_import_resolves_via_normalized_index() {
    // FBR-P2-006: Go local-import resolution must resolve each import edge
    // with one lookup against a per-unit normalized path index instead of an
    // O(U) scan with repeated path normalization per edge. A multi-unit
    // package whose entry imports a sibling module proves the index finds the
    // right unit: without it the namespace var for the imported binding is
    // missing (and compile fails with package_go_import_unit_missing).
    let dir = crate::package::test_support::test_temp_dir("go-multi-unit-index");
    std::fs::write(
        dir.join("main.fab"),
        r#"
importa ex "./lib" privata lib

incipit {
    nota lib.answer()
}
"#,
    )
    .expect("write entry");
    std::fs::write(
        dir.join("lib.fab"),
        r#"
functio answer() → numerus {
    redde 42
}
"#,
    )
    .expect("write lib");

    let result = compile_package(&Config::new().with_target(Target::Go), &dir.join("main.fab"));
    assert!(
        result.success(),
        "expected Go multi-unit compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Go(GoOutput { code, .. })) = result.output else {
        panic!("expected go output");
    };
    // The namespace var for the imported sibling unit is only injected when
    // the import resolves through the index to the sibling's func signatures.
    assert!(
        code.contains("var lib = struct"),
        "expected sibling namespace var in generated go code, got:\n{code}"
    );
}

#[test]
fn rust_multi_unit_sibling_exports_resolve_via_package_index() {
    // FBR-P2-007: Rust sibling export resolution must use one package-wide
    // normalized path index (built once, current unit excluded by identity)
    // instead of rebuilding a candidate map per unit. A multi-unit package
    // with a cross-module call proves the sibling set is complete and
    // deterministic: the generated entry references the imported module.
    let dir = crate::package::test_support::test_temp_dir("rust-sibling-index");
    std::fs::write(
        dir.join("main.fab"),
        r#"
importa ex "./lib" privata lib

incipit {
    nota lib.answer()
}
"#,
    )
    .expect("write entry");
    std::fs::write(
        dir.join("lib.fab"),
        r#"
functio answer() → numerus {
    redde 42
}
"#,
    )
    .expect("write lib");

    let result = compile_package(&Config::default(), &dir.join("main.fab"));
    assert!(
        result.success(),
        "expected Rust multi-unit compile success, got {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let Some(Output::Rust(output)) = result.output else {
        panic!("expected rust output");
    };
    assert!(
        output.code.contains("pub mod lib"),
        "expected sibling module declaration in generated rust code"
    );
}
