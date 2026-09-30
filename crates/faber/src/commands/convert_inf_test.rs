//! inf track U2 (ruling 49): `faber format` / `faber convert` keep the
//! author's spelling among the four names of the one `inf` type — bare `inf`,
//! `trapping<inf>`, `wrapping<inf>`, `saturating<inf>` and the la
//! `exactus`/`modulus`/`saturatus` twins — exactly as N7a keeps `trapping<u8>`
//! apart from `u8`. The type is identical either way; only the presentation
//! level carries the policy word.

use super::super::format_test::{
    run_faber_format_stdout, run_faber_format_stdout_with_args as run_faber,
};
use std::fs;

const EN_SOURCE: &str = r#"+++
locale = "en"
+++
fn keep(inf a, trapping<inf> b, wrapping<inf> c, saturating<inf> d) → inf {
    const inf plain ← a
    const trapping<inf> t ← b
    const wrapping<inf> w ← c
    const saturating<inf> s ← d
    const int<inf> long_form ← a
    const list<inf> items ← empty
    const list<wrapping<inf>> nested ← empty
    return plain
}
"#;

#[test]
fn convert_keeps_each_inf_spelling_across_locales() {
    let fixture = std::env::temp_dir().join("faber-convert-inf-spellings.fab");
    fs::write(&fixture, EN_SOURCE).expect("write inf fixture");
    let path = fixture.to_str().expect("utf8 fixture path");

    let latin = run_faber(&["convert", "--to", "la", "--stdout", path]);
    assert!(
        latin.contains(
            "functio keep(inf a, exactus<inf> b, modulus<inf> c, saturatus<inf> d) → inf"
        ),
        "{latin}"
    );
    assert!(latin.contains("fixum inf plain"), "{latin}");
    assert!(latin.contains("fixum exactus<inf> t"), "{latin}");
    assert!(latin.contains("fixum modulus<inf> w"), "{latin}");
    assert!(latin.contains("fixum saturatus<inf> s"), "{latin}");
    assert!(latin.contains("fixum numerus<inf> long_form"), "{latin}");
    assert!(latin.contains("fixum lista<inf> items"), "{latin}");
    assert!(
        latin.contains("fixum lista<modulus<inf>> nested"),
        "{latin}"
    );

    fs::write(&fixture, &latin).expect("rewrite latin fixture");
    let english = run_faber(&["convert", "--to", "en", "--stdout", path]);
    let _ = fs::remove_file(&fixture);
    assert!(
        english
            .contains("fn keep(inf a, trapping<inf> b, wrapping<inf> c, saturating<inf> d) → inf"),
        "{english}"
    );
    assert!(english.contains("const inf plain"), "{english}");
    assert!(english.contains("const trapping<inf> t"), "{english}");
    assert!(english.contains("const wrapping<inf> w"), "{english}");
    assert!(english.contains("const saturating<inf> s"), "{english}");
    assert!(english.contains("const int<inf> long_form"), "{english}");
    assert!(
        english.contains("const list<wrapping<inf>> nested"),
        "{english}"
    );
}

#[test]
fn format_is_a_fixed_point_on_the_inf_spellings() {
    let fixture = std::env::temp_dir().join("faber-format-inf-spellings.fab");
    fs::write(&fixture, EN_SOURCE).expect("write inf fixture");
    let once = run_faber_format_stdout(&fixture);
    fs::write(&fixture, &once).expect("rewrite formatted fixture");
    let twice = run_faber_format_stdout(&fixture);
    let _ = fs::remove_file(&fixture);
    assert_eq!(once, twice, "formatting must be idempotent");
    for spelling in [
        "inf a",
        "trapping<inf> b",
        "wrapping<inf> c",
        "saturating<inf> d",
        "const trapping<inf> t",
        "const wrapping<inf> w",
        "const saturating<inf> s",
        "const int<inf> long_form",
    ] {
        assert!(once.contains(spelling), "lost `{spelling}`:\n{once}");
    }
}
