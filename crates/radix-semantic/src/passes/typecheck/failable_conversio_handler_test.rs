//! D17.8: a failable `↦` must be handled.
//!
//! A failable conversion needs a `⊥` default, an enclosing catch (`fac/cape`
//! or `capta`), or an enclosing function that declares `⇥ E`; otherwise it is
//! `SEM011:failable_conversio_requires_handler`, the rule failable calls
//! already meet. The `incipit` (main) body itself is exempt for conversions
//! and for failable calls (an unhandled failure directly there panics on
//! every backend, fi6c). Helpers called from main, closures and
//! `praefixum` are never exempt. Conversions that
//! cannot fail (widening, constants proven to fit, `↦` into policy words) are
//! never rejected. Assertions are structured (code, issue, args).

use crate::test_support::{
    self, analyze_source_la, assert_accepts_la, assert_rejects_with_issue_la,
};

const ISSUE: &str = "failable_conversio_requires_handler";
const CALL_ISSUE: &str = "failable_call_requires_handler";
const FAILABLE_FN: &str = "functio f(numerus x) → numerus ⇥ textus {\n    redde x\n}\n\n";

fn arg<'a>(err: &'a crate::SemanticError, name: &str) -> Option<&'a str> {
    err.args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

// --- the new error -------------------------------------------------------

#[test]
fn unhandled_narrowing_in_a_plain_function_is_rejected_with_structured_facts() {
    let result = analyze_source_la("functio g(numerus x) → u8 {\n    redde x ↦ u8\n}\n");
    let err = result
        .diagnostics
        .iter()
        .find(|err| err.is_error() && test_support::issue(err) == Some(ISSUE))
        .unwrap_or_else(|| panic!("expected {ISSUE}, got {:?}", result.diagnostics));
    assert_eq!(err.kind, crate::SemanticErrorKind::OperandTypesInvalid); // SEM011
    assert_eq!(arg(err, "source"), Some("numerus"));
    assert_eq!(arg(err, "target"), Some("u8"));
    assert!(test_support::args_have_no_legacy_message(&err.args));
}

#[test]
fn every_failable_conversio_kind_is_rejected_without_a_handler() {
    for (param, target) in [
        ("textus", "numerus"),
        ("fractus", "numerus"),
        ("numerus", "u8"),
        ("u16", "u8"),
        ("d64", "i32"),
    ] {
        // Need: a float or decimal source names its rounding.
        let via = if matches!(param, "fractus" | "d64") {
            " via Trunc"
        } else {
            ""
        };
        assert_rejects_with_issue_la(
            &format!("functio g({param} x) → {target} {{\n    redde x ↦ {target}{via}\n}}\n"),
            ISSUE,
        );
    }
}

#[test]
fn registered_row_with_a_declared_channel_uses_the_same_rule() {
    // The user-row path shares the funnel: unhandled in a plain function.
    assert_rejects_with_issue_la(
        concat!(
            "genus Mensura { varia numerus valor }\n",
            "genus Positiva { varia numerus valor }\n",
            "@ conversio\n",
            "functio adPositiva(Mensura m) → Positiva ⇥ textus { redde Positiva { valor = m.valor } }\n",
            "functio g(Mensura m) → Positiva {\n    redde m ↦ Positiva\n}\n",
        ),
        ISSUE,
    );
}

// --- the three covers ----------------------------------------------------

#[test]
fn default_covers_the_conversio() {
    assert_accepts_la("functio g(numerus x) → u8 {\n    redde x ↦ u8 ⊥ 0\n}\n");
    assert_accepts_la("functio g(textus s) → numerus {\n    redde s ↦ numerus ⊥ 0\n}\n");
}

#[test]
fn fac_cape_covers_the_conversio() {
    assert_accepts_la(concat!(
        "functio g(numerus x) → numerus {\n",
        "    fac {\n        redde x ↦ u8\n    } cape err {\n        redde 0\n    }\n",
        "}\n",
    ));
}

#[test]
fn capta_covers_the_conversio() {
    assert_accepts_la(concat!(
        "functio g(textus s) → vacuum {\n",
        "    sit r ← capta { s ↦ u8 }\n    nota r\n",
        "}\n",
    ));
}

#[test]
fn enclosing_error_channel_covers_the_conversio() {
    assert_accepts_la("functio g(numerus x) → u8 ⇥ textus {\n    redde x ↦ u8\n}\n");
}

#[test]
fn a_handler_does_not_leak_past_its_boundary() {
    assert_rejects_with_issue_la(
        concat!(
            "functio g(textus s) → numerus {\n",
            "    sit r ← capta { s ↦ u8 }\n    nota r\n",
            "    sit u8 y ← s ↦ u8\n    redde 0\n",
            "}\n",
        ),
        ISSUE,
    );
}

