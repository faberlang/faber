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
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::hir::{HirExpression, HirExpressionKind, HirModule, HirStatementKind};
use crate::intrinsics::IntrinsicReceiver;
use crate::mir::MirLoweringOptions;
use crate::mir::lower_analyzed_unit;
use crate::mir::{MirCollectionOp, MirIntrinsic, MirProgram, MirStatementKind};

fn pack(id: &str) -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(id))
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
        None => Config::default().with_latin_locale(),
    };
    let session = Session::new(config);
    analyze_source(&session, "intrinsic_locale.fab", source, AnalyzeRequest::default())
}

fn diagnostic_summary(diagnostics: &[Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .map(|diag| format!("{:?}|{:?}|{:?}", diag.code, diag.args, diag.message))
        .collect()
}

/// True when the diagnostics carry the named `unknown_method` miss
/// (need: a method absent from a builtin receiver's registry is a
/// check error, not an accepted verbatim call).
fn reports_unknown_method(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(|diag| {
        diag.args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "unknown_method")
    })
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
        HirExpressionKind::MethodCall(receiver, name, _, _, _) => {
            out.push(interner.resolve(*name).to_owned());
            collect_method_names(receiver, interner, out);
        }
        HirExpressionKind::Call(callee, _, _, _) => collect_method_names(callee, interner, out),
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
    let unit = analyze(EN_SOURCE, Some(en_pack())).expect("en `contains` should typecheck with the en pack");
    assert_eq!(
        method_call_spellings(&unit.hir, &unit.interner),
        vec!["continet".to_owned()],
        "the en surface spelling must be rewritten to the canonical registry name on the HIR"
    );
    let mir = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("en `contains` call must lower to MIR");
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
    let en_source = "fn probe(mut list<string> xs) → void {\n    xs.append(\"a\")\n}\n";
    let unit = analyze(en_source, Some(en_pack())).expect("en `append` on a Lista must resolve with the en pack");
    assert_eq!(
        method_call_spellings(&unit.hir, &unit.interner),
        vec!["appende".to_owned()],
        "the en surface `append` must rewrite to canonical `appende` on the HIR"
    );

    let latin_source = "fn probe(mut list<string> xs) → void {\n    xs.appende(\"a\")\n}\n";
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
        let unit = analyze(LA_SOURCE, locale)
            .unwrap_or_else(|diags| panic!("{label}: canonical `continet` should typecheck: {diags:?}"));
        assert_eq!(
            method_call_spellings(&unit.hir, &unit.interner),
            vec!["continet".to_owned()],
            "{label}: the canonical Latin spelling must be kept as-is"
        );
        let mir = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
            .map(|lowered| lowered.validated.into_parts().0)
            .expect("{label}: `continet` call must lower to MIR");
        assert!(mir_has_contains(&mir), "{label}: `continet` must lower to Contains");
    }
}

/// done_when 2: without a pack, surface-only behavior is unchanged — the en
/// spelling is not resolved through any pack row, so it keeps its miss path.
/// Since need that miss is the named `unknown_method` check error on
/// a builtin textus receiver, not an accepted verbatim call.
#[test]
fn without_pack_surface_spelling_is_unchanged() {
    let diagnostics = analyze(NO_PACK_SURFACE, None).unwrap_err_or_diagnostics();
    assert!(
        reports_unknown_method(&diagnostics),
        "without a pack the surface spelling is never resolved: {diagnostics:?}"
    );
}

/// done_when 3: the la pack behaves identically to today — canonical spelling
/// works, and the en surface keeps its miss path (same as no-pack).
#[test]
fn la_pack_keeps_canonical_spelling_and_identity() {
    let unit = analyze(LA_SOURCE, Some(la_pack())).expect("la canonical `continet` should typecheck");
    assert_eq!(method_call_spellings(&unit.hir, &unit.interner), vec!["continet".to_owned()]);
    let mir = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("la `continet` call must lower to MIR");
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
    let unit = analyze(EN_SOURCE, Some(en_pack())).expect("en `contains` passes typecheck + lint with the en pack");
    let mir = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("en `contains` call lowers to MIR");
    assert!(mir_has_contains(&mir));
}

