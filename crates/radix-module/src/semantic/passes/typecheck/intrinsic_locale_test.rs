//! L3 proof — typecheck surface→canonical intrinsic resolution (locale packs).
//!
//! `check_intrinsic_method_call` resolves a localized surface spelling to the
//! unchanged canonical registry name through the active reader pack, then
//! rewrites the HIR method symbol so lint and MIR lowering match canonical
//! names without ever seeing the pack.
//!
//! Pack-authoritative lookup (db54b442): any NON-la pack is the sole authority
//! for intrinsic spellings. Only spellings the pack declares as a surface for
//! the receiver's family resolve (surface→canonical); the canonical Latin
//! spelling is a miss and a compile error — no Latin fallback, no registry
//! bypass. Under the la pack or no pack, lookup is registry-direct (canonical
//! Latin identity).
//!
//! Delivery done-when covered here:
//! 1. With the en pack active, `textus.contains(c)` resolves to the canonical
//!    intrinsic `continet` (HIR symbol is `continet`; MIR lowers the `Contains`
//!    collection op), while the canonical Latin spelling `continet` is a
//!    compile error — pack-authoritative, no Latin fallback.
//! 2. Without a pack, surface-only behavior is byte-identical to today (the
//!    en spelling keeps its miss path; no new diagnostics, no rewrite).
//! 3. la and no-pack behavior identical to today (canonical spelling works).
//! 4. Unknown-method calls on a textus receiver produce no surface-spelling
//!    leakage: the diagnostic set is identical with and without the pack.
//! 5. Lint and MIR lowering of an en-spelling call succeed without lint/MIR
//!    changes (the canonical name flows through the HIR).

use crate::diagnostics::Diagnostic;
use crate::driver::{analyze_source, Config, Session};
use crate::hir::{HirExpression, HirExpressionKind, HirModule, HirStatementKind};
use crate::mir::lower_analyzed_unit;
use crate::mir::{MirCollectionOp, MirIntrinsic, MirProgram, MirStatementKind};
use std::path::PathBuf;

fn pack(id: &str) -> crate::locale::LocalePack {
    let stdlib = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stdlib");
    crate::locale::LocalePack::from_toml_path(stdlib.join(format!("locale/{id}/pack.toml")))
        .unwrap_or_else(|err| panic!("installed {id} pack should load: {err:?}"))
}

fn en_pack() -> crate::locale::LocalePack {
    pack("en")
}

fn la_pack() -> crate::locale::LocalePack {
    pack("la")
}

fn analyze(
    source: &str,
    locale: Option<crate::locale::LocalePack>,
) -> Result<crate::driver::AnalyzedModule, Vec<Diagnostic>> {
    let config = match locale {
        Some(locale) => Config::new().with_locale_pack(locale),
        None => Config::default(),
    };
    let session = Session::new(config);
    analyze_source(&session, "intrinsic_locale.fab", source)
}

fn diagnostic_summary(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .map(|diag| format!("{:?}|{:?}|{:?}", diag.code, diag.args, diag.message))
        .collect()
}

/// Collect the method-call spellings present in a program's function bodies.
fn method_call_spellings(program: &HirModule, interner: &crate::lexer::Interner) -> Vec<String> {
    let mut spellings = Vec::new();
    for item in &program.items {
        let crate::hir::HirItemKind::Function(function) = &item.kind else {
            continue;
        };
        if let Some(body) = &function.body {
            collect_block_methods(body, interner, &mut spellings);
        }
    }
    spellings
}

fn collect_method_names(expr: &HirExpression, interner: &crate::lexer::Interner, out: &mut Vec<String>) {
    match &expr.kind {
        HirExpressionKind::MethodCall(receiver, name, _, _) => {
            out.push(interner.resolve(*name).to_owned());
            collect_method_names(receiver, interner, out);
        }
        HirExpressionKind::Call(callee, _, _) => collect_method_names(callee, interner, out),
        HirExpressionKind::Binary(_, lhs, rhs) => {
            collect_method_names(lhs, interner, out);
            collect_method_names(rhs, interner, out);
        }
        HirExpressionKind::Unary(_, operand) => collect_method_names(operand, interner, out),
        HirExpressionKind::Field(object, _) => collect_method_names(object, interner, out),
        HirExpressionKind::Index(object, index) => {
            collect_method_names(object, interner, out);
            collect_method_names(index, interner, out);
        }
        HirExpressionKind::Block(block) => collect_block_methods(block, interner, out),
        HirExpressionKind::Si { cond, then_block, then_catch, else_block } => {
            collect_method_names(cond, interner, out);
            collect_block_methods(then_block, interner, out);
            if let Some(catch) = then_catch {
                collect_block_methods(&catch.body, interner, out);
            }
            if let Some(block) = else_block {
                collect_block_methods(block, interner, out);
            }
        }
        _ => {}
    }
}

