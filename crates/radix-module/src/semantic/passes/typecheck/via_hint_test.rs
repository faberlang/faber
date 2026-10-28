//! D11.9 `via` convert-hint clause: routing and diagnostics.
//!
//! Contract: a convert hint is a `via` clause after the `↦` target
//! (`x ↦ u32 via Hex`). Every hint routes to its conversion row, the pre-D11.9
//! spellings (a further type argument, a bracketed tail) are parse errors that
//! point at the clause, and misuse of the clause itself (unknown hint, a target
//! that takes none) is a structured error.

use crate::driver::AnalyzeRequest;
use crate::hir::{HirExpressionKind, HirStatementKind};
use crate::{Compiler, Config, Output, Target};

fn pack(id: &str) -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(id))
        .unwrap_or_else(|err| panic!("installed {id} pack should load: {err:?}"))
}

fn latin_config() -> Config {
    Config::default()
        .with_latin_locale()
        .with_target(Target::HirRust)
}

fn english_config() -> Config {
    Config::new().with_locale_pack(pack("en"))
}

/// Compile with `config`, asserting success, and return the generated Rust.
fn rust_code(config: Config, source: &str) -> String {
    let result = Compiler::new(config).compile_str("test.fab", source);
    assert!(result.success(), "diagnostics for {source:?}: {:?}", result.diagnostics);
    match result.output {
        Some(Output::Rust(rust)) => rust.code,
        _ => panic!("expected Rust output for {source:?}"),
    }
}

fn diagnostics_with(config: Config, source: &str) -> Vec<crate::Diagnostic> {
    let session = crate::driver::Session::new(config);
    match crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()) {
        Ok(unit) => unit.diagnostics,
        Err(diagnostics) => diagnostics,
    }
}

fn has_issue(diagnostics: &[crate::Diagnostic], issue: &str) -> bool {
    diagnostics
        .iter()
        .any(|diagnostic| diagnostic.is_error() && diagnostic.issue() == Some(issue))
}

fn assert_issue(diagnostics: &[crate::Diagnostic], issue: &str) {
    assert!(
        has_issue(diagnostics, issue),
        "expected error issue {issue:?}, got: {diagnostics:?}"
    );
}

/// Target type and hint symbols of each conversion initializer of the entry
/// block, in source order (a conversion chain reports outermost first).
fn entry_conversions(source: &str) -> Vec<(String, Vec<String>)> {
    let session = crate::driver::Session::new(latin_config());
    let unit = crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()).expect("analyze");
    assert!(
        !unit
            .diagnostics
            .iter()
            .any(radix_diagnostics::Diagnostic::is_error),
        "diagnostics: {:?}",
        unit.diagnostics
    );
    let entry = unit.hir.entry.as_ref().expect("incipit");
    let mut found = Vec::new();
    for stmt in &entry.statements {
        let HirStatementKind::Local(local) = &stmt.kind else {
            continue;
        };
        let mut node = local.init.as_ref();
        while let Some(expr) = node {
            let HirExpressionKind::Conversio { source, target, params, .. } = &expr.kind else {
                break;
            };
            let target = match target {
                crate::hir::HirConversioTarget::Type(ty) => format!("{:?}", unit.types.get(*ty)),
                crate::hir::HirConversioTarget::Intervallum(_) => "intervallum".to_owned(),
            };
            let hints = params
                .iter()
                .map(|param| unit.interner.resolve(*param).to_owned())
                .collect();
            found.push((target, hints));
            node = Some(source);
        }
    }
    found
}

/// One hint row: the same program written with the retired second type
/// argument and with the `via` clause. `{}` marks the conversion body.
struct Row {
    name: &'static str,
    frame: &'static str,
    legacy: &'static str,
    via: &'static str,
}

