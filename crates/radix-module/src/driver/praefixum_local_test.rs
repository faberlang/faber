//! A block-level `fixum T x = praefixum { … }` is evaluated at build time
//! (st1 R6/R7).
//!
//! Contract: the body runs on the MIR runner, the literal replaces the local's
//! initializer wherever the local sits (function, method, entry, inside a
//! callee reached by another constant), nested sites run innermost first, a
//! failing body is SEM064 `praefixum_failed` naming the local, and Faber
//! re-emission keeps the authored body.
#![cfg(feature = "runner")]

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, AnalyzedModule, Config, Session, analyze_source};
use crate::hir::visit::HirVisitor;
use crate::hir::{HirExpression, HirExpressionKind, HirItemKind, HirLiteral, HirLocal};
use radix_diagnostics::Diagnostic;

fn analyze_with(config: Config, source: &str) -> Result<AnalyzedModule, Vec<Diagnostic>> {
    let session = Session::new(config.with_dev_locale_root());
    analyze_source(&session, "praefixum_local.fab", source, AnalyzeRequest::default())
}

fn analyze_ok(source: &str) -> AnalyzedModule {
    analyze_with(Config::default().with_latin_locale(), source)
        .unwrap_or_else(|diagnostics| panic!("analysis failed: {diagnostics:?}"))
}

/// Every block-level local with an initializer, by name.
fn local_inits(unit: &AnalyzedModule) -> Vec<(String, HirExpression)> {
    struct Collect<'a> {
        unit: &'a AnalyzedModule,
        found: Vec<(String, HirExpression)>,
    }
    impl HirVisitor for Collect<'_> {
        fn visit_local(&mut self, local: &HirLocal) {
            if let Some(init) = &local.init {
                self.found
                    .push((self.unit.interner.resolve(local.name).to_owned(), init.clone()));
            }
            radix_hir::visit::walk_local(self, local);
        }
    }
    let mut collect = Collect { unit, found: Vec::new() };
    collect.visit_program(&unit.hir);
    collect.found
}

fn local_int(unit: &AnalyzedModule, name: &str) -> u64 {
    let inits = local_inits(unit);
    let (_, init) = inits
        .iter()
        .find(|(found, _)| found == name)
        .unwrap_or_else(|| panic!("local {name} not found"));
    match &init.kind {
        HirExpressionKind::Literal(HirLiteral::Int(v)) => *v,
        other => panic!("expected int literal for {name}, got {other:?}"),
    }
}

fn rejection(source: &str, issue: &str) -> Diagnostic {
    let diagnostics = match analyze_with(Config::default().with_latin_locale(), source) {
        Ok(unit) => panic!("expected {issue}, analysis succeeded: {:?}", unit.diagnostics),
        Err(diagnostics) => diagnostics,
    };
    diagnostics
        .iter()
        .find(|d| d.code == Some("SEM064") && d.issue() == Some(issue))
        .cloned()
        .unwrap_or_else(|| panic!("expected SEM064 {issue}, got {diagnostics:?}"))
}

const HELPER: &str = "functio septem() → numerus {\n    redde 7\n}\n";

#[test]
fn entry_local_evaluates_through_a_callee() {
    let unit = analyze_ok(&format!(
        "{HELPER}incipit {{\n    fixum numerus x = praefixum {{\n        redde septem()\n    }}\n    nota x\n}}\n"
    ));
    assert_eq!(local_int(&unit, "x"), 7);
}

#[test]
fn underscore_local_takes_its_type_from_the_body() {
    let unit = analyze_ok(&format!(
        "{HELPER}incipit {{\n    fixum _ x = praefixum {{\n        redde septem() + 1\n    }}\n    nota x\n}}\n"
    ));
    assert_eq!(local_int(&unit, "x"), 8);
}

#[test]
fn local_reads_a_module_constant_evaluated_before_it() {
    let unit = analyze_ok(
        "fixum numerus LIMES = praefixum {\n    redde 6\n}\n\
         incipit {\n    fixum numerus a = praefixum {\n        redde LIMES * 2\n    }\n    nota a\n}\n",
    );
    assert_eq!(local_int(&unit, "a"), 12);
}

