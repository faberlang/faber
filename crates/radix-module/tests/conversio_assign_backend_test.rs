//! CDA-U7 — per-backend emission of `↤` (U+21A4) conversion-directed
//! assignment through each claimed `↦` route's backend machinery.
//!
//! Proves on each HIR backend (rust, ts, go, faber) that `target ↤ source`
//! emits through the backend's existing `↦` + assignment machinery:
//!   - the destination place is assigned the CONVERSION result (never a
//!     backend-native cast of the raw source, never constructor syntax),
//!   - an effectful destination/source subexpression is evaluated exactly
//!     once (AC7),
//!   - `⊥` default rides the conversion.
//!
//! Right-associated chains are covered per backend where the backend supports
//! assignment-result expressions; pre-existing disagreements are recorded in
//! `docs/factory/conversion-directed-assignment/evidence/` (not normalized
//! here).

#[cfg(any(
    feature = "hir-rust",
    feature = "hir-ts",
    feature = "hir-go",
    feature = "hir-faber",
    feature = "hir-swift"
))]
use radix_module::codegen::{Target, generate_from_analyzed};
#[cfg(any(
    feature = "hir-rust",
    feature = "hir-ts",
    feature = "hir-go",
    feature = "hir-faber",
    feature = "hir-swift"
))]
use radix_module::driver::{AnalyzeRequest, Config, Session, analyze_source};
#[cfg(any(
    feature = "hir-rust",
    feature = "hir-ts",
    feature = "hir-go",
    feature = "hir-faber",
    feature = "hir-swift"
))]
use radix_module::locale::{KeywordSurface, latin_locale_pack};

#[cfg(any(
    feature = "hir-rust",
    feature = "hir-ts",
    feature = "hir-go",
    feature = "hir-faber",
    feature = "hir-swift"
))]
fn analyze(source: &str) -> radix_module::driver::AnalyzedModule {
    let session = Session::new(Config::default());
    analyze_source(&session, "conversio-assign.fab", source, AnalyzeRequest::default()).expect("analysis succeeds")
}

#[cfg(feature = "hir-rust")]
fn emit_rust(source: &str) -> String {
    let latin = latin_locale_pack();
    let surface = KeywordSurface::new(&latin);
    match generate_from_analyzed(Target::HirRust, &analyze(source), &surface).expect("rust emit") {
        radix_module::Output::Rust(o) => o.code,
        _ => panic!("expected rust output"),
    }
}

#[cfg(feature = "hir-ts")]
fn emit_ts(source: &str) -> String {
    let latin = latin_locale_pack();
    let surface = KeywordSurface::new(&latin);
    match generate_from_analyzed(Target::HirTypeScript, &analyze(source), &surface).expect("ts emit") {
        radix_module::Output::TypeScript(o) => o.code,
        _ => panic!("expected ts output"),
    }
}

#[cfg(feature = "hir-go")]
fn emit_go(source: &str) -> String {
    let latin = latin_locale_pack();
    let surface = KeywordSurface::new(&latin);
    match generate_from_analyzed(Target::HirGo, &analyze(source), &surface).expect("go emit") {
        radix_module::Output::Go(o) => o.code,
        _ => panic!("expected go output"),
    }
}

#[cfg(feature = "hir-faber")]
fn emit_faber(source: &str) -> String {
    let latin = latin_locale_pack();
    let surface = KeywordSurface::new(&latin);
    match generate_from_analyzed(Target::HirFaber, &analyze(source), &surface).expect("faber emit") {
        radix_module::Output::Faber(o) => o.code,
        _ => panic!("expected faber output"),
    }
}

#[cfg(feature = "hir-swift")]
fn emit_swift(source: &str) -> String {
    let latin = latin_locale_pack();
    let surface = KeywordSurface::new(&latin);
    match generate_from_analyzed(Target::HirSwift, &analyze(source), &surface).expect("swift emit") {
        radix_module::Output::Swift(o) => o.code,
        _ => panic!("expected swift output"),
    }
}

#[cfg(any(feature = "hir-rust", feature = "hir-ts", feature = "hir-faber"))]
fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

/// Count `name()` CALL sites, excluding the function definition line.
#[cfg(all(
    feature = "hir-rust",
    feature = "hir-ts",
    feature = "hir-go",
    feature = "hir-swift",
    feature = "hir-faber"
))]
fn call_occurrences(code: &str, def_prefix: &str, name: &str) -> usize {
    count_occurrences(code, &format!("{name}()")) - count_occurrences(code, &format!("{def_prefix}{name}()"))
}

// ── R1 (textus → numerus) scalar assignment on each backend ────────────────