const ROWS: &[Row] = &[
    Row {
        name: "Hex",
        frame: "functio f(textus s) → i32 {{ redde s {} ⊥ 0 }}",
        legacy: "↦ numerus<i32, Hex>",
        via: "↦ i32 via Hex",
    },
    Row {
        name: "Bin",
        frame: "functio f(textus s) → u8 {{ redde s {} ⊥ 0 }}",
        legacy: "↦ numerus<u8, Bin>",
        via: "↦ u8 via Bin",
    },
    Row {
        name: "Oct",
        frame: "functio f(textus s) → i32 {{ redde s {} ⊥ 0 }}",
        legacy: "↦ numerus<i32, Oct>",
        via: "↦ i32 via Oct",
    },
    Row {
        name: "Be",
        frame: "incipit {{ sit _ v ← |de ad be ef|[0‥4] {} }}",
        legacy: "↦ numerus<u32, Be>",
        via: "↦ u32 via Be",
    },
    Row {
        name: "Le",
        frame: "incipit {{ sit _ v ← |de ad be ef|[0‥4] {} }}",
        legacy: "↦ numerus<u32, Le>",
        via: "↦ u32 via Le",
    },
    Row {
        name: "Be float",
        frame: "incipit {{ sit _ v ← |3f 80 00 00|[0‥4] {} }}",
        legacy: "↦ fractus<f32, Be>",
        via: "↦ f32 via Be",
    },
    Row {
        name: "Le float",
        frame: "incipit {{ sit _ v ← |00 00 80 3f|[0‥4] {} }}",
        legacy: "↦ fractus<f32, Le>",
        via: "↦ f32 via Le",
    },
    Row {
        name: "Code read",
        frame: "functio f(littera c) → u32 {{ redde c {} }}",
        legacy: "↦ numerus<u32, Code>",
        via: "↦ u32 via Code",
    },
    Row {
        name: "Code build",
        frame: "functio f(numerus n) → littera {{ redde n {} ⊥ '?' }}",
        legacy: "↦ littera<Code>",
        via: "↦ littera via Code",
    },
    Row {
        name: "ascii Hex",
        frame: "functio f(u32 n) → ascii<magnitudo 8> {{ redde n {} ⊥ '--' }}",
        legacy: "↦ ascii<8, Hex>",
        via: "↦ ascii<magnitudo 8> via Hex",
    },
    Row {
        name: "ascii Bin",
        frame: "functio f(u8 n) → ascii<magnitudo 8> {{ redde n {} ⊥ '--' }}",
        legacy: "↦ ascii<8, Bin>",
        via: "↦ ascii<magnitudo 8> via Bin",
    },
    Row {
        name: "ascii Oct",
        frame: "functio f(u16 n) → ascii<magnitudo 8> {{ redde n {} ⊥ '--' }}",
        legacy: "↦ ascii<8, Oct>",
        via: "↦ ascii<magnitudo 8> via Oct",
    },
    Row {
        name: "ascii solved hole",
        frame: "incipit {{ sit _ v ← 255 {} }}",
        legacy: "↦ ascii<_, Hex>",
        via: "↦ ascii<_> via Hex",
    },
];

fn splice(frame: &str, conversion: &str) -> String {
    frame
        .replacen("{}", conversion, 1)
        .replace("{{", "{")
        .replace("}}", "}")
}

#[cfg(feature = "hir-rust")]
#[test]
fn every_hint_row_compiles_through_the_via_clause() {
    for row in ROWS {
        let source = splice(row.frame, row.via);
        let code = rust_code(latin_config(), &source);
        assert!(!code.is_empty(), "row {}: {source}", row.name);
    }
}

#[test]
fn every_retired_type_argument_spelling_is_rejected_pointing_at_the_clause() {
    for row in ROWS {
        // The wrapped numeric heads are rejected whole (N7d); the other scalar
        // heads keep the hint diagnostic.
        let issue = if row.legacy.starts_with("↦ numerus<") || row.legacy.starts_with("↦ fractus<") {
            "numeric_wrapper_retired"
        } else {
            "conversio_hint_type_argument"
        };
        assert_issue(&diagnostics_with(latin_config(), &splice(row.frame, row.legacy)), issue);
    }
}

