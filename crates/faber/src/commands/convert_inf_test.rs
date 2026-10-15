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
    let inf plain ← a
    let trapping<inf> t ← b
    let wrapping<inf> w ← c
    let saturating<inf> s ← d
    let inf long_form ← a
    let list<inf> items ← empty
    let list<wrapping<inf>> nested ← empty
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
    assert!(latin.contains("sit inf plain"), "{latin}");
    assert!(latin.contains("sit exactus<inf> t"), "{latin}");
    assert!(latin.contains("sit modulus<inf> w"), "{latin}");
    assert!(latin.contains("sit saturatus<inf> s"), "{latin}");
    assert!(latin.contains("sit inf long_form"), "{latin}");
    assert!(latin.contains("sit lista<inf> items"), "{latin}");
    assert!(latin.contains("sit lista<modulus<inf>> nested"), "{latin}");

    fs::write(&fixture, &latin).expect("rewrite latin fixture");
    let english = run_faber(&["convert", "--to", "en", "--stdout", path]);
    let _ = fs::remove_file(&fixture);
    assert!(
        english
            .contains("fn keep(inf a, trapping<inf> b, wrapping<inf> c, saturating<inf> d) → inf"),
        "{english}"
    );
    assert!(english.contains("let inf plain"), "{english}");
    assert!(english.contains("let trapping<inf> t"), "{english}");
    assert!(english.contains("let wrapping<inf> w"), "{english}");
    assert!(english.contains("let saturating<inf> s"), "{english}");
    assert!(english.contains("let inf long_form"), "{english}");
    assert!(
        english.contains("let list<wrapping<inf>> nested"),
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
        "let trapping<inf> t",
        "let wrapping<inf> w",
        "let saturating<inf> s",
        "let inf long_form",
    ] {
        assert!(once.contains(spelling), "lost `{spelling}`:\n{once}");
    }
}
