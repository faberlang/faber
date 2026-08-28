//! Re-emission fidelity round-trips driven from source text.
//!
//! Each test compiles a source document through the full canonical path
//! (parse → lower → analyze → emit, presentation attached) and locks two
//! contracts: the emitted spelling for the construct under test, and
//! idempotence (re-emitting the emitted source is byte-identical).

use super::compile::{compile_canonical, compile_canonical_str};
use crate::driver::{Config, Session};

fn emit_once(source: &str) -> String {
    let result = compile_canonical_str("fidelity.fab", source);
    assert!(
        result.success(),
        "compile failed: {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    result.output.expect("emit should produce output").code
}

/// emit(emit(source)) == emit(source): the canonical output is a fixed point.
fn assert_idempotent(source: &str) -> String {
    let first = emit_once(source);
    let second = emit_once(&first);
    assert_eq!(first, second, "re-emission must be idempotent");
    first
}

#[test]
fn cli_function_keeps_argumenta_binding() {
    // GRAMMAR: `functio f() argumenta args` — the CLI bundle param rides the
    // postfix sugar slot, not the parameter list.
    let code = assert_idempotent("functio greet(textus nomen) argumenta args → vacuum {\n    scribe args.nomen\n}\n");
    assert!(
        code.contains("functio greet(textus nomen) argumenta args → vacuum"),
        "argumenta binding must survive re-emission:\n{code}"
    );
}

#[test]
fn implicit_entry_reemits_top_level_statements() {
    // An implicit entry (no `incipit`) must not gain an explicit wrapper:
    // wrapped statements change the document form and can outlaw
    // top-level-only statements.
    let code = assert_idempotent("fixum numerus answer ← 42\nscribe answer\n");
    assert!(!code.contains("incipit"), "implicit entry must stay implicit:\n{code}");
}

#[test]
fn explicit_entry_keeps_incipit_wrapper() {
    let code = assert_idempotent("incipit {\n    scribe \"salve\"\n}\n");
    assert!(code.contains("incipit {"), "explicit entry must keep its wrapper:\n{code}");
}

#[test]
fn negated_type_check_uses_infix_non_est() {
    // `<expr> non est <ty>`; a prefix `non` re-parses as `(non expr) est ty`.
    let code = assert_idempotent("functio probe(numerus x) → bivalens {\n    redde x non est textus\n}\n");
    assert!(
        code.contains("x non est textus"),
        "negated type check must keep the infix spelling:\n{code}"
    );
}

#[test]
fn conversio_markers_ride_target_generic_arguments() {
    // `↦ numerus<u32, Be>` — markers are extra generic arguments; a trailing
    // `<Be>` append is not the source form.
    let code = assert_idempotent("fixum numerus<u32> le ← |de ad be ef|[0‥4] ↦ numerus<u32, Be>\n");
    assert!(
        code.contains("↦ numerus<u32, Be>"),
        "conversio marker must fold into the target generics:\n{code}"
    );
}

#[test]
fn itera_coordinates_survive_reemission() {
    // `itera ex src apud [i] fixum x` — the coordinate binding is
    // loop-visible; dropping it breaks bodies that reference it.
    let code = assert_idempotent(
        "fixum tensor<fractus<f32>, [3]> linea ← vacua\nitera ex linea apud [i] fixum n {\n    scribe i\n}\n",
    );
    assert!(
        code.contains("itera ex linea apud [i] fixum n"),
        "itera apud coordinates must survive:\n{code}"
    );
}

#[test]
fn summa_reduction_reemits_source_shape() {
    let code = assert_idempotent(
        "functio total(tensor<fractus<f32>, [8]> a) → fractus<f32> {\n    redde summa ex a apud [i] fixum s { redde s }\n}\n",
    );
    assert!(
        code.contains("summa ex a apud [i] fixum s"),
        "summa reduction must re-emit its source shape:\n{code}"
    );
}

#[test]
fn extremum_coalesce_parenthesizes_binary_source() {
    let code = assert_idempotent(
        "functio peak(tensor<fractus<f32>, [4]> row, tensor<fractus<f32>, [4]> scale) → fractus<f32> {\n    redde maxima ex (row ⊙ scale) vel 0.0\n}\n",
    );
    assert!(
        code.contains("maxima ex (row ⊙ scale) vel 0.0"),
        "extremum source must stay grouped before the vel tail:\n{code}"
    );
}

#[test]
fn inference_hole_locals_stay_holes() {
    // `fixum _ x ← …` must not materialize the inferred type: a materialized
    // bounded-lista annotation narrows later spread legality on re-check.
    let code = assert_idempotent("fixum _ numeri ← [1, 2, 3]\nscribe numeri\n");
    assert!(code.contains("fixum _ numeri ←"), "inference hole must stay a hole:\n{code}");
}

#[test]
fn union_hole_locals_stay_union_holes() {
    // `var ∪ x` keeps accepting flow members; materializing the solved
    // union would freeze it.
    let mut pack_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    pack_path.push("../../stdlib/locale/en/pack.toml");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack loads");
    let session = Session::new(Config::default().with_locale_pack(pack));
    let result = compile_canonical(
        &session,
        "union-hole.fab",
        "main {\n    var ∪ mixed ← 1\n    mixed ← \"salve\"\n}\n",
    );
    assert!(
        result.success(),
        "{:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let code = result.output.expect("emit").code;
    assert!(code.contains("var ∪ mixed ←"), "union hole must stay a union hole:\n{code}");
}

#[test]
fn nondum_availability_projects_through_en_pack() {
    // The availability family is a contextual keyword: `unstable` under en.
    let mut pack_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    pack_path.push("../../stdlib/locale/en/pack.toml");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack loads");
    let session = Session::new(Config::default().with_locale_pack(pack));
    let result = compile_canonical(
        &session,
        "nondum.fab",
        "interface tempus {\n  @ unstable { target = rs, ratio = \"not implemented\" }\n  fn siste(int handle) → void\n}\n",
    );
    assert!(
        result.success(),
        "{:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let code = result.output.expect("emit").code;
    assert!(code.contains("@ unstable {"), "nondum must surface-project under en:\n{code}");
}

#[test]
fn bare_annotation_markers_stay_bare() {
    // Authored brace shape is carried: `@ futura` (bare) must not gain the
    // empty braced record, while `@ futura { }` keeps its braces.
    let bare = assert_idempotent("@ futura\nfunctio f() → vacuum { }\n");
    assert!(!bare.contains("@ futura { }"), "bare marker must stay bare:\n{bare}");
    let braced = assert_idempotent("@ futura { }\nfunctio f() → vacuum { }\n");
    assert!(braced.contains("@ futura { }"), "authored braces must survive:\n{braced}");
    let kernel = assert_idempotent("@ nucleum\n@ publica\nfunctio k() → vacuum { }\n");
    assert!(kernel.contains("@ nucleum\n"), "bare nucleum marker must stay bare:\n{kernel}");
}

#[test]
fn commune_annotation_round_trips_each_locale_spelling() {
    // la fixture: `@ commune` stays `@ commune`.
    let la = assert_idempotent("discretio E {\n    @ commune\n    textus nuntius\n\n    Malum\n}\n");
    assert!(la.contains("@ commune"), "la commune must stay commune:\n{la}");
    assert!(!la.contains("@ commune { }"), "commune is a bare marker:\n{la}");

    // en fixture: `@ shared` stays `@ shared` — a Latin word in an en file is
    // illegal surface (sealed locales).
    let mut pack_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    pack_path.push("../../stdlib/locale/en/pack.toml");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack loads");
    let session = Session::new(Config::default().with_locale_pack(pack));
    let result = compile_canonical(
        &session,
        "commune-en.fab",
        "union E {\n    @ shared\n    string message\n\n    Bad\n}\n",
    );
    assert!(
        result.success(),
        "{:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let code = result.output.expect("emit").code;
    assert!(code.contains("@ shared"), "en commune must project to shared:\n{code}");
    assert!(!code.contains("@ commune"), "Latin spelling is illegal en surface:\n{code}");
}

#[test]
fn require_throw_guards_re_sugar() {
    // `requirit <cond> iace variant …` re-emits in guard form, not as the
    // desugared `si non (…) { iace … }`.
    let code = assert_idempotent(
        "discretio Err {\n    Malum {\n        textus nuntius\n    }\n}\n\nfunctio probe(numerus x) → numerus ⇥ Err {\n    requirit x ≥ 1 iace finge Malum { nuntius = \"parvum\" }\n    redde x\n}\n",
    );
    assert!(code.contains("requirit x ≥ 1 iace"), "require guard must re-sugar:\n{code}");
    assert!(!code.contains("si non"), "desugared conditional must not surface:\n{code}");
}

#[test]
fn authored_negated_conditional_stays_braced() {
    // A structurally identical authored `si non … { iace … }` must NOT be
    // re-sugared into a guard — only lowered guards carry the marker.
    let code = assert_idempotent(
        "discretio Err {\n    Malum {\n        textus nuntius\n    }\n}\n\nfunctio probe(numerus x) → numerus ⇥ Err {\n    si non (x ≥ 1) {\n        iace finge Malum { nuntius = \"parvum\" }\n    }\n    redde x\n}\n",
    );
    assert!(
        code.contains("si non (x ≥ 1)"),
        "authored negated conditional must stay braced:\n{code}"
    );
    assert!(!code.contains("requirit"), "authored si must not become a guard:\n{code}");
}

#[test]
fn require_comparison_direction_is_preserved() {
    // The guard condition re-emits with its authored operator; no
    // canonicalization to a negated opposite direction.
    let code = assert_idempotent(
        "discretio Err {\n    Malum {\n        textus nuntius\n    }\n}\n\nfunctio probe(numerus x) → numerus ⇥ Err {\n    requirit x ≺ 10 iace finge Malum { nuntius = \"nimium\" }\n    redde x\n}\n",
    );
    assert!(
        code.contains("requirit x ≺ 10 iace"),
        "authored comparison direction must survive:\n{code}"
    );
}

#[test]
fn ambiguous_local_variant_guard_qualifies_with_union() {
    // Two local unions sharing a variant name: the bare guard payload would
    // re-parse ambiguously (SEM001/SEM008 on gradus qwen35moe.fab), so the
    // union-qualified two-part spelling re-emits exactly where authored.
    let code = assert_idempotent(
        "discretio Err {\n    Malum {\n        textus nuntius\n    }\n}\n\ndiscretio AliudErr {\n    Malum {\n        textus nuntius\n    }\n}\n\nfunctio probe(numerus x) → numerus ⇥ Err {\n    requirit x ≥ 1 iace finge Err.Malum { nuntius = \"parvum\" }\n    redde x\n}\n",
    );
    assert!(
        code.contains("iace finge Err.Malum { nuntius = \"parvum\" }"),
        "ambiguous variant must keep the union-qualified spelling:\n{code}"
    );
}

#[test]
fn authored_elige_vocabulary_round_trips() {
    // la fixture: authored `elige … casu … ceterum` keeps its vocabulary;
    // a bare wildcard arm never replaces the authored default.
    let la = assert_idempotent(
        "discretio Modus {\n    Parvum,\n    Magnum\n}\n\nfunctio nomen(Modus m) → textus {\n    elige m {\n        casu Parvum {\n            redde \"parvum\"\n        }\n        ceterum {\n            redde \"magnum\"\n        }\n    }\n}\n",
    );
    assert!(la.contains("elige m {"), "la elige head must survive:\n{la}");
    assert!(la.contains("ceterum {"), "la ceterum default must survive:\n{la}");
    assert!(!la.contains("casu _"), "wildcard arm must not replace ceterum:\n{la}");

    // en fixture: `switch`/`default` surface spellings project through the
    // en pack and round-trip as authored.
    let mut pack_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    pack_path.push("../../stdlib/locale/en/pack.toml");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack loads");
    let session = Session::new(Config::default().with_locale_pack(pack));
    let result = compile_canonical(
        &session,
        "switch-en.fab",
        "union Mode {\n    Small,\n    Big\n}\n\nfn name(Mode m) → string {\n    switch m {\n        case Small {\n            return \"small\"\n        }\n        default {\n            return \"big\"\n        }\n    }\n}\n",
    );
    assert!(
        result.success(),
        "{:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let code = result.output.expect("emit").code;
    assert!(code.contains("switch m {"), "en switch head must survive:\n{code}");
    assert!(code.contains("default {"), "en default arm must survive:\n{code}");
    assert!(!code.contains("match m {"), "switch must not canonicalize to match:\n{code}");
    assert!(!code.contains("case _"), "default must not become case _:\n{code}");
}

#[test]
fn authored_discerne_wildcard_arm_stays_casu() {
    // The mirror contract: an authored `discerne` with a wildcard arm keeps
    // `casu _`; only authored `elige` re-sugars a trailing wildcard to the
    // default spelling.
    let code = assert_idempotent(
        "discretio Modus {\n    Parvum,\n    Magnum\n}\n\nfunctio nomen(Modus m) → textus {\n    discerne m {\n        casu Parvum {\n            redde \"parvum\"\n        }\n        casu _ {\n            redde \"magnum\"\n        }\n    }\n}\n",
    );
    assert!(code.contains("casu _ {"), "authored wildcard arm must stay casu _:\n{code}");
    assert!(!code.contains("ceterum"), "discernere must not gain a default:\n{code}");
}

#[test]
fn discard_binding_round_trips() {
    // la fixture: `fixum T _ ← expr` is a discard binding, not a bare effect
    // call; the arrow count must survive (gradus generation.fab elision).
    let la = assert_idempotent("functio vacua() → numerus {\n    fixum numerus _ ← 6 * 7\n    redde 1\n}\n");
    assert!(la.contains("fixum numerus _ ← 6 * 7"), "la discard binding must survive:\n{la}");

    // en fixture: `const T _ ← expr` keeps the authored declaration form.
    let mut pack_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    pack_path.push("../../stdlib/locale/en/pack.toml");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack loads");
    let session = Session::new(Config::default().with_locale_pack(pack));
    let result = compile_canonical(
        &session,
        "discard-en.fab",
        "fn probe() → int {\n    const int _ ← 6 * 7\n    return 1\n}\n",
    );
    assert!(
        result.success(),
        "{:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let code = result.output.expect("emit").code;
    assert!(code.contains("const int _ ← 6 * 7"), "en discard binding must survive:\n{code}");
}

#[test]
fn ordo_enum_round_trips_discriminants() {
    // la note: the la-surface `ordo` declaration does not register in this
    // build (pre-existing SEM001 on the declaration alone), so the fixture is
    // en-only — the surface keyword and discriminant emission are shared.
    // en fixture: `enum` surface spelling with discriminants round-trips.
    let mut pack_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    pack_path.push("../../stdlib/locale/en/pack.toml");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack loads");
    let session = Session::new(Config::default().with_locale_pack(pack));
    let result = compile_canonical(
        &session,
        "ordo-en.fab",
        "enum Level {\n    One = 1,\n    Two = 2\n}\n\nmain {\n    const Level level ← One\n    print level\n}\n",
    );
    assert!(
        result.success(),
        "{:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
    let code = result.output.expect("emit").code;
    assert!(code.contains("enum Level {"), "en enum head must survive:\n{code}");
    assert!(code.contains("One = 1,"), "en discriminant must survive:\n{code}");
    assert!(!code.contains("union Level"), "enum must not become union:\n{code}");
}
