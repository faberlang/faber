//! audit1-a2: WARN013 counts an argument to a `mut` (`in`) parameter as a
//! mutation of its root binding.
//!
//! A `mut` parameter is the in-place-mutation mode: the callee may rewrite the
//! argument's contents, and the argument must be mutable. A `varia` that only
//! ever feeds such a call is therefore not 'unnecessary'. A `ref` (`de`)
//! parameter is a read, so a `varia` that only feeds one still is.
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use crate::test_support;
use crate::{SemanticErrorKind, WarningKind};

const POP: &str = "functio pop_unus(varia lista<numerus> xs) → vacuum { xs.detrahe() }\n";
const PEEK: &str = "functio specta(de lista<numerus> xs) → numerus { redde xs.longitudo() }\n";

fn unnecessary_varia(source: &str) -> usize {
    test_support::analyze_source(source)
        .result
        .diagnostics
        .iter()
        .filter(|err| {
            matches!(
                err.kind,
                SemanticErrorKind::Warning(WarningKind::UnnecessaryVaria)
            )
        })
        .count()
}

#[test]
fn varia_passed_only_to_mut_param_is_not_unnecessary() {
    let source = format!(
        "{POP}incipit {{\n    varia lista<numerus> zs ← [1, 2, 3]\n    pop_unus(varia zs)\n}}"
    );
    assert_eq!(unnecessary_varia(&source), 0);
}

#[test]
fn varia_field_passed_only_to_mut_param_is_not_unnecessary() {
    let source = format!(
        "genus Cumulus {{\n    varia lista<numerus> xs\n}}\n{POP}incipit {{\n    varia Cumulus c ← Cumulus {{ xs = [1, 2] }}\n    pop_unus(varia c.xs)\n}}"
    );
    assert_eq!(unnecessary_varia(&source), 0);
}

#[test]
fn varia_passed_only_to_ref_param_is_still_unnecessary() {
    let source = format!(
        "{PEEK}incipit {{\n    varia lista<numerus> zs ← [1, 2, 3]\n    nota specta(zs)\n}}"
    );
    assert_eq!(unnecessary_varia(&source), 1);
}

#[test]
fn varia_never_passed_stays_unnecessary() {
    let source =
        format!("{POP}incipit {{\n    varia lista<numerus> zs ← [1, 2, 3]\n    nota zs\n}}");
    assert_eq!(unnecessary_varia(&source), 1);
}
