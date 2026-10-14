//! HIR v1 G2: target-neutral mode and access policy integration tests.

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::semantic::{PassConfig, analyze};

fn analyze_base(source: &str) -> crate::semantic::SemanticResult {
    let lex = crate::lexer::lex(source);
    let mut parsed = crate::parser::parse(lex);
    assert!(parsed.success(), "parse: {:?}", parsed.errors);
    let program = parsed.program.take().expect("program");
    // Base config: modes on, no Rust lifetime.
    let config = PassConfig::base();
    analyze(&program, &config, &mut parsed.interner)
}

#[test]
fn mode_assign_to_de_rejects_on_base_analysis() {
    let source = r"
functio mutate(de numerus x) → vacuum {
  x ← 2
}
incipit { }
";
    let result = analyze_base(source);
    assert!(
        !result.success(),
        "base analysis should reject assign through de: {:?}",
        result.diagnostics
    );
    assert!(
        result.diagnostics.iter().any(|err| {
            err.args
                .iter()
                .any(|a| a.name == "issue" && a.value == "assign_to_de_parameter")
        }),
        "missing assign_to_de_parameter: {:?}",
        result.diagnostics
    );
}

#[test]
fn mode_assign_to_de_rejects_for_ts_go_targets() {
    let source = r"
functio mutate(de numerus x) → vacuum {
  x ← 2
}
incipit { }
";
    for target in [Target::HirTypeScript, Target::HirGo] {
        let lex = crate::lexer::lex(source);
        let mut parsed = crate::parser::parse(lex);
        let program = parsed.program.take().expect("program");
        let mut config = PassConfig::for_target(target);
        config.borrow_analysis = false;
        let result = analyze(&program, &config, &mut parsed.interner);
        assert!(
            !result.success(),
            "target {target:?} should reject assign through de: {:?}",
            result.diagnostics
        );
        assert!(
            result.diagnostics.iter().any(|err| {
                err.args
                    .iter()
                    .any(|a| a.name == "issue" && a.value == "assign_to_de_parameter")
            }),
            "target {target:?}: {:?}",
            result.diagnostics
        );
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_access_plain_lista_traps_not_silent_default() {
    let session = Session::new(Config::default().with_target(Target::HirRust));
    let unit = analyze_source(
        &session,
        "access.fab",
        r"
incipit {
  fixum lista<numerus> xs ← [1, 2]
  fixum numerus v ← xs[0]
  nota v
}
",
        AnalyzeRequest::default(),
    )
    .expect("analyze");
    let latin = crate::locale::latin_locale_pack();
    let surface = crate::locale::KeywordSurface::new(&latin);
    let out = crate::codegen::generate_from_analyzed(Target::HirRust, &unit, &surface).expect("emit");
    let crate::Output::Rust(rust) = out else {
        panic!("expected rust");
    };
    assert!(
        rust.code.contains("faber::Trap::ListaIndex") || rust.code.contains(".get("),
        "expected checked lista get, got:\n{}",
        rust.code
    );
    assert!(
        !rust.code.contains("unwrap_or_default()"),
        "plain lista must not use unwrap_or_default:\n{}",
        rust.code
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_access_plain_tabula_traps_missing_key() {
    let session = Session::new(Config::default().with_target(Target::HirRust));
    let unit = analyze_source(
        &session,
        "map.fab",
        r#"
incipit {
  fixum tabula<textus, numerus> m ← { "a": 1 }
  fixum numerus v ← m["a"]
  nota v
}
"#,
        AnalyzeRequest::default(),
    )
    .expect("analyze");
    let latin = crate::locale::latin_locale_pack();
    let surface = crate::locale::KeywordSurface::new(&latin);
    let out = crate::codegen::generate_from_analyzed(Target::HirRust, &unit, &surface).expect("emit");
    let crate::Output::Rust(rust) = out else {
        panic!("expected rust");
    };
    assert!(
        rust.code.contains("faber::Trap::TabulaKey"),
        "expected tabula trap, got:\n{}",
        rust.code
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_regex_conversio_compiles_through_the_runtime_constructor() {
    // RD-1/RD-9: the conversion is the one compile point and it is failable;
    // the emitted program calls `faber::Regex::new` (never the engine crate
    // directly, never a `MustCompile`-style wrapper) and consumes its `Result`.
    let session = Session::new(Config::default().with_target(Target::HirRust));
    let unit = analyze_source(
        &session,
        "regex.fab",
        r#"
incipit {
  fixum textus s ← "\\d+"
  fixum regex r ← s ↦ regex
  nota r
}
"#,
        AnalyzeRequest::default(),
    )
    .expect("analyze");
    let latin = crate::locale::latin_locale_pack();
    let surface = crate::locale::KeywordSurface::new(&latin);
    let out = crate::codegen::generate_from_analyzed(Target::HirRust, &unit, &surface).expect("emit");
    let crate::Output::Rust(rust) = out else {
        panic!("expected rust");
    };
    assert!(
        rust.code
            .contains("faber::regex::compile_or_trap(l_s.as_str())"),
        "regex conversion must consume the runtime constructor's result: {}",
        rust.code
    );
    assert!(
        !rust.code.contains("regex::Regex::new(") || rust.code.contains("faber::Regex::new("),
        "regex conversion must not call the engine crate directly: {}",
        rust.code
    );
    assert!(
        !rust.code.contains("MustCompile"),
        "conversion must not MustCompile: {}",
        rust.code
    );
}