/// Family scoping: `append` is the en surface of the lista `appende` row; on a
/// textus receiver it must NOT resolve (the en pack has no Textus row for it),
/// so the call keeps its miss path: the named `unknown_method` check error
///, identical to the no-pack Latin-equivalent call.
#[test]
fn en_surface_is_family_scoped() {
    let diagnostics = analyze(FAMILY_MISS, Some(en_pack())).unwrap_err_or_diagnostics();
    assert!(
        reports_unknown_method(&diagnostics),
        "family-scoped miss must be the named unknown_method error: {diagnostics:?}"
    );
    // The Latin-equivalent call behaves identically without any pack.
    let latin = "functio probe(textus t) → textus {\n    redde t.append(\"a\")\n}\n";
    let latin_diagnostics = analyze(latin, None).unwrap_err_or_diagnostics();
    assert!(
        reports_unknown_method(&latin_diagnostics),
        "la/no-pack append must be the named unknown_method error: {latin_diagnostics:?}"
    );
}

// ---------------------------------------------------------------------------
// L4 — en-locale proof + naming-judgment callout proofs (goal done-when 2).
// ---------------------------------------------------------------------------

/// The rivus `is_ascii_alphanumeric` shape verbatim: a named textus const
/// `ASCII_ALNUM_CHARS` queried with `.contains(c)`. The en surface rewrites to
/// the canonical `continet` on the HIR and lowers to the same `Contains` MIR
/// runtime call as the canonical spelling — same intrinsic, same semantics.
// D5.7 retired top-level `const`/`fixum`; the top-level equivalent is the
// `static` (en) / `generis` (la) compile-time-evaluable declaration
// (D5.8-D5.9). The literal textus initializer here is compile-time
// evaluable, so the fold pass folds it in place without changing the
// `.contains`/`.continet` proof under test.
const EN_ALNUM_SOURCE: &str = "const string ASCII_ALNUM_CHARS = \"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ\"\nfn is_ascii_alphanumeric(string c) → bool {\n    return ASCII_ALNUM_CHARS.contains(c)\n}\n";
const EN_ALNUM_CANONICAL: &str = "const string ASCII_ALNUM_CHARS = \"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ\"\nfn is_ascii_alphanumeric(string c) → bool {\n    return ASCII_ALNUM_CHARS.continet(c)\n}\n";

