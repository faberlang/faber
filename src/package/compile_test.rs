use super::ensure_go_import;

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