#[test]
fn closures_do_not_inherit_the_enclosing_error_channel_or_main() {
    // A closure is its own frame: errors do not unwind through it, so it
    // needs its own handler even inside main or a `⇥` function.
    assert_rejects_with_issue_la(
        "incipit {\n    sit h ← numerus x ∴ x ↦ u8\n    nota h(1)\n}\n",
        ISSUE,
    );
    assert_rejects_with_issue_la(
        concat!(
            "functio g() → vacuum ⇥ textus {\n",
            "    sit h ← numerus x ∴ x ↦ u8\n    nota h(1)\n",
            "}\n",
        ),
        ISSUE,
    );
}

// --- main is exempt, helpers called from main are not --------------------

#[test]
fn main_body_is_exempt_for_conversions() {
    assert_accepts_la(
        "incipit {\n    sit u8 n ← 300 ↦ u8\n    sit numerus m ← \"12\" ↦ numerus\n    nota n\n    nota m\n}\n",
    );
}

#[test]
fn main_body_is_exempt_for_failable_calls() {
    // D17.8: an unhandled failable call directly in main panics (fi6c), as a
    // failable conversion there does.
    assert_accepts_la(&format!(
        "{FAILABLE_FN}incipit {{\n    sit numerus n ← f(1)\n    nota n\n}}\n"
    ));
    assert_accepts_la(&format!("{FAILABLE_FN}incipit {{\n    nota f(1)\n}}\n"));
}

#[test]
fn closures_in_main_do_not_inherit_the_call_exemption() {
    assert_rejects_with_issue_la(
        &format!("{FAILABLE_FN}incipit {{\n    sit h ← numerus x ∴ f(x)\n    nota h(1)\n}}\n"),
        CALL_ISSUE,
    );
}

#[test]
fn helper_called_from_main_is_not_exempt_for_failable_calls() {
    assert_rejects_with_issue_la(
        &format!(
            "{FAILABLE_FN}functio g() → numerus {{\n    redde f(1)\n}}\n\nincipit {{\n    nota g()\n}}\n"
        ),
        CALL_ISSUE,
    );
}

#[test]
fn helper_called_from_main_is_not_exempt() {
    let source = "functio helper(numerus x) → u8 {\n    redde x ↦ u8\n}\n\nincipit {\n    nota helper(300)\n}\n";
    assert_rejects_with_issue_la(source, ISSUE);
}

#[test]
fn a_failable_call_in_a_plain_function_still_needs_a_handler() {
    assert_rejects_with_issue_la(
        &format!("{FAILABLE_FN}functio g() → numerus {{\n    redde f(1)\n}}\n"),
        CALL_ISSUE,
    );
}

// --- conversions that cannot fail are never rejected ---------------------

#[test]
fn widening_is_exempt() {
    assert_accepts_la("functio g(u8 x) → numerus {\n    redde x ↦ numerus\n}\n");
}

#[test]
fn constant_proven_to_fit_is_exempt() {
    assert_accepts_la("functio g() → u8 {\n    redde 200 ↦ u8\n}\n");
}

#[test]
fn policy_word_targets_are_exempt() {
    assert_accepts_la(concat!(
        "functio g(numerus x) → modulus<u8> {\n",
        "    sit saturatus<u8> s ← x ↦ saturatus<u8>\n",
        "    redde x ↦ modulus<u8>\n",
        "}\n",
    ));
}

#[test]
fn infallible_text_rows_are_exempt() {
    assert_accepts_la("functio g(numerus n) → ascii {\n    redde n ↦ ascii\n}\n");
}

#[test]
fn bn_reinterpretation_is_exempt() {
    assert_accepts_la("functio bits(f32 x) → b32 {\n    redde x ↦ b32\n}\n");
}

// --- float into a policy word -----------------------------

/// A float into a `wrapping`/`saturating` word fails at NaN and ±∞, so a
/// direct `fac/cape` types its catch binding `textus` (not the `ignotum`
/// escape): the text payload is usable as text and is not a number.
#[test]
fn float_into_policy_word_types_the_catch_binding_as_textus() {
    for word in ["modulus<u8>", "saturatus<u8>"] {
        let program = |binding: &str| {
            format!(
                "functio g(fractus x) → vacuum {{\n    fac {{\n        sit {word} w ← x ↦ {word} via Trunc\n        nota w\n    }} cape err {{\n        {binding}\n    }}\n}}\n"
            )
        };
        assert_accepts_la(&program("sit textus s ← err\n        nota s"));
        let rejected = analyze_source_la(&program("sit numerus n ← err\n        nota n"));
        assert!(
            rejected.diagnostics.iter().any(|err| err.is_error()),
            "{word}: catch binding must be textus, got {:?}",
            rejected.diagnostics
        );
    }
}

/// An integer source into a policy word still cannot fail: no channel is
/// consulted and nothing needs a handler.
#[test]
fn integer_into_policy_word_needs_no_handler() {
    assert_accepts_la("functio g(numerus x) → modulus<u8> {\n    redde x ↦ modulus<u8>\n}\n");
}
