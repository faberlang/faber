//! C-D1a: the codegen-owned analysis trait answers from the driver's analyzed
//! unit without copying facts.

use super::{AnalyzeRequest, Config, Session, analyze_source};
use crate::codegen::input::CodegenAnalysis;

#[test]
fn analyzed_module_answers_codegen_analysis_from_its_own_arenas() {
    let session = Session::new(Config::default().with_latin_locale());
    let source = "functio septem() → numerus { redde 7 }";
    let module = analyze_source(&session, "codegen_analysis.fab", source, AnalyzeRequest::default())
        .unwrap_or_else(|diagnostics| panic!("analysis failed: {diagnostics:?}"));

    assert!(std::ptr::eq(CodegenAnalysis::hir(&module), &module.hir));
    let diagnostics = CodegenAnalysis::diagnostics(&module);
    assert_eq!(diagnostics.len(), module.diagnostics.len());
    assert!(std::ptr::eq(diagnostics.as_ptr(), module.diagnostics.as_ptr()));
}
