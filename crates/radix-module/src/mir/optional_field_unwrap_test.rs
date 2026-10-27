//! Minimal repro of triga `src/scene.fab` / `src/scene.proba` MIR validation:
//! `invalid MIR: option unwrap operand is not nullable`.
//!
//! Typecheck wraps a `sponte` field load as `T ∪ nihil`. MIR stores the field
//! as `T`. `x.field!.member` therefore lowered `option unwrap` onto a
//! non-nullable place. The scene.proba form is `moved!.parent!.index` after
//! `moved!.parent not is null`.

use super::*;
use crate::codegen::Target;
use crate::driver::AnalyzedModule;
use crate::driver::{AnalyzeRequest, Config, Session};

fn analyze(source: &str) -> AnalyzedModule {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript),
    );
    crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()).unwrap_or_else(
        |diagnostics| {
            panic!(
                "analysis succeeds: {:?}",
                diagnostics
                    .iter()
                    .map(|diagnostic| &diagnostic.message)
                    .collect::<Vec<_>>()
            )
        },
    )
}

fn lower_errors(source: &str) -> Vec<String> {
    let unit = analyze(source);
    match lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()) {
        Ok(_) => Vec::new(),
        Err(errors) => errors.into_iter().map(|error| error.message).collect(),
    }
}

fn dump_source(source: &str) -> String {
    let unit = analyze(source);
    dump_analyzed_unit(unit.application_view()).unwrap_or_else(|errors| {
        panic!(
            "MIR lowering succeeds: {:?}",
            errors
                .iter()
                .map(|error| &error.message)
                .collect::<Vec<_>>()
        )
    })
}

const SPONTE_FIELD_BANG_MEMBER: &str = r"
genus Handle { varia numerus index }
genus Node { varia Handle parent sponte }
functio cape_indicem(Node nodus) → numerus {
    redde nodus.parent!.index
}
";

const SCENE_PROBA_PARENT_BANG_INDEX: &str = r"
genus Handle { varia numerus index }
genus Node { varia Handle parent sponte }
functio cape_indicem(Node ∪ nihil nodus) → numerus {
    si nodus est nihil ergo redde 0
    si nodus!.parent est nihil ergo redde 0
    redde nodus!.parent!.index
}
";

const EXPLICIT_NULLABLE_FIELD_BANG: &str = r"
genus Handle { varia numerus index }
genus Node { varia Handle ∪ nihil parent }
functio cape_indicem(Node nodus) → numerus {
    redde nodus.parent!.index
}
";

const TRUE_OPTION_LOCAL_BANG: &str = r"
genus Handle { varia numerus index }
functio cape_indicem(Handle ∪ nihil manubrium) → numerus {
    redde manubrium!.index
}
";

#[test]
fn sponte_field_bang_member_lowers_without_unwrap_on_raw_slot() {
    let errors = lower_errors(SPONTE_FIELD_BANG_MEMBER);
    assert!(
        errors.is_empty(),
        "sponte field `!.member` must produce valid MIR; errors: {errors:?}"
    );
    let dump = dump_source(SPONTE_FIELD_BANG_MEMBER);
    assert!(
        !dump.contains("option unwrap"),
        "sponte field storage is T, so `!.member` must not unwrap: {dump}"
    );
}

#[test]
fn scene_proba_parent_bang_index_lowers_and_validates() {
    let errors = lower_errors(SCENE_PROBA_PARENT_BANG_INDEX);
    assert!(
        errors.is_empty(),
        "scene.proba `moved!.parent!.index` must produce valid MIR; errors: {errors:?}"
    );
}

#[test]
fn explicit_nullable_field_bang_still_unwraps() {
    let errors = lower_errors(EXPLICIT_NULLABLE_FIELD_BANG);
    assert!(
        errors.is_empty(),
        "T ∪ nihil field `!.member` must remain valid MIR: {errors:?}"
    );
    let dump = dump_source(EXPLICIT_NULLABLE_FIELD_BANG);
    assert!(
        dump.contains("option unwrap"),
        "true nullable field storage must still unwrap: {dump}"
    );
}

#[test]
fn true_option_local_bang_still_unwraps() {
    let errors = lower_errors(TRUE_OPTION_LOCAL_BANG);
    assert!(errors.is_empty(), "option local `!.member` must remain valid MIR: {errors:?}");
    let dump = dump_source(TRUE_OPTION_LOCAL_BANG);
    assert!(dump.contains("option unwrap"), "true option local must still unwrap: {dump}");
}
