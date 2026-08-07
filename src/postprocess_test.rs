use super::parse_eslint_output;

#[test]
fn eslint_output_uses_fixed_source_when_present() {
    let output = r#"[{"filePath":"main.ts","output":"const value = 1;\n"}]"#;

    let fixed = parse_eslint_output("const value=1;\n", output).expect("valid ESLint output");

    assert_eq!(fixed, "const value = 1;\n");
}

#[test]
fn eslint_output_preserves_source_when_no_fix_was_needed() {
    let source = "const value = 1;\n";
    let output = r#"[{"filePath":"main.ts","messages":[]}]"#;

    let fixed = parse_eslint_output(source, output).expect("valid ESLint output");

    assert_eq!(fixed, source);
}

#[test]
fn eslint_output_rejects_unusable_json() {
    let invalid = parse_eslint_output("source", "not json").expect_err("invalid JSON");
    assert!(invalid.contains("invalid JSON"), "{invalid}");

    let wrong_shape = parse_eslint_output("source", "{}").expect_err("wrong JSON shape");
    assert!(wrong_shape.contains("must be an array"), "{wrong_shape}");

    let wrong_output_type =
        parse_eslint_output("source", r#"[{"filePath":"main.ts","output":null}]"#)
            .expect_err("non-string fixed output");
    assert!(
        wrong_output_type.contains("output field was not a string"),
        "{wrong_output_type}"
    );
}
