//! inf track U2, driver level: the catalog `code` and `issue` of every `inf`
//! rejection, the English spellings, and the fail-closed emit of every
//! backend (none carries a bignum yet, so none may print a wrong program).

use super::test_support::compile_for_target_and_get_result;
use crate::codegen::Target;
use crate::diagnostics::Diagnostic;
use crate::driver::AnalyzeRequest;
use crate::{Config, Target as ConfigTarget};

fn pack(id: &str) -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(id))
        .unwrap_or_else(|err| panic!("installed {id} pack should load: {err:?}"))
}

fn diagnostics_with(config: Config, source: &str) -> Vec<Diagnostic> {
    let session = crate::driver::Session::new(config);
    match crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()) {
        Ok(unit) => unit.diagnostics,
        Err(diagnostics) => diagnostics,
    }
}

fn latin(source: &str) -> Vec<Diagnostic> {
    diagnostics_with(Config::default().with_target(ConfigTarget::HirRust), source)
}

fn english(source: &str) -> Vec<Diagnostic> {
    diagnostics_with(Config::new().with_locale_pack(pack("en")), source)
}

fn issue_of(diagnostic: &Diagnostic) -> Option<&str> {
    diagnostic
        .args
        .iter()
        .find(|arg| arg.name == "issue")
        .map(|arg| arg.value.as_str())
}

fn arg_of<'a>(diagnostic: &'a Diagnostic, name: &str) -> Option<&'a str> {
    diagnostic
        .args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

fn assert_accepts(diagnostics: &[Diagnostic]) {
    assert!(!diagnostics.iter().any(Diagnostic::is_error), "diagnostics: {diagnostics:?}");
}

fn expect<'a>(diagnostics: &'a [Diagnostic], code: &str, issue: &str) -> &'a Diagnostic {
    diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.is_error() && diagnostic.code == Some(code) && issue_of(diagnostic) == Some(issue)
        })
        .unwrap_or_else(|| panic!("expected {code}.{issue}, got: {diagnostics:?}"))
}

const INF_FILE: &str = r"
genus Capsa {
    fixum inf a
    fixum exactus<inf> b
    fixum modulus<inf> c
    fixum saturatus<inf> d
}

functio transmitte(inf x) → inf {
    redde x
}

incipit {
    fixum inf x ← 0
    fixum inf y ← transmitte(x)
    fixum Capsa c ← Capsa { a = y, b = x, c = y, d = x }
    fixum lista<inf> xs ← [x, y]
    nota c.a
    nota xs
}
";

const AIR_INF_LOCAL: &str = r#"
@ radix lane "air"
functio p(numerus<i32> n) → numerus<i32> {
    fixum inf x ← 1
    redde n
}
"#;

#[test]
fn a_file_declaring_the_four_spellings_checks_clean() {
    assert_accepts(&latin(INF_FILE));
}

#[test]
fn the_english_spellings_check_clean() {
    assert_accepts(&english(
        r"
class Box {
    const inf a
    const trapping<inf> b
    const wrapping<inf> c
    const saturating<inf> d
}

fn carry(inf x) → saturating<inf> {
    return x
}

main {
    const trapping<inf> t ← 1
    const wrapping<inf> w ← carry(t)
    print w
}
",
    ));
}

#[test]
fn rejections_carry_their_catalog_code_and_issue() {
    let tensor = latin("functio f(tensor<inf, [2]> t) → numerus { redde 0 }\n");
    expect(&tensor, "SEM008", "tensor_element_unbounded");

    let kernel = latin("@ nucleum\n@ publica\nfunctio k(inf x) → u32 { redde 1 }\n");
    let host = expect(&kernel, "SEM008", "nucleum_host_type");
    assert_eq!(arg_of(host, "family"), Some("inf"));

    let air = latin(AIR_INF_LOCAL);
    let air = expect(&air, "SEM059", "air_unbounded_integer");
    assert_eq!(arg_of(air, "lane"), Some("air"));

    let sugar = latin("functio f(linf x) → numerus { redde 0 }\n");
    expect(&sugar, "SEM002", "unknown_type");

    // inf track U3: arithmetic and conversions are typed, not rejected.
    assert_accepts(&latin("functio f(inf a, inf b) → inf { redde a + b }\n"));
    assert_accepts(&latin("functio f(inf a) → i32 { redde a ↦ i32 ⊥ 0 }\n"));
    let failable = latin("functio f(inf a) → i32 { redde a ↦ i32 }\n");
    expect(&failable, "SEM011", "failable_conversio_requires_handler");
    let literal = latin("incipit { fixum numerus x ← 18446744073709551616\n nota x }\n");
    expect(&literal, "SEM010", "numerus_literal_out_of_range");
}

