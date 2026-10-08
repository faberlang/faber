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
    const tf32[2, 3] grid ← tf32[2, 3] {}
    const tensor<f32, [2, 3]> long ← tensor<f32, [2, 3]> {}
    const tensor<vector<f32, 4>, [2]> lanes ← tensor<vector<f32, 4>, [2]> {}
    const vector<f32, 4> v ← vector<f32, 4> {}
    const vf32[4] sv ← vf32[4] {}
    const mf32[2, 2] sm ← mf32[2, 2] {}
    const matrix<f32, [2, 2]> lm ← matrix<f32, [2, 2]> {}
    const vf32[2] authored ← [0.0, 0.0] ∷ vf32[2]
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
    fixum tf32[2, 3] grid ← tf32[2, 3] {}
    fixum tensor<f32, [2, 3]> long ← tensor<f32, [2, 3]> {}
    fixum tensor<vector<f32, 4>, [2]> lanes ← tensor<vector<f32, 4>, [2]> {}
    fixum vector<f32, 4> v ← vector<f32, 4> {}
    fixum vf32[4] sv ← vf32[4] {}
    fixum mf32[2, 2] sm ← mf32[2, 2] {}
    fixum matrix<f32, [2, 2]> lm ← matrix<f32, [2, 2]> {}
    nota grid.magnitudines()
    nota long.magnitudines()
    nota lanes.magnitudines()
    nota v[3]
    nota sv[3]
}
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
fn assert_zero_forms(output: &str) {
    for line in [
        "grid ← tf32[2, 3] {}",
        "long ← tensor<f32, [2, 3]> {}",
        "lanes ← tensor<vector<f32, 4>, [2]> {}",
        "v ← vector<f32, 4> {}",
        "sv ← vf32[4] {}",
    ] {
        assert!(
            output.contains(line),
            "typed zero `{line}` must round trip as authored:\n{output}"
        );
    }
    assert!(
        !output.contains("vacua") && !output.contains("crea"),
        "the synthesized constructor call must not leak into source:\n{output}"
    );
}

#[test]
fn format_en_typed_zero_is_idempotent() {
    let output = format_idempotent("faber-typed-zero-en.fab", EN_SOURCE);
    assert_zero_forms(&output);
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
    assert_zero_forms(&output);
    assert!(
        output.contains("fixum ") && output.contains("incipit"),
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
    assert_zero_forms(&latin);
    assert!(latin.contains("fixum "), "converted to Latin:\n{latin}");
    assert_zero_forms(&english);
    assert!(
        english.contains("const "),
        "converted back to English:\n{english}"
    );
}