#[test]
fn nested_sites_run_innermost_first() {
    let unit = analyze_ok(
        "incipit {\n    fixum numerus a = praefixum {\n        fixum numerus b = praefixum {\n            redde 2\n        }\n        redde b + 1\n    }\n    nota a\n}\n",
    );
    assert_eq!(local_int(&unit, "a"), 3);
}

#[test]
fn method_local_is_evaluated_and_does_not_block_other_sites() {
    let unit = analyze_ok(
        "genus Cista {\n    varia numerus n\n    functio ter() → numerus {\n        fixum numerus t = praefixum {\n            redde 3\n        }\n        redde ego.n * t\n    }\n}\n\
         fixum numerus Z = praefixum {\n    redde 4\n}\n\
         incipit {\n    nota Z\n}\n",
    );
    assert_eq!(local_int(&unit, "t"), 3);
}

#[test]
fn constant_that_calls_a_function_holding_a_local_site_sees_it_evaluated() {
    let unit = analyze_ok(
        "fixum numerus SUMMA = praefixum {\n    redde ima()\n}\n\
         functio ima() → numerus {\n    fixum numerus k = praefixum {\n        redde 4\n    }\n    redde k * 2\n}\n\
         incipit {\n    nota SUMMA\n}\n",
    );
    assert_eq!(local_int(&unit, "k"), 4);
    let value = unit
        .hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Constant(konst) if unit.interner.resolve(konst.name) == "SUMMA" => Some(konst.value.clone()),
            _ => None,
        })
        .expect("SUMMA");
    assert!(matches!(value.kind, HirExpressionKind::Literal(HirLiteral::Int(8))));
}

#[test]
fn compile_time_local_may_read_an_evaluated_local_through_a_plain_constant() {
    let unit = analyze_ok(
        "incipit {\n    fixum numerus a = praefixum {\n        redde 2\n    }\n    fixum numerus b = a + 1\n    nota b\n}\n",
    );
    assert_eq!(local_int(&unit, "a"), 2);
}

#[test]
fn mori_in_a_local_body_is_a_build_error_naming_the_local() {
    let diagnostic = rejection(
        "incipit {\n    fixum numerus z = praefixum {\n        mori \"limes excessus\"\n    }\n    nota z\n}\n",
        "praefixum_failed",
    );
    let arg = |name: &str| {
        diagnostic
            .args
            .iter()
            .find(|arg| arg.name == name)
            .map(|arg| arg.value.as_str())
    };
    assert_eq!(arg("message"), Some("limes excessus"));
    assert_eq!(arg("name"), Some("z"));
}

#[test]
fn faber_reemission_keeps_the_authored_local_body() {
    let unit = analyze_with(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirFaber),
        "incipit {\n    fixum numerus n = praefixum {\n        redde 2 + 2\n    }\n    nota n\n}\n",
    )
    .expect("analysis");
    let inits = local_inits(&unit);
    let (_, init) = inits.iter().find(|(name, _)| name == "n").expect("local n");
    assert!(matches!(init.kind, HirExpressionKind::Praefixum(_)));
}

#[test]
fn reemit_keeps_the_local_body_even_under_a_default_target_session() {
    // The roundtrip lane (and any other caller) hands `reemit_faber` a default
    // session; the evaluating targets would print the literal in the body's
    // place, because a block-level constant keeps no authored copy.
    let session = Session::new(Config::default().with_latin_locale().with_dev_locale_root());
    let result = crate::driver::reemit_faber(
        &session,
        &crate::locale::latin_locale_pack(),
        "praefixum_local.fab",
        "incipit {\n    fixum numerus n = praefixum {\n        redde 2 + 2\n    }\n    nota n\n}\n",
    );
    let code = result.output.expect("re-emission").code;
    assert!(code.contains("praefixum {"), "{code}");
    assert!(!code.contains("fixum numerus n = 4"), "{code}");
}
