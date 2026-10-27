//! A float literal has no width until a store gives it one. A same-as-receiver
//! float intrinsic on a literal receiver stored into a sized float cell lowers
//! to MIR that validates: the receiver takes the slot's width, so the call's
//! result type and receiver type agree.

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session};

fn dump_source(source: &str) -> String {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirTypeScript),
    );
    let unit = crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default())
        .expect("analysis succeeds");
    super::dump_analyzed_unit(unit.application_view()).expect("MIR lowering validates")
}

#[test]
fn literal_receiver_scalar_math_stored_into_f64_validates() {
    for body in [
        "sit f64 y ← ∛ 8.0",
        "sit f64 y ← (8.0).radix_cubica()",
        "sit f64 y ← (8.0).radix()",
        "sit f64 y ← ∜ 16.0",
        "sit f64 y ← (0.0).sinus()",
    ] {
        let dump = dump_source(&format!("incipit {{\n    {body}\n    nota y\n}}\n"));
        assert!(dump.contains("scalar_math"), "{body}\n{dump}");
    }
}

#[test]
fn literal_receiver_absolute_stored_into_f64_validates() {
    let dump = dump_source("incipit {\n    sit f64 y ← (-2.5).absolutum()\n    nota y\n}\n");
    assert!(dump.contains("absolutum") || dump.contains("abs"), "{dump}");
}