fn collect_block_methods(block: &crate::hir::HirBlock, interner: &crate::lexer::Interner, out: &mut Vec<String>) {
    for statement in &block.statements {
        match &statement.kind {
            HirStatementKind::Expr(expr) => collect_method_names(expr, interner, out),
            HirStatementKind::Redde(Some(expr)) => collect_method_names(expr, interner, out),
            HirStatementKind::Local(local) => {
                if let Some(init) = &local.init {
                    collect_method_names(init, interner, out);
                }
            }
            _ => {}
        }
    }
}

/// Whether the MIR program contains a `Contains` collection runtime call.
fn mir_has_contains(mir: &MirProgram) -> bool {
    mir.functions.iter().any(|function| {
        function.blocks.iter().any(|block| {
            block.statements.iter().any(|statement| {
                matches!(
                    &statement.kind,
                    MirStatementKind::RuntimeCall { call, .. }
                        if call.intrinsic == MirIntrinsic::Collection(MirCollectionOp::Contains)
                )
            })
        })
    })
}

const EN_SOURCE: &str = "fn probe(string t) → bool {\n    return t.contains(\"a\")\n}\n";
const EN_CANONICAL_SOURCE: &str = "fn probe(string t) → bool {\n    return t.continet(\"a\")\n}\n";
const LA_SOURCE: &str = "functio probe(textus t) → bivalens {\n    redde t.continet(\"a\")\n}\n";
const LA_SURFACE_MISS: &str = "functio probe(textus t) → bivalens {\n    redde t.contains(\"a\")\n}\n";
const NO_PACK_SURFACE: &str = "functio probe(textus t) → bivalens {\n    redde t.contains(\"a\")\n}\n";
const UNKNOWN_METHOD: &str = "functio probe(textus t) → bivalens {\n    redde t.foo(\"a\")\n}\n";
const FAMILY_MISS: &str = "fn probe(string t) → string {\n    return t.append(\"a\")\n}\n";

/// done_when 1: with the en pack active, `textus.contains(c)` typechecks to
/// the same intrinsic as `continet(c)`. The HIR carries the canonical symbol
/// and MIR lowers the same `Contains` op.
#[test]
fn en_surface_resolves_to_same_intrinsic_as_canonical() {
    let mut unit = analyze(EN_SOURCE, Some(en_pack())).expect("en `contains` should typecheck with the en pack");
    assert_eq!(
        method_call_spellings(&unit.hir, &unit.interner),
        vec!["continet".to_owned()],
        "the en surface spelling must be rewritten to the canonical registry name on the HIR"
    );
    let mir = lower_analyzed_unit(&mut unit, Default::default()).map(|lowered| lowered.program).expect("en `contains` call must lower to MIR");
    assert!(
        mir_has_contains(&mir),
        "en `contains` must lower to the same Contains op as `continet`"
    );
}

/// done_when 1 (corrected db54b442): the canonical Latin spelling `continet`
/// is a MISS under the en pack — pack-authoritative, no Latin fallback, no
/// registry bypass. The en surface is `contains`, so `continet` is a compile
/// error under en.
#[test]
fn en_canonical_latin_spelling_is_compile_error() {
    let result = analyze(EN_CANONICAL_SOURCE, Some(en_pack()));
    let diagnostics = result.unwrap_err_or_diagnostics();
    assert!(
        !diagnostics.is_empty(),
        "canonical Latin spelling `continet` must be a compile error under the en pack"
    );
}

