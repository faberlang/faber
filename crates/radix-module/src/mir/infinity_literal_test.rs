//! IL-1 MIR and runner pins for non-finite literals.

use super::*;
use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, AnalyzedModule, Config, Session};

fn analyze(source: &str) -> AnalyzedModule {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    crate::driver::analyze_source(&session, "infinity.fab", source, AnalyzeRequest::default())
        .expect("analysis succeeds")
}

const SOURCE: &str = r"incipit {
    fixum f32 positive ← ∞
    fixum f32 quiet ← nonnumerus
    fixum f32 negative ← -∞
    nota positive
    nota quiet
    nota negative
}";

#[test]
fn non_finite_literals_lower_to_float_constants_and_neg_instruction() {
    let unit = analyze(SOURCE);
    let dump = dump_analyzed_unit(unit.application_view()).expect("MIR lowering succeeds");
    assert!(dump.contains("const float inf"), "infinity constant missing: {dump}");
    assert!(dump.contains("const float NaN"), "NaN constant missing: {dump}");
    assert!(dump.contains("neg %"), "negative infinity must use MIR negation: {dump}");
}

#[test]
fn non_finite_literals_run_as_ieee_float_values() {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let mut host = crate::mir::BufferHost::default();
    crate::mir::run_source(&session, "infinity.fab", SOURCE, &mut host)
        .unwrap_or_else(|error| panic!("non-finite constants must run: {error:?}"));
    assert_eq!(host.stdout_lines, ["inf", "NaN", "-inf"]);
}