#[test]
fn the_retired_type_argument_is_rejected_under_the_english_surface() {
    for source in [
        "fn f(string s) → i32 { return s ↦ int<i32, Hex> ⊥ 0 }",
        "fn f(u32 n) → f32 { return n ↦ float<f32, Bits> }",
        "fn f(char c) → u32 { return c ↦ int<u32, Code> }",
    ] {
        assert_issue(&diagnostics_with(english_config(), source), "numeric_wrapper_retired");
    }
    assert_issue(
        &diagnostics_with(english_config(), "fn f(char c) → char { return c ↦ char<Code> ⊥ '?' }"),
        "conversio_hint_type_argument",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn every_hint_routes_under_the_english_surface() {
    for source in [
        "fn f(string s) → i32 { return s ↦ i32 via Hex ⊥ 0 }",
        "fn f(char c) → u32 { return c ↦ u32 via Code }",
        "main { let _ v ← |de ad be ef|[0‥4] ↦ u32 via Be }",
    ] {
        let _ = rust_code(english_config(), source);
    }
}

#[test]
fn the_conversion_node_carries_the_via_hint() {
    let via = entry_conversions(
        r#"incipit {
    sit _ a ← "ff" ↦ i32 via Hex
    sit littera big ← 'A'
    sit _ b ← big ↦ u32 via Code
}"#,
    );
    let hints: Vec<_> = via.iter().map(|(_, hints)| hints.clone()).collect();
    assert_eq!(hints, vec![vec!["Hex".to_owned()], vec!["Code".to_owned()]]);
}

#[test]
fn via_bits_is_the_retirement_diagnostic_not_a_hint() {
    assert_issue(
        &diagnostics_with(latin_config(), "incipit { sit u16 n ← 15360\n sit _ x ← n ↦ f16 via Bits }"),
        "conversio_via_bits_retired",
    );
}

#[test]
fn a_plain_conversion_carries_no_hint() {
    let hints = entry_conversions(r#"incipit { sit _ a ← "22" ↦ numerus }"#);
    assert_eq!(hints.len(), 1);
    assert!(hints[0].1.is_empty());
}

#[cfg(feature = "hir-rust")]
#[test]
fn the_default_binds_after_the_via_clause() {
    let code = rust_code(latin_config(), r"functio f(textus s) → i32 { redde s ↦ i32 via Hex ⊥ 7 }");
    assert!(code.contains('7'), "{code}");
}

#[test]
fn chained_conversions_carry_one_hint_per_hop() {
    let via =
        entry_conversions("incipit { sit lista<b8> b ← |3c 00|\n sit _ x ← b[0‥2] ↦ u16 via Le ↦ b16 ↦ f16 ↦ f32 }");
    let hints: Vec<_> = via.iter().map(|(_, hints)| hints.clone()).collect();
    assert_eq!(
        hints,
        vec![
            Vec::<String>::new(),
            Vec::<String>::new(),
            Vec::<String>::new(),
            vec!["Le".to_owned()]
        ],
        "outermost hop first: the last hop has no hint"
    );
}

#[test]
fn existing_pair_rules_still_reject_through_the_via_clause() {
    // The clause changes the spelling, not the rows: a wrong pair fails with
    // the same structured issue it failed with as a type argument.
    assert_issue(
        &diagnostics_with(latin_config(), "functio f(u16 n) → f32 { redde n ↦ f32 via Bits }"),
        "conversio_via_bits_retired",
    );
    assert_issue(
        &diagnostics_with(latin_config(), "functio f(textus s) → u32 { redde s ↦ u32 via Code }"),
        "code_hint_pair_mismatch",
    );
}

#[test]
fn an_unknown_hint_after_via_is_a_structured_error() {
    assert_issue(
        &diagnostics_with(latin_config(), "functio f(textus s) → i32 { redde s ↦ i32 via Hexx }"),
        "conversio_via_unknown_hint",
    );
}

#[test]
fn via_on_a_target_that_takes_no_hint_is_a_structured_error() {
    for source in [
        "functio f(numerus n) → textus { redde n ↦ textus via Hex }",
        "functio f(numerus n) → bivalens { redde n ↦ bivalens via Le }",
        "functio f(numerus n) → lista<numerus> { redde n ↦ lista<numerus> via Hex }",
        "functio f(numerus n) → numerus { redde n ↦ 0‥10 via Hex }",
    ] {
        assert_issue(&diagnostics_with(latin_config(), source), "conversio_via_target_takes_no_hint");
    }
}

#[test]
fn a_register_target_takes_a_via_hint() {
    // The register windows carry `Be`/`Le` in the clause like any other target.
    for source in [
        "functio w(lista<b8> c) → vector<u32, magnitudo 4> { redde c[0‥16] ↦ vector<u32, magnitudo 4> via Be }",
        "functio o(vector<u32, magnitudo 4> v) → lista<b8, magnitudo 16> { redde v ↦ lista<b8, magnitudo 16> via Le }",
    ] {
        let diagnostics = diagnostics_with(latin_config(), source);
        assert!(
            !diagnostics.iter().any(crate::Diagnostic::is_error),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn the_retired_register_tail_is_rejected_pointing_at_the_clause() {
    for source in [
        "functio w(lista<b8> c) → vector<u32, magnitudo 4> { redde c[0‥16] ↦ vector<u32, magnitudo 4><Be> }",
        "functio o(vector<u32, magnitudo 4> v) → lista<b8, magnitudo 16> { redde v ↦ lista<b8, magnitudo 16><Le> }",
    ] {
        assert_issue(&diagnostics_with(latin_config(), source), "conversio_hint_tail_argument");
    }
}

#[test]
fn via_without_a_hint_is_a_parse_error_issue() {
    assert_issue(
        &diagnostics_with(latin_config(), "functio f(textus s) → i32 { redde s ↦ i32 via }"),
        "conversio_via_missing_hint",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn via_stays_an_ordinary_name_in_a_program_that_also_uses_the_clause() {
    let code = rust_code(
        latin_config(),
        r"functio f(textus via) → i32 {
    sit _ n ← via ↦ i32 via Hex ⊥ 0
    redde n
}",
    );
    assert!(code.contains("via"), "{code}");
}

#[test]
fn every_installed_pack_spells_the_clause_word() {
    // The clause word is a keyword row in each installed pack. The six
    // translated packs carry unreviewed proposals (pack-owner work); the row
    // itself is gated by the pack-completeness tests.
    for (id, word) in [
        ("en", "via"),
        ("la", "via"),
        ("ar", "عبر"),
        ("hi", "द्वारा"),
        ("th-TH", "ผ่านทาง"),
        ("vi", "thông_qua"),
        ("zh-Hans", "经由"),
        ("zh-Hant", "經由"),
    ] {
        let pack = pack(id);
        let spelling = radix_lexer::KeywordPack::keyword_spelling(&pack, "via")
            .unwrap_or_else(|| panic!("{id} pack has no `via` keyword row"));
        assert_eq!(spelling, word, "{id} clause word");
    }
}
