//! Pass-2 validation of the `¶` format operator (DECISIONS D2.1–D2.5).
//!
//! The spec is checked against the left side's type: numbers take the full
//! pattern, `textus` takes fill/align/width/truncate, `instans` takes only the
//! named presets. `¶` is closed to built-in types; `d64` takes the float
//! parts, the radix kinds stay refused and `e` is admitted (operator ruling
//! 2026-10-17).

use crate::test_support;
use radix_diagnostics::DiagnosticArg;

#[test]
fn format_yields_textus() {
    test_support::assert_accepts(
        "functio f(numerus n, fractus x, textus s) → textus { redde (n ¶ \".2\") + (x ¶ \"08.3\") + (s ¶ \"*^9.3\") }",
    );
}

#[test]
fn integer_kinds_and_sign_are_accepted() {
    test_support::assert_accepts(
        "functio f(numerus n, u8 b) → textus { redde (n ¶ \"+08x\") + (b ¶ \"b\") + (n ¶ \".1e\") }",
    );
}

#[test]
fn instans_presets_are_accepted() {
    test_support::assert_accepts(
        "functio f(instans t, instans<ms> m) → textus { redde (t ¶ \"iso\") + (t ¶ \"date\") + (m ¶ \"time\") }",
    );
}

#[test]
fn format_result_is_not_a_number() {
    test_support::assert_rejects_with_issue(
        "functio f(numerus n) → numerus { redde n ¶ \".2\" }",
        "return_type_mismatch",
    );
}

#[test]
fn malformed_spec_is_rejected() {
    test_support::assert_rejects_with_issue(
        "functio f(numerus n) → textus { redde n ¶ \"%.2f\" }",
        "format_spec_malformed",
    );
}

#[test]
fn radix_kind_on_float_is_rejected_by_part() {
    test_support::assert_rejects_with_args(
        "functio f(fractus x) → textus { redde x ¶ \"x\" }",
        &[
            DiagnosticArg::new("issue", "format_spec_part_invalid"),
            DiagnosticArg::new("part", "kind"),
        ],
    );
}

#[test]
fn sign_on_text_is_rejected_by_part() {
    test_support::assert_rejects_with_args(
        "functio f(textus s) → textus { redde s ¶ \"+5\" }",
        &[
            DiagnosticArg::new("issue", "format_spec_part_invalid"),
            DiagnosticArg::new("part", "sign"),
        ],
    );
}

#[test]
fn precision_with_radix_kind_is_rejected() {
    test_support::assert_rejects_with_args(
        "functio f(numerus n) → textus { redde n ¶ \".2x\" }",
        &[
            DiagnosticArg::new("issue", "format_spec_part_invalid"),
            DiagnosticArg::new("part", "precision"),
        ],
    );
}

#[test]
fn layout_spec_on_instans_requires_a_preset() {
    test_support::assert_rejects_with_issue(
        "functio f(instans t) → textus { redde t ¶ \"10\" }",
        "format_spec_preset_required",
    );
}

#[test]
fn preset_on_number_is_rejected() {
    test_support::assert_rejects_with_issue(
        "functio f(numerus n) → textus { redde n ¶ \"iso\" }",
        "format_spec_preset_not_allowed",
    );
}

#[test]
fn user_type_points_to_an_ordinary_function() {
    test_support::assert_rejects_with_issue(
        "genus Punctum { varia numerus x } functio f(Punctum p) → textus { redde p ¶ \"5\" }",
        "format_type_unsupported",
    );
}

#[test]
fn bivalens_is_not_a_format_subject() {
    test_support::assert_rejects_with_issue(
        "functio f(bivalens b) → textus { redde b ¶ \"5\" }",
        "format_type_unsupported",
    );
}

#[test]
fn decimal_takes_the_float_parts() {
    test_support::assert_accepts(
        "functio f(d64 d) → textus { redde (d ¶ \".2\") + (d ¶ \"*^+12.3\") + (d ¶ \"08\") + (d ¶ \"\") + (d ¶ \"e\") + (d ¶ \".3e\") }",
    );
}

#[test]
fn decimal_rejects_every_kind_by_part() {
    for kind in ["x", "b", "o"] {
        let source = format!("functio f(d64 d) → textus {{ redde d ¶ \"{kind}\" }}");
        test_support::assert_rejects_with_args(
            &source,
            &[
                DiagnosticArg::new("issue", "format_spec_part_invalid"),
                DiagnosticArg::new("part", "kind"),
            ],
        );
    }
}

#[test]
fn decimal_zero_and_align_do_not_combine() {
    test_support::assert_rejects_with_args(
        "functio f(d64 d) → textus { redde d ¶ \"<05\" }",
        &[
            DiagnosticArg::new("issue", "format_spec_part_invalid"),
            DiagnosticArg::new("part", "zero"),
        ],
    );
}
