//! The typed zero `Type {}` survives `faber format` and `faber convert`:
//! idempotent per locale, never respelled as the synthesized `crea` call, and
//! a conversion between locales keeps the form.

use super::format_test::run_faber_format_stdout;
use std::fs;
use std::path::PathBuf;

const EN_SOURCE: &str = r#"+++
locale = "en"
+++
main {
    let tf32[2, 3] grid ← tf32[2, 3] {}
    let tensor<f32, [2, 3]> long ← tensor<f32, [2, 3]> {}
    let tensor<vector<f32, size 4>, [2]> lanes ← tensor<vector<f32, size 4>, [2]> {}
    let vector<f32, size 4> v ← vector<f32, size 4> {}
    let vf32[4] sv ← vf32[4] {}
    let mf32[2, 2] sm ← mf32[2, 2] {}
    let matrix<f32, [2, 2]> lm ← matrix<f32, [2, 2]> {}
    let vf32[2] authored ← [0.0, 0.0] ∷ vf32[2]
    print grid.shape()
    print long.shape()
    print lanes.shape()
    print v[3]
    print sv[3]
    print authored[1]
}
"#;

const LA_SOURCE: &str = r#"+++
locale = "la"
+++
incipit {
    sit tf32[2, 3] grid ← tf32[2, 3] {}
    sit tensor<f32, [2, 3]> long ← tensor<f32, [2, 3]> {}
    sit tensor<vector<f32, magnitudo 4>, [2]> lanes ← tensor<vector<f32, magnitudo 4>, [2]> {}
    sit vector<f32, magnitudo 4> v ← vector<f32, magnitudo 4> {}
    sit vf32[4] sv ← vf32[4] {}
    sit mf32[2, 2] sm ← mf32[2, 2] {}
    sit matrix<f32, [2, 2]> lm ← matrix<f32, [2, 2]> {}
    nota grid.magnitudines()
    nota long.magnitudines()
    nota lanes.magnitudines()
    nota v[3]
    nota sv[3]
}
"#;

const OPTIONAL_RANGED_LIST_SOURCE: &str = r#"+++
locale = "en"
+++
fn show(list<int, size 0…3> ∪ none xs) → void {
    print xs
}

main {}
"#;

fn temp_fixture(name: &str, source: &str) -> PathBuf {
    let path = std::env::temp_dir().join(name);
    fs::write(&path, source).expect("write typed-zero fixture");
    path
}

/// Format `source` twice and require the second pass to change nothing.
fn format_idempotent(name: &str, source: &str) -> String {
    let fixture = temp_fixture(name, source);
    let first = run_faber_format_stdout(&fixture);
    fs::write(&fixture, &first).expect("rewrite first pass");
    let second = run_faber_format_stdout(&fixture);
    let _ = fs::remove_file(&fixture);
    assert_eq!(first, second, "format(format(x)) must equal format(x)");
    first
}

/// The zero's type prints as authored: the short sugar stays short and the
/// long form stays long.
fn assert_zero_forms(output: &str, size: &str) {
    for line in [
        "grid ← tf32[2, 3] {}".to_owned(),
        "long ← tensor<f32, [2, 3]> {}".to_owned(),
        format!("lanes ← tensor<vector<f32, {size} 4>, [2]> {{}}"),
        format!("v ← vector<f32, {size} 4> {{}}"),
        "sv ← vf32[4] {}".to_owned(),
    ] {
        assert!(
            output.contains(line.as_str()),
            "typed zero `{line}` must round trip as authored:\n{output}"
        );
    }
    assert!(
        !output.contains("vacua") && !output.contains("crea"),
        "the synthesized constructor call must not leak into source:\n{output}"
    );
}

#[test]
fn format_optional_ranged_list_keeps_authored_bounds_and_is_idempotent() {
    let output = format_idempotent(
        "faber-optional-ranged-list-en.fab",
        OPTIONAL_RANGED_LIST_SOURCE,
    );
    assert!(
        output.contains("list<int, size 0…3> ∪ none"),
        "format must retain the list range beneath the optional union:\n{output}"
    );
}

#[test]
fn format_en_typed_zero_is_idempotent() {
    let output = format_idempotent("faber-typed-zero-en.fab", EN_SOURCE);
    assert_zero_forms(&output, "size");
    for line in ["sm ← mf32[2, 2] {}", "lm ← matrix<f32, [2, 2]> {}"] {
        assert!(
            output.contains(line),
            "matrix zero `{line}` must keep its spelling:\n{output}"
        );
    }
    assert!(
        output.contains("vf32[2] authored ← [0.0, 0.0] ∷ vf32[2]"),
        "an authored ascription of a zero array is not a typed zero:\n{output}"
    );
}

#[test]
fn format_la_typed_zero_is_idempotent() {
    let output = format_idempotent("faber-typed-zero-la.fab", LA_SOURCE);
    assert_zero_forms(&output, "magnitudo");
    assert!(
        output.contains("sit ") && output.contains("incipit"),
        "la format must keep the Latin surface:\n{output}"
    );
    for line in ["sm ← mf32[2, 2] {}", "lm ← matrix<f32, [2, 2]> {}"] {
        assert!(
            output.contains(line),
            "matrix zero `{line}` must keep its spelling:\n{output}"
        );
    }
}

#[test]
fn convert_en_to_la_preserves_typed_zero() {
    let fixture = temp_fixture("faber-typed-zero-convert.fab", EN_SOURCE);
    let fixture_arg = fixture.to_str().expect("utf8 path");
    let latin = super::format_test::run_faber_format_stdout_with_args(&[
        "convert",
        "--to",
        "la",
        "--stdout",
        fixture_arg,
    ]);
    fs::write(&fixture, &latin).expect("rewrite latin fixture");
    let again = super::format_test::run_faber_format_stdout_with_args(&[
        "convert",
        "--to",
        "la",
        "--stdout",
        fixture_arg,
    ]);
    let english = super::format_test::run_faber_format_stdout_with_args(&[
        "convert",
        "--to",
        "en",
        "--stdout",
        fixture_arg,
    ]);
    let _ = fs::remove_file(&fixture);
    assert_eq!(latin, again, "la conversion must be idempotent");
    assert_zero_forms(&latin, "magnitudo");
    assert!(latin.contains("sit "), "converted to Latin:\n{latin}");
    assert_zero_forms(&english, "size");
    assert!(
        english.contains("let "),
        "converted back to English:\n{english}"
    );
}
