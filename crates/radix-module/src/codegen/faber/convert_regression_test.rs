//! Convert-path regression tests for the S1-U2 third repair (paren-drop + def_N).
//!
//! Each test drives the real reader-locale pipeline: an `la` source snippet
//! with the exact defect shape converts to `en` via [`reemit_faber`], the
//! emitted en surface is asserted to carry the regrouped/renamed shape, and
//! the converted output re-parses and typechecks under the en pack (the
//! converted-output-compiles oracle).

use crate::codegen::Target;
use crate::driver::reemit_faber;
use crate::driver::{Config, Session};
use crate::services::check_diagnostics_for_source_with_locale_pack;
use std::fs;

fn en_locale_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
        .expect("installed en pack should load")
}

/// Convert an `la` source to the `en` surface through the canonical HIR
/// re-emit, panicking with the diagnostics when the conversion fails.
fn convert_to_en(source: &str) -> String {
    convert_to_en_named("test.fab", source)
}

/// Convert an `la` source document at `name` to the `en` surface, resolving
/// the file's package-relative imports against `name` on disk.
fn convert_to_en_named(name: &str, source: &str) -> String {
    let pack = en_locale_pack();
    let session = Session::new(
        Config::default()
            .with_target(Target::HirFaber)
            .with_dev_locale_root(),
    );
    let result = reemit_faber(&session, &pack, name, source);
    assert!(
        result.success(),
        "la source at {name} must convert cleanly: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    result
        .output
        .expect("successful convert must carry output")
        .code
}

/// Re-parse and typecheck converted en source under the en pack.
fn assert_en_output_compiles(name: &str, code: &str) {
    let pack = en_locale_pack();
    let result = check_diagnostics_for_source_with_locale_pack(name, code, false, Some(&pack), None);
    assert!(
        result.success,
        "converted en output must re-parse and typecheck ({name}): {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn reemit_real_file_emits_session_pack_not_frontmatter() {
    // RTR1b (`prepare_program_module`) overwrites the session pack with
    // source frontmatter on a real path. Convert must still emit the
    // requested session pack (en), not the input locale (la).
    let td = crate::test_support::test_temp_dir("canonical-emit-pack");
    let path = td.path().join("locale.fab");
    let source = "+++\nlocale = \"la\"\n+++\nfunctio monstra() {\n    nota \"salve\"\n}\n";
    fs::write(&path, source).expect("write locale fixture");
    let code = convert_to_en_named(&path.display().to_string(), source);
    assert!(
        code.contains("fn monstra"),
        "real-file convert must emit the session pack (en), not frontmatter (la):\n{code}"
    );
    assert!(
        code.contains("print \"salve\""),
        "real-file convert must localize nota→print:\n{code}"
    );
    assert!(
        !code.contains("functio"),
        "frontmatter la must not steal the en emit surface:\n{code}"
    );
}

#[test]
fn convert_shift_cast_operand_parenthesizes() {
    // S1-U2 defect 1: `(n ⇒ 24) ↦ numerus<u8>` re-emitted without the operand
    // parens binds as `n ⇒ (24 ↦ numerus<u8>)` on re-parse (the conversio is
    // tighter than the shift in the grammar), so every shift-cast broke
    // serialize.fab (36), train.fab, cache.fab.
    let source = r#"+++
locale = "la"
+++
functio be4(numerus n) → lista<u8> ⇥ textus {
    varia lista<u8> out ← []
    out.appende((n ⇒ 24) ↦ u8)
    redde out
}
"#;
    let code = convert_to_en(source);
    assert!(
        code.contains("(n ⇒ 24) ↦ u8"),
        "shift-cast operand must keep its grouping parens:\n{code}"
    );
    assert!(
        !code.contains("n ⇒ 24 ↦"),
        "the ungrouped shift-cast shape must not be emitted:\n{code}"
    );
    assert_en_output_compiles("shift-cast.en.fab", &code);
}

#[test]
fn convert_list_element_cast_parenthesizes() {
    // S1-U4 defect: list elements carrying `∷` ascriptions must keep the
    // ascription attached to the whole element expression. Without grouping,
    // `n % 256 ∷ int<u8>` re-parses as a list-level/trailing cast shape and
    // changes the arithmetic operand widths.
    let source = r#"+++
locale = "la"
+++
functio casted_bytes(numerus n) → lista<u8> {
    redde [
        (n % 256) ∷ u8,
        ((n ⇒ 8) % 256) ∷ u8
    ]
}
"#;
    let code = convert_to_en(source);
    assert!(
        code.contains("(n % 256) ∷ u8"),
        "list element cast must keep the modulo expression grouped:\n{code}"
    );
    assert!(
        code.contains("((n ⇒ 8) % 256) ∷ u8"),
        "list element cast must keep the shift/modulo expression grouped:\n{code}"
    );
    assert_en_output_compiles("list-element-cast.en.fab", &code);
}

#[test]
fn convert_verte_method_receiver_parenthesizes() {
    // S1-U2 defect 2: `(2.718… ∷ f32).potentia(x)` re-emitted without the
    // receiver parens misparses as `(2.718… ∷ f32.power)(x)` — the ascription
    // target type greedily consumes the member tail — so attention.fab and
    // nn.fab failed SEM002.
    let source = r#"+++
locale = "la"
+++
functio expo(f32 x) → f32 {
    redde (2.718281828459045 ∷ f32).potentia(x)
}
"#;
    let code = convert_to_en(source);
    assert!(
        code.contains("(2.718281828459045 ∷ f32).power(x)"),
        "ascription receiver of a method call must keep its grouping parens:\n{code}"
    );
    assert!(
        !code.contains("∷ f32.power"),
        "the ungrouped ascription-receiver shape must not be emitted:\n{code}"
    );
    assert_en_output_compiles("verte-method.en.fab", &code);
}

#[test]
fn convert_local_backward_companion_call_keeps_spelling() {
    // S1-U2 defect 3: a call to the compiler-generated local backward
    // companion (`loss_backward`) re-emitted as the `def_2147483648` fallback,
    // because the companion has no HIR definition for the name collector.
    // gradient.fab failed SEM001/SEM008 on the def_N identifier.
    let source = r#"+++
locale = "la"
+++
functio nil() → vacuum {
    tacet
}

@ radix lane "air"
@ radix backward "loss_backward"
functio simple_loss(tensor<f32, [2, 2]> x, tensor<f32, [2, 2]> w) → f32 {
    redde (x ⊙ w).media()
}

functio gradientes(tensor<f32, [2, 2]> x, tensor<f32, [2, 2]> w) → f32 {
    fixum _ grads ← loss_backward(x, w, nil(), 1.0)
    fixum [_, gw] ← grads
    redde gw.media()
}
"#;
    let code = convert_to_en(source);
    assert!(
        code.contains("loss_backward(x, w, nil(), 1.0)"),
        "local companion call must keep its registered name:\n{code}"
    );
    assert!(
        !code.contains("def_2"),
        "the def_N fallback must not leak into the companion call:\n{code}"
    );
    assert_en_output_compiles("companion.en.fab", &code);
}

#[test]
fn convert_module_qualified_member_keeps_user_spelling() {
    // S1-U2 defect 4 (repair 5): a module-qualified member call
    // (`parametrum.muta(p)`) re-emitted its member through the target pack's
    // intrinsic rows (`muta` → `replace`) because the module-import receiver
    // is non-nominal and the projection fired on every non-nominal receiver.
    // The member of a module-qualified call names a user callable declared in
    // the imported module (emitted from the canonical symbol as `fn muta`), so
    // projecting it breaks the qualified call with SEM004 namespace-missing-
    // export + SEM010 cascade (gradus optimize.fab). Only genuine
    // intrinsic-capable receivers project. Two-file minimal package, la → en,
    // converted outputs re-checked under the en pack.
    let td = crate::test_support::test_temp_dir("convert-qualified");
    let package_dir = td.path().join("mutarepro");
    let source_dir = package_dir.join("src");
    fs::create_dir_all(&source_dir).expect("package source dir");
    fs::write(
        package_dir.join("faber.toml"),
        r#"[package]
name = "mutarepro"
version = "0.1.0"
edition = "2026"

[library]
provider = "mutarepro"

[paths]
source = "src"

[build]
kind = "lib"
target = "fmir"
targets = ["fmir"]

[reader]
locale = "la"
"#,
    )
    .expect("write manifest");
    let parameter_path = source_dir.join("parameter.fab");
    fs::write(
        &parameter_path,
        r#"+++
locale = "la"
+++
@ publica
functio muta(numerus p) → numerus {
    redde p + 1
}
"#,
    )
    .expect("write parameter module");
    let consumer_path = source_dir.join("optimize.fab");
    let consumer_source = r#"+++
locale = "la"
+++
importa ex "./parameter" parametrum

functio passus(numerus p) → numerus {
    redde parametrum.muta(p)
}
"#;
    fs::write(&consumer_path, consumer_source).expect("write consumer");

    let parameter_name = parameter_path.display().to_string();
    let consumer_name = consumer_path.display().to_string();
    let parameter_code =
        convert_to_en_named(&parameter_name, &fs::read_to_string(&parameter_path).expect("read parameter"));
    let consumer_code = convert_to_en_named(&consumer_name, consumer_source);
    assert!(
        consumer_code.contains("parametrum.muta(p)"),
        "module-qualified member call must keep the user spelling:\n{consumer_code}"
    );
    assert!(
        !consumer_code.contains("parametrum.replace"),
        "the module-qualified member must not project through intrinsic rows:\n{consumer_code}"
    );
    // The converted-output-compiles oracle: write both converted files back
    // with the en surface (frontmatter flipped, mirroring the delivery's
    // flipped package state) and re-check both under the en pack — the
    // re-check feeds the frontmatter-less emitted body (the helper lexes
    // the raw source), while the on-disk files carry the frontmatter so the
    // sibling import contract resolves against the flipped state. The re-check
    // must rebuild the import contract from the overwritten files, not reuse
    // the contract cached while the siblings were still la.
    let en_frontmatter = "+++\nlocale = \"en\"\n+++\n\n";
    fs::write(&parameter_path, format!("{en_frontmatter}{parameter_code}")).expect("write converted parameter");
    fs::write(&consumer_path, format!("{en_frontmatter}{consumer_code}")).expect("write converted consumer");
    let manifest = fs::read_to_string(package_dir.join("faber.toml")).expect("read manifest");
    fs::write(
        package_dir.join("faber.toml"),
        manifest.replace("locale = \"la\"", "locale = \"en\""),
    )
    .expect("flip manifest locale");
    assert_en_output_compiles(&parameter_name, &parameter_code);
    assert_en_output_compiles(&consumer_name, &consumer_code);
}

#[test]
fn convert_intrinsic_receiver_projects_surface_spelling() {
    // S1-U2 defect 4 guard (repair 5): the repair must not narrow the
    // intrinsic projection itself. A genuine intrinsic-method receiver (a
    // collection) still localizes its canonical method name through the en
    // surface (`longitudo` → `length`), which is what keeps the converted
    // output re-parseable under the en pack.
    let source = r#"+++
locale = "la"
+++
functio passus(lista<numerus> x) → numerus {
    redde x.longitudo()
}
"#;
    let code = convert_to_en(source);
    assert!(
        code.contains("x.length()"),
        "intrinsic collection receiver must localize through the en surface:\n{code}"
    );
    assert!(
        !code.contains("x.longitudo"),
        "the canonical intrinsic spelling must not leak into en output:\n{code}"
    );
    assert_en_output_compiles("intrinsic.en.fab", &code);
}

#[test]
fn convert_nominal_receiver_method_keeps_canonical_spelling() {
    // S1-U2 defect 4: a method call on a nominal (class) receiver was
    // localized through the pack's intrinsic rows (`longitudo` → `length`)
    // while the class declaration kept its canonical method name — cache.fab
    // SEM014. Nominal receivers now keep the canonical spelling so the call
    // resolves against the class's own declaration; only intrinsic-capable
    // receivers (collections, primitives) project through the pack surface.
    let source = r#"+++
locale = "la"
+++
genus Tabulatio {
    varia lista<numerus> historia

    functio longitudo() → numerus {
        redde ego.historia.longitudo()
    }
}

functio probe(Tabulatio t) → numerus {
    redde t.longitudo()
}
"#;
    let code = convert_to_en(source);
    assert!(
        code.contains("t.longitudo()"),
        "nominal receiver method call must keep its canonical spelling:\n{code}"
    );
    assert!(
        !code.contains("t.length("),
        "the pack-localized name must not leak into a nominal receiver call:\n{code}"
    );
    assert!(
        code.contains("historia.length()"),
        "intrinsic-capable receivers must still project through the pack surface:\n{code}"
    );
    assert_en_output_compiles("nominal-receiver.en.fab", &code);
}

#[test]
fn deferred_hole_binding_converts_without_materializing_the_inferred_type() {
    // ON-S1: an initializer-free `sit` binding re-emits as a `_` hole.
    // Materializing the inferred type (`const string label`) changes the
    // authored surface and fails output re-validation with
    // `authored_type_form_map_diverged`, which blocked every reader-locale
    // render of the devsite's deferred-binding fence.
    let source = "+++\nlocale = \"la\"\n+++\nincipit {\n    sit label\n    label ← \"deferred\"\n}\n";
    let code = convert_to_en(source);
    assert!(
        code.contains("const _ label"),
        "initializer-free hole binding must re-emit as `_`, not a materialized type:\n{code}"
    );
    assert!(
        !code.contains("string label"),
        "materialized inferred type must not appear in the converted surface:\n{code}"
    );
    assert_en_output_compiles("deferred-hole.fab", &code);
}

#[test]
fn reader_keyword_reverse_lookup_matches_nfkc_normalized_text() {
    // ON-S1: the th-TH `fac` row spells SARA AM composed (U+0E33), which the
    // scanner NFKC-decomposes before keyword lookup; the pack reverse maps
    // must carry NFKC keys or the spelling lexes as an identifier and the
    // fac/cape recovery fence fails re-validation in that locale.
    let source = "+++\nlocale = \"la\"\n+++\nfunctio tutum(numerus a, numerus b) → numerus {\n    fac {\n        redde a / b\n    }\n    cape err {\n        mone err\n        redde 0\n    }\n}\n";
    let pack = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("th-TH"))
        .expect("th-TH pack should load");
    let session = Session::new(
        Config::default()
            .with_target(Target::HirFaber)
            .with_dev_locale_root(),
    );
    let result = reemit_faber(&session, &pack, "test.fab", source);
    assert!(
        result.success(),
        "la→th-TH recovery fence must convert cleanly: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    let code = result
        .output
        .expect("successful convert carries output")
        .code;
    assert!(
        code.contains("\u{0E17}\u{0E33}"),
        "th-TH output must carry the pack's composed fac spelling:\n{code}"
    );
}