#[cfg(feature = "hir-rust")]
#[test]
fn rust_emits_conversio_assign_as_parse_then_store() {
    let code =
        emit_rust("incipit {\n  fixum textus raw ← \"42\"\n  varia numerus count ← 0\n  count ↤ raw\n  nota count\n}");
    assert!(
        code.contains(".parse::<"),
        "rust must emit the text→numeric conversion, not a cast:\n{code}"
    );
    assert!(!code.contains("count ↤"), "raw ↤ must not leak into emitted rust:\n{code}");
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_emits_conversio_assign_of_a_literal_as_its_value() {
    let code = emit_rust("incipit {\n  varia numerus count ← 0\n  count ↤ \"42\"\n  nota count\n}");
    assert!(
        code.contains("count = 42_i64;"),
        "a literal text→numeric conversion stores its value:\n{code}"
    );
    assert!(!code.contains(".parse::<"), "no runtime parse of a literal:\n{code}");
    assert!(!code.contains("count ↤"), "raw ↤ must not leak into emitted rust:\n{code}");
}

#[cfg(feature = "hir-ts")]
#[test]
fn ts_emits_conversio_assign_as_parse_then_store() {
    let code = emit_ts("incipit {\n  varia numerus count ← 0\n  count ↤ \"42\"\n  nota count\n}");
    assert!(
        code.contains("Number(") || code.contains("parseInt("),
        "ts must emit the text→numeric conversion, not a bare cast:\n{code}"
    );
    assert!(
        code.contains("count = "),
        "ts must store the converted value into the destination:\n{code}"
    );
    assert!(!code.contains("count ↤"), "raw ↤ must not leak into emitted ts:\n{code}");
}

#[cfg(feature = "hir-go")]
#[test]
fn go_emits_conversio_assign_as_parse_then_store() {
    // Non-literal source so the conversion is not constant-folded at emit time.
    let source = "functio src() → textus {\n  redde \"42\"\n}\n\
                  incipit {\n  varia numerus count ← 0\n  count ↤ src()\n  nota count\n}";
    let code = emit_go(source);
    assert!(
        code.contains("strconv") || code.contains("ParseInt") || code.contains("parseNumerus"),
        "go must emit the text→numeric conversion, not a bare cast:\n{code}"
    );
    assert!(
        code.contains("count = "),
        "go must store the converted value into the destination:\n{code}"
    );
    assert!(!code.contains("count ↤"), "raw ↤ must not leak into emitted go:\n{code}");
}

#[cfg(feature = "hir-faber")]
#[test]
fn faber_emits_conversio_assign_verbatim() {
    let code = emit_faber("incipit {\n  varia numerus count ← 0\n  count ↤ \"42\"\n  nota count\n}");
    assert!(
        code.contains("count ↤ \"42\""),
        "faber re-emits ↤ verbatim through the ↦ route:\n{code}"
    );
    assert!(!code.contains(" ← 42"), "must never rewrite ↤ to ordinary ←:\n{code}");
}

#[cfg(feature = "hir-swift")]
#[test]
fn swift_emits_conversio_assign_as_conversion_then_store() {
    let code = emit_swift("incipit {\n  varia numerus count ← 0\n  count ↤ \"42\"\n  nota count\n}");
    assert!(
        code.contains("count = "),
        "swift must store the converted value into the destination:\n{code}"
    );
    assert!(
        code.contains("Int(") || code.contains("try") || code.contains("parse"),
        "swift must emit the text→numeric conversion through the ↦ route:\n{code}"
    );
    assert!(!code.contains("count ↤"), "raw ↤ must not leak into emitted swift:\n{code}");
}

// ── AC7: effectful source evaluated exactly once per backend ───────────────

#[cfg(all(
    feature = "hir-rust",
    feature = "hir-ts",
    feature = "hir-go",
    feature = "hir-swift",
    feature = "hir-faber"
))]
#[test]
fn effectful_source_referenced_once_on_each_backend() {
    let source = "functio src() → textus {\n  nota \"src\"\n  redde \"42\"\n}\n\
                  incipit {\n  varia numerus count ← 0\n  count ↤ src()\n  nota count\n}";
    for (label, def_prefix, code) in [
        ("rust", "fn ", emit_rust(source)),
        ("ts", "function ", emit_ts(source)),
        ("go", "func ", emit_go(source)),
        ("swift", "func ", emit_swift(source)),
        ("faber", "functio ", emit_faber(source)),
    ] {
        assert_eq!(
            call_occurrences(&code, def_prefix, "src"),
            1,
            "{label} must reference the effectful source exactly once:\n{code}"
        );
    }
}

#[cfg(all(
    feature = "hir-rust",
    feature = "hir-ts",
    feature = "hir-go",
    feature = "hir-swift",
    feature = "hir-faber"
))]
#[test]
fn effectful_destination_index_referenced_once_on_each_backend() {
    let source = "functio idx() → numerus {\n  nota \"idx\"\n  redde 0\n}\n\
                  incipit {\n  varia lista<numerus> xs ← [0, 0]\n  xs[idx()] ↤ \"7\"\n  nota xs[0]\n}";
    for (label, def_prefix, code) in [
        ("rust", "fn ", emit_rust(source)),
        ("ts", "function ", emit_ts(source)),
        ("go", "func ", emit_go(source)),
        ("swift", "func ", emit_swift(source)),
        ("faber", "functio ", emit_faber(source)),
    ] {
        assert_eq!(
            call_occurrences(&code, def_prefix, "idx"),
            1,
            "{label} must reference the effectful destination index exactly once:\n{code}"
        );
    }
}