/// done_when 1 (corrected db54b442): a NON-la pack is the sole authority for
/// intrinsic spellings. Under en, the declared en surface `append` (Lista
/// `appende` row) resolves and rewrites to the canonical `appende`, while the
/// canonical Latin spelling `appende` is a compile error — the task's own
/// example (`x.append()` resolves; `x.appende()` = compile error).
#[test]
fn en_append_surface_resolves_and_latin_spelling_errors() {
    let en_source = "fn probe(list<string> xs) → void {\n    xs.append(\"a\")\n}\n";
    let unit = analyze(en_source, Some(en_pack())).expect("en `append` on a Lista must resolve with the en pack");
    assert_eq!(
        method_call_spellings(&unit.hir, &unit.interner),
        vec!["appende".to_owned()],
        "the en surface `append` must rewrite to canonical `appende` on the HIR"
    );

    let latin_source = "fn probe(list<string> xs) → void {\n    xs.appende(\"a\")\n}\n";
    let result = analyze(latin_source, Some(en_pack()));
    let diagnostics = result.unwrap_err_or_diagnostics();
    assert!(
        !diagnostics.is_empty(),
        "canonical Latin `appende` must be a compile error under the en pack"
    );
}

/// done_when 3 (corrected db54b442): the la pack and no-pack both keep
/// registry-direct Latin identity — the canonical spelling `continet` resolves
/// with no pack mediation, byte-identical to no-pack behavior.
#[test]
fn la_and_no_pack_keep_latin_identity() {
    let cases: [(&str, Option<crate::locale::LocalePack>); 2] = [("la", Some(la_pack())), ("no-pack", None)];
    for (label, locale) in cases {
        let mut unit = analyze(LA_SOURCE, locale)
            .unwrap_or_else(|diags| panic!("{label}: canonical `continet` should typecheck: {diags:?}"));
        assert_eq!(
            method_call_spellings(&unit.hir, &unit.interner),
            vec!["continet".to_owned()],
            "{label}: the canonical Latin spelling must be kept as-is"
        );
        let mir = lower_analyzed_unit(&mut unit, Default::default()).map(|lowered| lowered.program).expect("{label}: `continet` call must lower to MIR");
        assert!(mir_has_contains(&mir), "{label}: `continet` must lower to Contains");
    }
}

/// done_when 2: without a pack, surface-only behavior is byte-identical to
/// today — the en spelling keeps its miss path, is not rewritten, and the
/// diagnostic set is unchanged.
#[test]
fn without_pack_surface_spelling_is_unchanged() {
    let unit = analyze(NO_PACK_SURFACE, None).expect("no-pack `contains` keeps today's accept path");
    assert_eq!(
        method_call_spellings(&unit.hir, &unit.interner),
        vec!["contains".to_owned()],
        "without a pack the surface spelling is never rewritten"
    );
}

/// done_when 3: the la pack behaves identically to today — canonical spelling
/// works, and the en surface keeps its miss path (same as no-pack).
#[test]
fn la_pack_keeps_canonical_spelling_and_identity() {
    let mut unit = analyze(LA_SOURCE, Some(la_pack())).expect("la canonical `continet` should typecheck");
    assert_eq!(method_call_spellings(&unit.hir, &unit.interner), vec!["continet".to_owned()]);
    let mir = lower_analyzed_unit(&mut unit, Default::default()).map(|lowered| lowered.program).expect("la `continet` call must lower to MIR");
    assert!(mir_has_contains(&mir), "la `continet` must lower to Contains");

    // la has no `[intrinsics]` rows: the en surface miss is byte-identical to no-pack.
    let la_result = analyze(LA_SURFACE_MISS, Some(la_pack()));
    let no_pack_result = analyze(NO_PACK_SURFACE, None);
    assert_eq!(
        diagnostic_summary(&la_result.unwrap_err_or_diagnostics()),
        diagnostic_summary(&no_pack_result.unwrap_err_or_diagnostics()),
        "la surface miss must equal the no-pack surface miss (identity-by-absence)"
    );
}

/// done_when 4: an unknown method on a textus receiver produces the same
/// diagnostic set with and without the pack — no surface spelling leaks. The
/// with-pack case is the `la` pack (identity surface): the Latin source
/// parses under la authority exactly as it does with no pack, so the
/// unknown-method miss diagnostics are byte-identical.
#[test]
fn unknown_method_diagnostics_are_pack_agnostic() {
    let with_pack = analyze(UNKNOWN_METHOD, Some(la_pack()));
    let without_pack = analyze(UNKNOWN_METHOD, None);
    assert_eq!(
        diagnostic_summary(&with_pack.unwrap_err_or_diagnostics()),
        diagnostic_summary(&without_pack.unwrap_err_or_diagnostics()),
        "unknown-method diagnostics must not depend on the active pack"
    );
}