const INF_ARITHMETIC: &str = "incipit {\n fixum inf x ← 5\n fixum inf y ← x * x\n nota y\n}\n";
const INF_BIG_LITERAL: &str = "incipit {\n fixum inf x ← 18446744073709551616\n nota x\n}\n";

/// inf track U4: the U3 fail-closed MIR gate (`inf_mir_unsupported_*`) is gone.
/// Every `inf` operation lowers to MIR and validates, for the exact MIR runner
/// to execute; the MIR-backed native targets fail closed at their own layer
/// (`every_mir_backend_fails_closed`).
#[test]
fn inf_operations_lower_to_mir_with_no_gate() {
    for source in [
        INF_ARITHMETIC,
        INF_BIG_LITERAL,
        "functio f(inf a) → i32 { redde a ↦ i32 ⊥ 0 }\nincipit { nota f(3) }\n",
        "functio f(inf a) → inf { redde -a }\nincipit { nota f(3) }\n",
        "functio f(inf a) → inf { redde a.potentia(2) }\nincipit { nota f(3) }\n",
    ] {
        let session = crate::driver::Session::new(Config::default().with_target(ConfigTarget::MirSexp));
        let unit = crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default())
            .unwrap_or_else(|diagnostics| panic!("analysis of {source}: {diagnostics:?}"));
        crate::mir::dump_analyzed_unit(&unit).unwrap_or_else(|errors| panic!("MIR lowering of {source}: {errors:?}"));
    }
}

/// Value flow never changes a value, so it keeps lowering.
#[test]
fn inf_value_flow_still_lowers_to_mir() {
    let result = compile_for_target_and_get_result(INF_FILE, Target::MirSexp);
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|diagnostic| issue_of(diagnostic).is_some_and(|issue| issue.starts_with("inf_mir_unsupported"))),
        "{:?}",
        result.diagnostics
    );
}

/// Every HIR backend rejects a module that names `inf`, by one named issue
/// carrying the type and the target.
#[test]
fn every_hir_backend_fails_closed_with_a_named_diagnostic() {
    for (target, name) in [
        (Target::HirTypeScript, "HirTypeScript"),
        // HirGo carries `inf` as `*big.Int` (inf-U6g); it is no longer fail-closed.
        (Target::HirSwift, "HirSwift"),
        (Target::HirPython, "HirPython"),
        (Target::HirHaskell, "HirHaskell"),
    ] {
        let result = compile_for_target_and_get_result(INF_FILE, target);
        let diagnostic = expect(&result.diagnostics, "CODEGEN001", "inf_target_unsupported");
        assert_eq!(arg_of(diagnostic, "type"), Some("inf"), "{name}");
        assert_eq!(arg_of(diagnostic, "target"), Some(name), "{name}");
    }
}

/// The Rust backend carries `inf` as `faber::Magnus` (inf U6r).
#[test]
fn the_rust_backend_carries_inf() {
    let result = compile_for_target_and_get_result(INF_FILE, Target::HirRust);
    assert_accepts(&result.diagnostics);
}

/// Go carries `inf` as `*big.Int` (inf track U6g): the module compiles.
#[test]
fn the_go_backend_carries_inf() {
    for source in [INF_FILE, INF_BIG_LITERAL] {
        let result = compile_for_target_and_get_result(source, Target::HirGo);
        assert_accepts(&result.diagnostics);
    }
}

/// The Faber re-emitter carries `inf` (it only re-spells types).
#[test]
fn the_faber_reemitter_keeps_inf() {
    let result = compile_for_target_and_get_result(INF_FILE, Target::HirFaber);
    assert_accepts(&result.diagnostics);
}

/// MIR-backed targets fail closed at their own layer: LLVM, Wasm and sexp
/// each reject `numerus<inf>` by their unsupported-shape diagnostic.
#[test]
fn every_mir_backend_fails_closed() {
    for target in [Target::MirLlvm, Target::MirWasm, Target::MirSexp] {
        let result = compile_for_target_and_get_result(INF_FILE, target);
        let code = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.is_error())
            .filter_map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>();
        assert!(
            code.contains(&"CODEGEN001"),
            "{target:?} must reject inf, got: {:?}",
            result.diagnostics
        );
    }
}
