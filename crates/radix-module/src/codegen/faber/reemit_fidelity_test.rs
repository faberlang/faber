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