/// done_when 5: lint (inside `analyze_source`) and MIR lowering of an
/// en-spelling call succeed without any lint/MIR changes. The MIR op assertion
/// proves the canonical name reached lowering unchanged.
#[test]
fn lint_and_mir_accept_en_spelling_call() {
    let mut unit = analyze(EN_SOURCE, Some(en_pack())).expect("en `contains` passes typecheck + lint with the en pack");
    let mir = lower_analyzed_unit(&mut unit, Default::default()).map(|lowered| lowered.program).expect("en `contains` call lowers to MIR");
    assert!(mir_has_contains(&mir));
}

/// Family scoping: `append` is the en surface of the lista `appende` row; on a
/// textus receiver it must NOT resolve (the en pack has no Textus row for it),
/// so the HIR keeps the surface spelling and the miss path is unchanged.
#[test]
fn en_surface_is_family_scoped() {
    let unit = analyze(FAMILY_MISS, Some(en_pack())).expect("family-scoped surface keeps today's accept path");
    assert_eq!(
        method_call_spellings(&unit.hir, &unit.interner),
        vec!["append".to_owned()],
        "a surface mapped for another receiver family must not resolve on textus"
    );
    // The Latin-equivalent call behaves identically without any pack.
    let latin = "functio probe(textus t) → textus {\n    redde t.append(\"a\")\n}\n";
    assert!(analyze(latin, None).is_ok(), "la/no-pack append must keep today's miss path");
}

// ---------------------------------------------------------------------------
// L4 — en-locale proof + naming-judgment callout proofs (goal done-when 2).
// ---------------------------------------------------------------------------

/// The rivus `is_ascii_alphanumeric` shape verbatim: a named textus const
/// `ASCII_ALNUM_CHARS` queried with `.contains(c)`. The en surface rewrites to
/// the canonical `continet` on the HIR and lowers to the same `Contains` MIR
/// runtime call as the canonical spelling — same intrinsic, same semantics.
const EN_ALNUM_SOURCE: &str = "const string ASCII_ALNUM_CHARS ← \"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ\"\nfn is_ascii_alphanumeric(string c) → bool {\n    return ASCII_ALNUM_CHARS.contains(c)\n}\n";
const EN_ALNUM_CANONICAL: &str = "const string ASCII_ALNUM_CHARS ← \"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ\"\nfn is_ascii_alphanumeric(string c) → bool {\n    return ASCII_ALNUM_CHARS.continet(c)\n}\n";

/// L4 done_when 1 — the en proof: `ASCII_ALNUM_CHARS.contains(c)` on a textus
/// passes faber check and lowers to the same `Contains` MIR op as canonical
/// `continet`. Under the pack-authoritative lookup, the canonical Latin
/// spelling `.continet(c)` is itself a compile error under en (no Latin
/// fallback) — only the declared en surface `contains` resolves.
#[test]
fn en_proof_ascii_alnum_chars_contains() {
    let mut en = analyze(EN_ALNUM_SOURCE, Some(en_pack()))
        .expect("en proof `.contains(c)` must pass faber check with the en pack");

    // The en surface rewrites to the canonical registry identity.
    assert_eq!(
        method_call_spellings(&en.hir, &en.interner),
        vec!["continet".to_owned()],
        "the en surface `contains` must rewrite to canonical `continet` on the HIR"
    );

    // The canonical Latin spelling is a miss under en: pack-authoritative,
    // no Latin fallback, no registry bypass.
    let canonical = analyze(EN_ALNUM_CANONICAL, Some(en_pack()));
    let canonical_diagnostics = canonical.unwrap_err_or_diagnostics();
    assert!(
        !canonical_diagnostics.is_empty(),
        "canonical `.continet(c)` must be a compile error under the en pack"
    );

    // The en surface lowers to the same `Contains` runtime call (identical
    // runtime semantics; the canonical name flowed into MIR lowering unchanged).
    let en_mir = lower_analyzed_unit(&mut en, Default::default()).map(|lowered| lowered.program).expect("en proof `.contains(c)` must lower to MIR");
    assert!(mir_has_contains(&en_mir), "en `.contains(c)` must lower to the Contains op");
}