/// L4 done_when 1 — the en proof: `ASCII_ALNUM_CHARS.contains(c)` on a textus
/// passes faber check and lowers to the same `Contains` MIR op as canonical
/// `continet`. Under the pack-authoritative lookup, the canonical Latin
/// spelling `.continet(c)` is itself a compile error under en (no Latin
/// fallback) — only the declared en surface `contains` resolves.
#[test]
fn en_proof_ascii_alnum_chars_contains() {
    let en = analyze(EN_ALNUM_SOURCE, Some(en_pack()))
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
    let en_mir = lower_analyzed_unit(en.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("en proof `.contains(c)` must lower to MIR");
    assert!(mir_has_contains(&en_mir), "en `.contains(c)` must lower to the Contains op");
}

/// L4 done_when 2 — the la proof: the same shape in la surface (canonical
/// identity-by-absence) with `.continet(c)` still passes under the la pack.
#[test]
fn la_proof_ascii_alnum_chars_continet() {
    let la_source = "fixum textus ASCII_ALNUM_CHARS = \"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ\"\nfunctio is_ascii_alphanumeric(textus c) → bivalens {\n    redde ASCII_ALNUM_CHARS.continet(c)\n}\n";
    let unit = analyze(la_source, Some(la_pack())).expect("la proof `.continet(c)` must pass with the la pack");
    assert_eq!(
        method_call_spellings(&unit.hir, &unit.interner),
        vec!["continet".to_owned()],
        "la `.continet(c)` must keep the canonical spelling"
    );
    let mir = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .map(|lowered| lowered.validated.into_parts().0)
        .expect("la proof `.continet(c)` must lower to MIR");
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
    let source = "fn probe(range<int> i, range<int> j) → any {\n    return i.union(j)\n}\n";
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
    let sort_source = "fn probe(mut list<int> xs) → void {\n    xs.sort()\n}\n";
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

// ---------------------------------------------------------------------------
// LPT — lista `omnia`/`quilibet` reader-pack surface (all eight packs).
//
// Row ownership: LPT-1 owns the en rows (`all`/`any`); the six translated
// packs (ar, hi, th-TH, vi, zh-Hans, zh-Hant) own their own translated rows
// for `omnia`/`quilibet`; la owns no row and proves the twins through
// registry-direct lookup. The reverse cache is explicit-row-only, so
// ownership is asserted with `intrinsic_canonical`; `intrinsic_surface`
// falls back to the canonical spelling and cannot distinguish a row from
// its absence.
// ---------------------------------------------------------------------------

/// Real `Lista` source calls under the en pack: `xs.all(...)` / `ys.any(...)`.
const TWIN_EN_SOURCE: &str = "fn sonda() → bool {\n    let list<int> xs ← empty\n    return xs.all(int d ∴ d ≥ 0)\n}\n\nfn aliud() → bool {\n    let list<int> ys ← empty\n    return ys.any(int d ∴ d ≺ 0)\n}\n";

/// The canonical Latin twin spelling under the en pack: not a declared en
/// surface, so the pack-authoritative lookup must reject the call.
const TWIN_EN_CANONICAL_MISS: &str =
    "fn sonda() → bool {\n    let list<int> xs ← empty\n    return xs.omnia(int d ∴ d ≥ 0)\n}\n";

/// la source (canonical Latin keywords): the twins resolve registry-direct.
const TWIN_LA_SOURCE: &str = "functio sonda() → bivalens {\n    sit lista<numerus> xs ← vacua\n    redde xs.omnia(numerus d ∴ d ≥ 0)\n}\n\nfunctio aliud() → bivalens {\n    sit lista<numerus> ys ← vacua\n    redde ys.quilibet(numerus d ∴ d ≺ 0)\n}\n";

/// (locale, real localized source) for the six translated packs. Each source
/// is composed from that pack's own `[keywords]`/`[types]` rows; the twin
/// surfaces are the pack's own `[intrinsics]` rows (all, any).
const TWIN_TRANSLATED_SOURCES: &[(&str, &str, &str, &str)] = &[
    (
        "ar",
        "الكل",
        "أي",
        "دالة sonda() → منطقي {\n    ليكن قائمة<عدد> xs ← فارغ\n    أعد xs.الكل(عدد d ∴ d ≥ 0)\n}\n\nدالة aliud() → منطقي {\n    ليكن قائمة<عدد> ys ← فارغ\n    أعد ys.أي(عدد d ∴ d ≺ 0)\n}\n",
    ),
    (
        "hi",
        "सभी",
        "कोई",
        "फलन sonda() → तार्किक {\n    बैठा सूची<संख्या> xs ← खाली\n    लौटाओ xs.सभी(संख्या d ∴ d ≥ 0)\n}\n\nफलन aliud() → तार्किक {\n    बैठा सूची<संख्या> ys ← खाली\n    लौटाओ ys.कोई(संख्या d ∴ d ≺ 0)\n}\n",
    ),
    (
        "th-TH",
        "ทั้งหมด",
        "ใดก็ได้",
        "ฟังก์ชัน sonda() → ตรรกะ {\n    กำหนด รายการ<จำนวน> xs ← เซตว่าง\n    คืน xs.ทั้งหมด(จำนวน d ∴ d ≥ 0)\n}\n\nฟังก์ชัน aliud() → ตรรกะ {\n    กำหนด รายการ<จำนวน> ys ← เซตว่าง\n    คืน ys.ใดก็ได้(จำนวน d ∴ d ≺ 0)\n}\n",
    ),
    (
        "vi",
        "tất_cả",
        "bất_kỳ",
        "hàm sonda() → logic {\n    đặt danh_sách<số> xs ← tập_rỗng\n    trả xs.tất_cả(số d ∴ d ≥ 0)\n}\n\nhàm aliud() → logic {\n    đặt danh_sách<số> ys ← tập_rỗng\n    trả ys.bất_kỳ(số d ∴ d ≺ 0)\n}\n",
    ),
    (
        "zh-Hans",
        "全部",
        "任一",
        "函数 sonda() → 布尔 {\n    设 列表<整数> xs ← 空集\n    返回 xs.全部(整数 d ∴ d ≥ 0)\n}\n\n函数 aliud() → 布尔 {\n    设 列表<整数> ys ← 空集\n    返回 ys.任一(整数 d ∴ d ≺ 0)\n}\n",
    ),
    (
        "zh-Hant",
        "全部",
        "任一",
        "函式 sonda() → 布林 {\n    設為 列表<整數> xs ← 空集\n    傳回 xs.全部(整數 d ∴ d ≥ 0)\n}\n\n函式 aliud() → 布林 {\n    設為 列表<整數> ys ← 空集\n    傳回 ys.任一(整數 d ∴ d ≺ 0)\n}\n",
    ),
];

/// done-when: the en surfaces `all`/`any` resolve both twins on a real Lista
/// call to the canonical registry spellings.
#[test]
fn en_twin_surfaces_resolve_to_canonical_omnia_and_quilibet() {
    let pack = en_pack();
    assert_eq!(
        pack.intrinsic_canonical(IntrinsicReceiver::Lista, "all"),
        Some("omnia"),
        "the en reverse cache must map `all` → `omnia` for Lista"
    );
    assert_eq!(
        pack.intrinsic_canonical(IntrinsicReceiver::Lista, "any"),
        Some("quilibet"),
        "the en reverse cache must map `any` → `quilibet` for Lista"
    );
    let unit = analyze(TWIN_EN_SOURCE, Some(pack)).expect("en `all`/`any` on a Lista must typecheck with the en pack");
    assert_eq!(
        method_call_spellings(&unit.hir, &unit.interner),
        vec!["omnia".to_owned(), "quilibet".to_owned()],
        "the en surfaces `all`/`any` must rewrite to canonical `omnia`/`quilibet` on the HIR"
    );
}

/// done-when: each translated pack owns its own rows for the twins — proven on
/// the reverse cache (`intrinsic_canonical`) — and both twins resolve on a real
/// localized Lista call while keeping the canonical HIR spelling. The Latin
/// canonical spelling is a miss under a translated pack, as under en.
#[test]
fn translated_pack_rows_resolve_both_twins_on_lista() {
    for (locale, all_surface, any_surface, source) in TWIN_TRANSLATED_SOURCES {
        let pack = pack(locale);
        assert_eq!(
            pack.intrinsic_canonical(IntrinsicReceiver::Lista, all_surface),
            Some("omnia"),
            "{locale}: `{all_surface}` must be the pack's own row for `omnia`"
        );
        assert_eq!(
            pack.intrinsic_canonical(IntrinsicReceiver::Lista, any_surface),
            Some("quilibet"),
            "{locale}: `{any_surface}` must be the pack's own row for `quilibet`"
        );
        assert_eq!(
            pack.intrinsic_canonical(IntrinsicReceiver::Lista, "omnia"),
            None,
            "{locale}: the Latin spelling must not be a declared surface"
        );
        let unit = analyze(source, Some(pack)).unwrap_or_else(|diagnostics| {
            panic!("{locale}: localized `omnia`/`quilibet` calls must typecheck: {diagnostics:?}")
        });
        assert_eq!(
            method_call_spellings(&unit.hir, &unit.interner),
            vec!["omnia".to_owned(), "quilibet".to_owned()],
            "{locale}: the localized surfaces must rewrite to the canonical HIR spelling"
        );
    }
}

/// List `find` (la `reperta`) and regex `find` (la `inventa`) share a surface in
/// every translated pack and stay apart by receiver family: each family's
/// reverse cache maps the one surface to its own canonical row.
#[test]
fn list_find_and_regex_find_share_a_surface_and_resolve_by_receiver_family() {
    for (locale, find_surface) in [
        ("en", "find"),
        ("ar", "ابحث"),
        ("hi", "खोज"),
        ("th-TH", "ค้นหา"),
        ("vi", "tìm"),
        ("zh-Hans", "查找"),
        ("zh-Hant", "尋找"),
    ] {
        let pack = pack(locale);
        assert_eq!(
            pack.intrinsic_canonical(IntrinsicReceiver::Lista, find_surface),
            Some("reperta"),
            "{locale}: `{find_surface}` must be the list search row"
        );
        assert_eq!(
            pack.intrinsic_canonical(IntrinsicReceiver::Regex, find_surface),
            Some("inventa"),
            "{locale}: `{find_surface}` must stay the regex find row"
        );
    }
    let source = "fn probe(list<int> xs, regex r) → int ∪ none {\n    let _ m ← r.find(\"a\")\n    return xs.find(int d ∴ d ≻ 10)\n}\n";
    let unit = analyze(source, Some(en_pack())).expect("en list `find` and regex `find` must typecheck together");
    assert_eq!(
        method_call_spellings(&unit.hir, &unit.interner),
        vec!["inventa".to_owned(), "reperta".to_owned()],
        "the one en surface must rewrite to each receiver's own canonical spelling"
    );
}

/// The list join `extende` has its own word in every translated pack, distinct
/// from the one-element `appende`, and each resolves back to the Lista row.
#[test]
fn list_join_surface_resolves_to_extende_in_every_pack() {
    for (locale, extend_surface, append_surface) in [
        ("en", "extend", "append"),
        ("ar", "ألحق_الكل", "ألحق"),
        ("hi", "पीछे_मिलाओ", "पीछे_जोड़ो"),
        ("th-TH", "ขยาย", "ต่อท้าย"),
        ("vi", "nối_dài", "nối_đuôi"),
        ("zh-Hans", "扩展", "追加"),
        ("zh-Hant", "擴展", "附加"),
    ] {
        let pack = pack(locale);
        assert_eq!(
            pack.intrinsic_canonical(IntrinsicReceiver::Lista, extend_surface),
            Some("extende"),
            "{locale}: `{extend_surface}` must be the list join row"
        );
        assert_eq!(
            pack.intrinsic_canonical(IntrinsicReceiver::Lista, append_surface),
            Some("appende"),
            "{locale}: `{append_surface}` must stay the one-element append row"
        );
    }
    let source = "fn probe(mut list<int> xs, list<int> ys) → void {\n    xs.extend(ys)\n}\n";
    let unit = analyze(source, Some(en_pack())).expect("en list `extend` must typecheck");
    assert_eq!(
        method_call_spellings(&unit.hir, &unit.interner),
        vec!["extende".to_owned()],
        "the en surface must rewrite to the canonical `extende`"
    );
}

/// done-when (pack-authoritative): the canonical Latin `omnia` spelling is not
/// a declared en surface — the same miss law the other rows already prove.
#[test]
fn twin_canonical_latin_spelling_is_miss_under_en() {
    let result = analyze(TWIN_EN_CANONICAL_MISS, Some(en_pack()));
    let diagnostics = result.unwrap_err_or_diagnostics();
    assert!(
        !diagnostics.is_empty(),
        "canonical Latin `omnia` must be a compile error under the en pack"
    );
}

/// done-when: la owns no intrinsics row; the twins resolve through
/// registry-direct lookup while the reverse cache stays empty — and
/// `intrinsic_surface` still reports the canonical spelling, which is exactly
/// why surface output alone is not evidence of a row.
#[test]
fn la_pack_resolves_twins_registry_direct_without_rows() {
    let la = la_pack();
    assert_eq!(
        la.intrinsic_canonical(IntrinsicReceiver::Lista, "omnia"),
        None,
        "la must have no reverse-cache row for `omnia` (identity-by-absence)"
    );
    assert_eq!(
        la.intrinsic_canonical(IntrinsicReceiver::Lista, "quilibet"),
        None,
        "la must have no reverse-cache row for `quilibet` (identity-by-absence)"
    );
    assert_eq!(
        la.intrinsic_surface("omnia"),
        "omnia",
        "the forward display fallback keeps the canonical spelling with no row — surface output alone is not evidence"
    );
    let unit = analyze(TWIN_LA_SOURCE, Some(la))
        .expect("la canonical `omnia`/`quilibet` must typecheck through registry-direct lookup");
    assert_eq!(
        method_call_spellings(&unit.hir, &unit.interner),
        vec!["omnia".to_owned(), "quilibet".to_owned()],
        "la must keep the canonical spellings unchanged on the HIR"
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
