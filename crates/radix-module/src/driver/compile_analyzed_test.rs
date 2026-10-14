//! ST2-2.10a: [`compile_analyzed`] is a behavior-preserving extraction of
//! [`compile`]'s analysis-to-output tail. A trivial source compiled whole and
//! the same source analyzed separately, then handed to the seam, must agree
//! on output.

use super::{AnalyzeRequest, Config, ModuleCompileRequest, Session, analyze_source, compile, compile_analyzed};

#[cfg(feature = "hir-rust")]
#[test]
fn compile_analyzed_matches_compile() {
    // One source without an entry block (backend generation) and one with
    // `incipit` (the CLI-program handoff). The session pins the Latin pack
    // explicitly so the sources' reading never depends on the process-wide
    // default-code-locale switch other tests in this suite flip.
    for source in ["functio septem() → numerus { redde 7 }", "incipit { nota 1 }"] {
        let session = Session::new(Config::default().with_latin_locale());
        let name = "seam.fab";

        let whole = compile(&session, name, source, ModuleCompileRequest::default());
        let Some(whole_output) = whole.output else {
            panic!("compile produced no output for trivial source: {:?}", whole.diagnostics);
        };

        let analyzed = analyze_source(&session, name, source, AnalyzeRequest::default())
            .unwrap_or_else(|diagnostics| panic!("analysis failed: {diagnostics:?}"));
        let via_seam = compile_analyzed(&session, name, source, analyzed);
        let Some(seam_output) = via_seam.output else {
            panic!("compile_analyzed produced no output: {:?}", via_seam.diagnostics);
        };

        assert_eq!(
            crate::services::output_code(seam_output),
            crate::services::output_code(whole_output)
        );
    }
}