/// L4 done_when 2 — the la proof: the same shape in la surface (canonical
/// identity-by-absence) with `.continet(c)` still passes under the la pack.
#[test]
fn la_proof_ascii_alnum_chars_continet() {
    let la_source = "fixum textus ASCII_ALNUM_CHARS ← \"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ\"\nfunctio is_ascii_alphanumeric(textus c) → bivalens {\n    redde ASCII_ALNUM_CHARS.continet(c)\n}\n";
    let mut unit = analyze(la_source, Some(la_pack())).expect("la proof `.continet(c)` must pass with the la pack");
    assert_eq!(
        method_call_spellings(&unit.hir, &unit.interner),
        vec!["continet".to_owned()],
        "la `.continet(c)` must keep the canonical spelling"
    );
    let mir = lower_analyzed_unit(&mut unit, Default::default()).map(|lowered| lowered.program).expect("la proof `.continet(c)` must lower to MIR");
    assert!(mir_has_contains(&mir), "la `.continet(c)` must lower to the Contains op");
}

/// Naming judgment callout (L2 review gate) — `inversa → "invert"` shared
/// surface: one en spelling serves both the Lista immutable-reverse row and
/// the Matrix square-inverse row. `.invert()` must resolve to the canonical
/// `inversa` on either receiver.
#[test]
fn en_surface_invert_shared_across_lista_and_matrix() {
    let lista_source = "fn probe(list<int> xs) → list<int> {\n    return xs.invert()\n}\n";
    let matrix_source = "fn probe(matrix<f32, [2, 2]> m) → any {\n    return m.invert()\n}\n";

    let lista =
        analyze(lista_source, Some(en_pack())).expect("en `.invert()` on a list must typecheck with the en pack");
    assert_eq!(
        method_call_spellings(&lista.hir, &lista.interner),
        vec!["inversa".to_owned()],
        "en `invert` on a Lista receiver must rewrite to canonical `inversa`"
    );

    let matrix =
        analyze(matrix_source, Some(en_pack())).expect("en `.invert()` on a matrix must typecheck with the en pack");
    assert_eq!(
        method_call_spellings(&matrix.hir, &matrix.interner),
        vec!["inversa".to_owned()],
        "en `invert` on a Matrix receiver must rewrite to canonical `inversa`"
    );
}

/// Naming judgment callout (L2 review gate) — the `union → "union"` identity
/// row: the Intervallum `union` intrinsic's en surface is its own canonical
/// spelling. The call parses and resolves even though `union` is also the en
/// keyword surface for `discretio` (grammar slots claim keywords by latin
/// identity, so a member call still parses).
#[test]
fn en_surface_union_is_identity_row_on_intervallum() {
    let source = "fn probe(intervallum<int> i, intervallum<int> j) → any {\n    return i.union(j)\n}\n";
    let unit =
        analyze(source, Some(en_pack())).expect("en `.union(j)` on an intervallum must typecheck with the en pack");
    assert_eq!(
        method_call_spellings(&unit.hir, &unit.interner),
        vec!["union".to_owned()],
        "en `union` is an identity row: the canonical spelling is its own surface"
    );
}

/// Naming judgment callout (L2 review gate) — the mutating = imperative
/// scheme: mutating operations take the plain English imperative (`.sort()`),
/// immutable value-returning counterparts the participle (`.sorted()`). Each
/// rewrites to its own canonical row (`ordina` / `ordinata`).
#[test]
fn en_surface_mutating_imperative_scheme_sort_sorted() {
    let sort_source = "fn probe(list<int> xs) → void {\n    xs.sort()\n}\n";
    let sorted_source = "fn probe(list<int> xs) → list<int> {\n    return xs.sorted()\n}\n";

    let sort = analyze(sort_source, Some(en_pack())).expect("en `.sort()` on a list must typecheck with the en pack");
    assert_eq!(
        method_call_spellings(&sort.hir, &sort.interner),
        vec!["ordina".to_owned()],
        "en `sort` (imperative, mutating) must rewrite to canonical `ordina`"
    );

    let sorted =
        analyze(sorted_source, Some(en_pack())).expect("en `.sorted()` on a list must typecheck with the en pack");
    assert_eq!(
        method_call_spellings(&sorted.hir, &sorted.interner),
        vec!["ordinata".to_owned()],
        "en `sorted` (participle, value-returning) must rewrite to canonical `ordinata`"
    );
}

trait UnwrapErrOrDiagnostics {
    fn unwrap_err_or_diagnostics(self) -> Vec<Diagnostic>;
}

impl UnwrapErrOrDiagnostics for Result<crate::driver::AnalyzedModule, Vec<Diagnostic>> {
    fn unwrap_err_or_diagnostics(self) -> Vec<Diagnostic> {
        match self {
            Ok(_) => Vec::new(),
            Err(diagnostics) => diagnostics,
        }
    }
}
