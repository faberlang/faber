use super::*;

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
    let code = r#"package main

func main() {}
"#;
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
    let code = r#"func main() {}"#;
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
    let names = vec!["z".to_owned(), "a".to_owned(), "b".to_owned(), "a".to_owned()];
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
    assert_eq!(
        result,
        r#"if strings.HasPrefix(arg, "-") && false {"#
    );
}

#[test]
fn allow_go_cli_dashed_rest_operands_passes_through_unmatched_code() {
    let code = r#"func main() { fmt.Println("hello") }"#;
    let result = allow_go_cli_dashed_rest_operands(code);
    assert_eq!(result, code);
}