// ── `⊥` default rides the conversion on each backend ─────────────────────

#[cfg(all(
    feature = "hir-rust",
    feature = "hir-ts",
    feature = "hir-go",
    feature = "hir-swift",
    feature = "hir-faber"
))]
#[test]
fn recovery_rides_conversion_on_each_backend() {
    let source = "incipit {\n  varia numerus count ← 0\n  count ↤ \"x\" ⊥ 0\n  nota count\n}";
    let mut codes = vec![
        ("rust", emit_rust(source), false),
        ("ts", emit_ts(source), false),
        ("go", emit_go(source), false),
        ("swift", emit_swift(source), false),
        ("faber", emit_faber(source), true),
    ];
    for (label, code, is_faber) in codes.drain(..) {
        assert!(code.contains('0'), "{label} must carry the ⊥ recovery value:\n{code}");
        if is_faber {
            assert!(
                code.contains(" ↤ ") && code.contains(" ⊥ 0"),
                "{label} must re-emit ↤ with its ⊥ recovery verbatim:\n{code}"
            );
        } else {
            assert!(!code.contains(" ↤ "), "{label} must not leak raw ↤:\n{code}");
        }
    }
}

// ── Right-associated chain (AC6) per backend ───────────────────────────────

#[cfg(feature = "hir-rust")]
#[test]
fn chain_emits_inner_conversion_feeding_outer_store_on_rust() {
    // Rust assignments are statements whose value is `()`, not the stored
    // value — a PRE-EXISTING ordinary-assignment result-value disagreement
    // (recorded in evidence), shared identically by `←` chains. The chain
    // still converts through the `↦` route: the inner text→numeric parse
    // appears once, and both stores are emitted.
    let code = emit_rust(
        "incipit {\n  fixum textus raw ← \"42\"\n  varia numerus a ← 0\n  varia numerus b ← 0\n  a ↤ b ↤ raw\n  nota a\n  nota b\n}",
    );
    assert_eq!(
        count_occurrences(&code, ".parse::<"),
        1,
        "inner text→numeric link must convert through the ↦ route:\n{code}"
    );
    assert_eq!(count_occurrences(&code, "a = "), 1, "outer store must appear once:\n{code}");
    assert_eq!(count_occurrences(&code, "b = "), 1, "inner store must appear once:\n{code}");
}

#[cfg(feature = "hir-swift")]
#[test]
fn chain_emits_inner_conversion_feeding_outer_store_on_swift() {
    // Swift assignments are statements whose value is not the stored value —
    // the same PRE-EXISTING ordinary-assignment result-value disagreement as
    // rust/go (recorded in evidence), shared identically by `←` chains. The
    // chain still converts through the `↦` route.
    let code =
        emit_swift("incipit {\n  varia numerus a ← 0\n  varia numerus b ← 0\n  a ↤ b ↤ \"42\"\n  nota a\n  nota b\n}");
    assert!(
        code.contains("a = ") && code.contains("b = "),
        "both chain stores must be emitted:\n{code}"
    );
    assert!(
        code.contains("Int(") || code.contains("try") || code.contains("parse"),
        "inner text→numeric link must convert through the ↦ route:\n{code}"
    );
    assert!(!code.contains(" ↤ "), "raw ↤ must not leak into emitted swift:\n{code}");
}

#[cfg(feature = "hir-ts")]
#[test]
fn chain_emits_conversion_on_ts() {
    // JS assignment IS an expression (`a = b = v` yields `v`), so TS chains
    // preserve the inner result without extra machinery.
    let code =
        emit_ts("incipit {\n  varia numerus a ← 0\n  varia numerus b ← 0\n  a ↤ b ↤ \"42\"\n  nota a\n  nota b\n}");
    assert!(code.contains("l_a = l_b = "), "ts must emit the chained store:\n{code}");
    assert!(
        code.contains("Number(") || code.contains("parseInt("),
        "ts chain must convert the inner text→numeric link:\n{code}"
    );
    assert!(!code.contains(" ↤ "), "raw ↤ must not leak into emitted ts:\n{code}");
}

#[cfg(feature = "hir-faber")]
#[test]
fn chain_emits_inner_conversion_feeding_outer_store_on_faber() {
    let code =
        emit_faber("incipit {\n  varia numerus a ← 0\n  varia numerus b ← 0\n  a ↤ b ↤ \"42\"\n  nota a\n  nota b\n}");
    assert!(
        code.contains("a ↤ (b ↤ \"42\")"),
        "faber must re-emit the chain with the inner assignment parenthesized:\n{code}"
    );
    assert_eq!(count_occurrences(&code, " ↤ "), 2, "both links preserved:\n{code}");
}
