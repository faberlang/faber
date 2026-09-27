//! Re-emission fidelity round-trips driven from source text.
//!
//! Each test compiles a source document through the full canonical path
//! (parse → lower → analyze → emit, presentation attached) and locks two
//! contracts: the emitted spelling for the construct under test, and
//! idempotence (re-emitting the emitted source is byte-identical).

use super::compile::reemit_faber;
use crate::Target;
use crate::driver::{AnalyzeRequest, Config, ModuleImportContract, Session, analyze_source};
use crate::file_interface::{FileExport, FileExportKind, FileInterface, InterfaceEnumVariant, InterfaceNominalExport};
use crate::import_resolve::library_home_test_support;
use crate::program::compile::prepare_program_module_for_name;
use crate::semantic::passes::visibility_annotation::VisibilityTier;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn emit_once(source: &str) -> String {
    let session = Session::new(Config::default().with_dev_locale_root());
    let emit_pack = crate::locale::latin_locale_pack();
    let result = reemit_faber(&session, &emit_pack, "fidelity.fab", source);
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
fn access_marked_parameter_forms_keep_their_inner_type() {
    let source = "genus Box { varia numerus value }\nfunctio update(in Box box, de lista<numerus> items, in numerus count, de li32 compact) { tacet }\n";
    let emitted = assert_idempotent(source);
    assert!(emitted.contains("in Box box"), "{emitted}");
    assert!(emitted.contains("de lista<numerus> items"), "{emitted}");
    assert!(emitted.contains("in numerus count"), "{emitted}");
    assert!(emitted.contains("de li32 compact"), "{emitted}");
}

fn imported_token_interface() -> FileInterface {
    let mut interface = FileInterface::new();
    interface.insert(FileExport {
        name: "TokenKind".to_owned(),
        kind: FileExportKind::Enum(InterfaceNominalExport {
            params: Vec::new(),
            name: "TokenKind".to_owned(),
            methods: Vec::new(),
            shared_fields: Vec::new(),
            radix_contract: None,
            variants: vec![InterfaceEnumVariant { name: "Palabra".to_owned(), fields: Vec::new() }],
        }),
    });
    interface
}

struct LibraryHomeGuard {
    previous: Option<PathBuf>,
}

impl LibraryHomeGuard {
    fn set(home: &Path) -> Self {
        let previous = library_home_test_support::set_thread_library_home(Some(home.to_path_buf()));
        Self { previous }
    }
}

impl Drop for LibraryHomeGuard {
    fn drop(&mut self) {
        library_home_test_support::set_thread_library_home(self.previous.take());
    }
}

fn emit_imported_variant_case(source: &str, locale: &str) -> String {
    let pack = if locale == "en" {
        let pack_path = crate::locale::workspace_locale_pack_path("en");
        crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack loads")
    } else {
        crate::locale::latin_locale_pack()
    };
    let session = Session::new(
        Config::default()
            .with_target(Target::HirFaber)
            .with_locale_pack(pack.clone()),
    );
    let contract = ModuleImportContract {
        namespace_exports: BTreeMap::from([(
            "token".to_owned(),
            BTreeMap::from([("TokenKind".to_owned(), VisibilityTier::Publica)]),
        )]),
        file_interfaces: BTreeMap::from([("token".to_owned(), imported_token_interface())]),
        ..ModuleImportContract::default()
    };
    let analysis = analyze_source(
        &session,
        "alias-case.fab",
        source,
        AnalyzeRequest { cli_program: None, contract },
    )
    .unwrap_or_else(|diagnostics| {
        panic!(
            "imported variant analysis failed: {:?}",
            diagnostics
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        )
    });
    let surface = crate::locale::KeywordSurface::new(&pack);
    let output =
        super::super::generate_from_analyzed(Target::HirFaber, &analysis, &surface).expect("imported variant emission");
    let crate::Output::Faber(output) = output else {
        panic!("expected Faber output");
    };
    output.code
}

#[test]
fn imported_alias_qualified_variant_path_round_trips_in_each_locale() {
    let latin = "importa ex \"./token\" token\n\nfunctio classify(token.TokenKind value) → textus {\n    elige value {\n        casu token.TokenKind.Palabra {\n            redde \"palabra\"\n        }\n        ceterum {\n            redde \"other\"\n        }\n    }\n}\n";
    let latin_first = emit_imported_variant_case(latin, "la");
    let latin_second = emit_imported_variant_case(&latin_first, "la");
    assert_eq!(latin_first, latin_second, "Latin re-emission must be idempotent");
    assert!(
        latin_first.contains("casu token.TokenKind.Palabra"),
        "Latin alias-qualified variant path must survive:\n{latin_first}"
    );
    let latin_short = latin.replace("token.TokenKind.Palabra", "token.Palabra");
    let latin_short_output = emit_imported_variant_case(&latin_short, "la");
    assert!(
        latin_short_output.contains("casu token.Palabra"),
        "Latin two-part alias path must not be widened:\n{latin_short_output}"
    );

    let english = "import from \"./token\" token\n\nfn classify(token.TokenKind value) → string {\n    switch value {\n        case token.TokenKind.Palabra {\n            return \"palabra\"\n        }\n        default {\n            return \"other\"\n        }\n    }\n}\n";
    let english_first = emit_imported_variant_case(english, "en");
    let english_second = emit_imported_variant_case(&english_first, "en");
    assert_eq!(english_first, english_second, "English re-emission must be idempotent");
    assert!(
        english_first.contains("case token.TokenKind.Palabra"),
        "English alias-qualified variant path must survive:\n{english_first}"
    );
    let english_short = english.replace("token.TokenKind.Palabra", "token.Palabra");
    let english_short_output = emit_imported_variant_case(&english_short, "en");
    assert!(
        english_short_output.contains("case token.Palabra"),
        "English two-part alias path must not be widened:\n{english_short_output}"
    );
}

#[test]
fn canonical_revalidation_rejects_shortened_imported_variant_path() {
    let td = crate::test_support::test_temp_dir("canonical-revalidation-variant-path");
    let home = td.path().join("library-home");
    let provider_dir = home.join("canonicalrevalidation");
    let source_dir = provider_dir.join("src");
    fs::create_dir_all(&source_dir).expect("create provider source directory");
    fs::write(
        provider_dir.join("faber.toml"),
        r#"[package]
name = "canonicalrevalidation"
version = "0.1.0"
edition = "2026"

[library]
provider = "canonicalrevalidation"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["fmir"]

[locale]
locale = "la"
"#,
    )
    .expect("write provider manifest");
    let provider_path = source_dir.join("token.fab");
    let consumer_path = td.path().join("consumer.fab");
    fs::write(&provider_path, "@ publica\ndiscretio TokenKind {\n    Palabra\n}\n")
        .expect("write imported variant provider");
    let source = "importa ex \"canonicalrevalidation:token\" token\n\nfunctio classify(token.TokenKind value) → textus {\n    elige value {\n        casu token.TokenKind.Palabra {\n            redde \"palabra\"\n        }\n        ceterum {\n            redde \"other\"\n        }\n    }\n}\n";
    fs::write(&consumer_path, source).expect("write imported variant consumer");

    let _library_home = LibraryHomeGuard::set(&home);
    let latin = crate::locale::latin_locale_pack();
    let session = Session::new(Config::default().with_locale_pack(latin.clone()));
    let name = consumer_path.display().to_string();
    let prepared = prepare_program_module_for_name(&name, source, &session).expect("prepare imported variant source");
    let analysis = analyze_source(
        &prepared.session,
        &name,
        source,
        AnalyzeRequest { cli_program: None, contract: prepared.contract },
    )
    .expect("analyze imported variant source");
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
        &name,
        source,
    );
    assert!(
        result.success(),
        "faithful imported variant output must pass the gate: {:?}",
        result.diagnostics
    );
    let output = result.output.expect("faithful output").code;
    assert!(
        output.contains("token.TokenKind.Palabra"),
        "expected qualified output:\n{output}"
    );

    let shortened = output.replace("token.TokenKind.Palabra", "token.Palabra");
    let diagnostic = super::compile::revalidate_emitted_output(&session, &name, &shortened, &analysis, &latin)
        .expect_err("shortened imported variant path must fail re-validation");
    assert_eq!(diagnostic.code, Some("CODEGEN006"));
    assert_eq!(diagnostic.issue(), Some("variant_path_spelling_multiset_diverged"));
    assert_eq!(
        diagnostic
            .args
            .iter()
            .find(|arg| arg.name == "expected_count")
            .map(|arg| arg.value.as_str()),
        Some("1")
    );
    assert_eq!(
        diagnostic
            .args
            .iter()
            .find(|arg| arg.name == "actual_count")
            .map(|arg| arg.value.as_str()),
        Some("1")
    );
}

#[test]
fn canonical_revalidation_rejects_swapped_same_typed_sites() {
    let source = "functio swap() → numerus {\n    fixum lista<numerus<u32>> first ← vacua\n    fixum lista<u32> second ← vacua\n    redde 0\n}\n";
    let latin = crate::locale::latin_locale_pack();
    let session = Session::new(Config::default().with_locale_pack(latin.clone()));
    let prepared =
        prepare_program_module_for_name("swapped-sites.fab", source, &session).expect("prepare swapped-site source");
    let analysis = analyze_source(
        &prepared.session,
        "swapped-sites.fab",
        source,
        AnalyzeRequest { cli_program: None, contract: prepared.contract },
    )
    .expect("analyze swapped-site source");
    let output = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
        "swapped-sites.fab",
        source,
    );
    assert!(output.success(), "faithful source must pass: {:?}", output.diagnostics);
    let emitted = output.output.expect("faithful output").code;
    let swapped = emitted
        .replace("lista<numerus<u32>> first", "lista<u32> first")
        .replace("lista<u32> second", "lista<numerus<u32>> second");
    let diagnostic =
        super::compile::revalidate_emitted_output(&session, "swapped-sites.fab", &swapped, &analysis, &latin)
            .expect_err("swapping same-typed authored forms must fail the occurrence-aware gate");
    assert_eq!(diagnostic.code, Some("CODEGEN006"));
    assert_eq!(diagnostic.issue(), Some("authored_type_form_map_diverged"));
}

#[test]
fn canonical_revalidation_rejects_analysis_errors() {
    let latin = crate::locale::latin_locale_pack();
    let session = Session::new(Config::default().with_locale_pack(latin.clone()));
    let source = "functio noop() → vacuum { }\n";
    let prepared = prepare_program_module_for_name("revalidation.fab", source, &session).expect("prepare valid source");
    let analysis = analyze_source(
        &prepared.session,
        "revalidation.fab",
        source,
        AnalyzeRequest { cli_program: None, contract: prepared.contract },
    )
    .expect("analyze valid source");
    let diagnostic = super::compile::revalidate_emitted_output(
        &session,
        "revalidation.fab",
        "functio noop( → vacuum { }\n",
        &analysis,
        &latin,
    )
    .expect_err("invalid emitted source must fail re-validation");
    assert_eq!(diagnostic.code, Some("CODEGEN006"));
    assert_eq!(diagnostic.issue(), Some("variant_path_spelling_reanalysis_failed"));
    assert_eq!(
        diagnostic
            .args
            .iter()
            .find(|arg| arg.name == "expected_count")
            .map(|arg| arg.value.as_str()),
        Some("0")
    );
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
    //
    // D5.7 retired top-level `fixum`/`varia` entirely (SEM062), including
    // under an implicit entry: `lower_program` still lowers any top-level
    // `fixum`/`varia` statement to a module item, not an implicit-entry
    // local. The fixture below uses only executable statements, which is
    // what this test actually exercises (the absence of an `incipit`
    // wrapper), not a declaration.
    let code = assert_idempotent("scribe 40 + 2\nscribe 41 + 1\n");
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
    //
    // D5.7 retired top-level `fixum`; this binding moves inside `incipit`
    // (the marker spelling under test is unrelated to item vs. local scope).
    let code = assert_idempotent("incipit {\n    fixum numerus<u32> le ← |de ad be ef|[0‥4] ↦ numerus<u32, Be>\n}\n");
    assert!(
        code.contains("↦ numerus<u32, Be>"),
        "conversio marker must fold into the target generics:\n{code}"
    );
}

#[test]
fn littera_code_marker_rides_the_scalar_head() {
    // D10.6 `n ↦ littera<Code>`: the hint is the scalar head's only generic
    // argument, never the float-family width wrap (`fractus<littera, Code>`).
    let code = assert_idempotent("incipit {\n    fixum littera c ← 65 ↦ littera<Code> ⊥ '?'\n    scribe c\n}\n");
    assert!(
        code.contains("↦ littera<Code>"),
        "the Code hint must stay on the littera head:\n{code}"
    );
}

#[test]
fn itera_coordinates_survive_reemission() {
    // `itera ex src apud [i] fixum x` — the coordinate binding is
    // loop-visible; dropping it breaks bodies that reference it.
    //
    // D5.7 retired top-level `fixum`; the tensor binding moves inside
    // `incipit` (the coordinate spelling under test is unrelated to scope).
    let code = assert_idempotent(
        "incipit {\n    fixum tensor<fractus<f32>, [3]> linea ← vacua\n    itera ex linea apud [i] fixum n {\n        scribe i\n    }\n}\n",
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
    //
    // D5.7 retired top-level `fixum`; this binding moves inside `incipit`
    // (the hole spelling under test is unrelated to item vs. local scope).
    let code = assert_idempotent("incipit {\n    fixum _ numeri ← [1, 2, 3]\n    scribe numeri\n}\n");
    assert!(code.contains("fixum _ numeri ←"), "inference hole must stay a hole:\n{code}");
}

#[test]
fn ratio_authored_type_stays_stable_after_witness_normalization() {
    // Record fields are stored in canonical map order, while authored ratio
    // children retain source order. Re-emission must normalize both analyses
    // without changing the authored ratio declaration.
    //
    // D5.7 retired top-level `fixum`; this binding moves inside `incipit`
    // (the ratio-type spelling under test is unrelated to item vs. local scope).
    let code = assert_idempotent(
        "incipit {\n    fixum ratio<g: f32, w: numerus> point ← ratio<g: f32, w: numerus> { g = 1.0, w = 2 }\n    scribe point.g\n    scribe point.w\n}\n",
    );
    assert!(
        code.contains("ratio<g: f32, w: numerus> point"),
        "ratio authored type must survive witness normalization:\n{code}"
    );
}

#[test]
fn union_hole_locals_stay_union_holes() {
    // `var ∪ x` keeps accepting flow members; materializing the solved
    // union would freeze it.
    let pack_path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack loads");
    let session = Session::new(Config::default().with_locale_pack(pack));
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
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
    let pack_path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack loads");
    let session = Session::new(Config::default().with_locale_pack(pack));
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
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
    let pack_path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack loads");
    let session = Session::new(Config::default().with_locale_pack(pack));
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
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
    let pack_path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack loads");
    let session = Session::new(Config::default().with_locale_pack(pack));
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
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
    let pack_path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack loads");
    let session = Session::new(Config::default().with_locale_pack(pack));
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
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
fn await_assignment_and_morphology_keep_their_authored_forms() {
    let code = assert_idempotent(
        "functio fetch() fiet → numerus {\n    redde 7\n}\n\nfunctio use_both() fiet → numerus {\n    fixum numerus arrow ↢ fetch()\n    figendum numerus morphology ← fetch()\n    redde arrow + morphology\n}\n",
    );
    assert!(
        code.contains("fixum numerus arrow ↢ fetch()"),
        "await-directed assignment must survive:\n{code}"
    );
    assert!(
        code.contains("figendum numerus morphology ← fetch()"),
        "morphological await binding must remain unchanged:\n{code}"
    );
}

#[test]
fn ordo_enum_round_trips_discriminants() {
    // la note: the la-surface `ordo` declaration does not register in this
    // build (pre-existing SEM001 on the declaration alone), so the fixture is
    // en-only — the surface keyword and discriminant emission are shared.
    // en fixture: `enum` surface spelling with discriminants round-trips.
    let pack_path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack loads");
    let session = Session::new(Config::default().with_locale_pack(pack));
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
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

#[test]
fn array_destructuring_reemits_as_pattern() {
    let source = "incipit {\n    fixum _ xs ← [1, 2, 3, 4]\n    fixum [a, _, ceteri r] ← xs\n    varia [c, d] ← [5, 6]\n    nota a\n    nota r\n    nota c + d\n}\n";
    let emitted = assert_idempotent(source);
    assert!(emitted.contains("fixum [a, _, ceteri r] ← xs\n"), "{emitted}");
    assert!(emitted.contains("varia [c, d] ← [5, 6]\n"), "{emitted}");
    assert!(!emitted.contains("__destructura"), "{emitted}");
}

#[test]
fn object_destructuring_reemits_as_pattern() {
    let source = "genus Punctum {\n    varia numerus x\n    varia numerus y\n}\nfunctio fac() → Punctum {\n    redde Punctum { x = 1, y = 2 }\n}\nincipit {\n    fixum {x, y ut z} ← fac()\n    nota x + z\n}\n";
    let emitted = assert_idempotent(source);
    assert!(emitted.contains("fixum {x, y ut z} ← fac()\n"), "{emitted}");
}

#[test]
fn itera_destructuring_binder_reemits_as_pattern() {
    let source = "incipit {\n    fixum _ m ← [[1, 2, 3], [4, 5, 6]]\n    itera ex m fixum [caput, ceteri cauda] {\n        nota caput\n        nota cauda\n    }\n}\n";
    let emitted = assert_idempotent(source);
    assert!(emitted.contains("itera ex m fixum [caput, ceteri cauda] {\n"), "{emitted}");
    assert!(!emitted.contains("__itera_element"), "{emitted}");
}

#[test]
fn ordo_members_keep_their_authored_qualification() {
    let source = "ordo Color {\n    rubrum,\n    viridis\n}\n\nincipit {\n    fixum Color color ← Color.rubrum\n    fixum Color bare ← viridis\n    elige color {\n        casu Color.rubrum {\n            nota \"rubrum\"\n        }\n        casu viridis {\n            nota \"viridis\"\n        }\n    }\n    nota bare\n}\n";
    let emitted = assert_idempotent(source);
    assert!(emitted.contains("ordo Color {"), "{emitted}");
    assert!(emitted.contains("fixum Color color ← Color.rubrum"), "{emitted}");
    assert!(emitted.contains("casu Color.rubrum {"), "{emitted}");
}

#[test]
fn canonical_revalidation_refuses_an_emitter_that_changes_meaning() {
    let source = "functio duplex(numerus x) → numerus {\n    redde x + x\n}\n";
    let latin = crate::locale::latin_locale_pack();
    let session = Session::new(Config::default().with_locale_pack(latin.clone()));
    let prepared = prepare_program_module_for_name("meaning.fab", source, &session).expect("prepare meaning source");
    let analysis = analyze_source(
        &prepared.session,
        "meaning.fab",
        source,
        AnalyzeRequest { cli_program: None, contract: prepared.contract },
    )
    .expect("analyze meaning source");
    let output = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
        "meaning.fab",
        source,
    );
    assert!(output.success(), "faithful source must pass: {:?}", output.diagnostics);
    let emitted = output.output.expect("faithful output").code;
    assert!(emitted.contains("x + x"), "{emitted}");

    // A deliberately broken emitter: same shape, same types, other meaning.
    let broken = emitted.replace("x + x", "x * x");
    let diagnostic = super::compile::revalidate_emitted_output(&session, "meaning.fab", &broken, &analysis, &latin)
        .expect_err("an emitter that changes meaning must be refused");
    assert_eq!(diagnostic.code, Some("CODEGEN006"));
    assert_eq!(diagnostic.issue(), Some("hir_meaning_diverged"));
    assert_eq!(
        diagnostic
            .args
            .iter()
            .find(|arg| arg.name == "segment")
            .map(|arg| arg.value.as_str()),
        Some("item 0 (duplex)")
    );
}

#[test]
fn canonical_revalidation_accepts_layout_only_changes() {
    let source = "functio duplex(numerus x) → numerus {\n    redde x + x\n}\n";
    let latin = crate::locale::latin_locale_pack();
    let session = Session::new(Config::default().with_locale_pack(latin.clone()));
    let prepared = prepare_program_module_for_name("layout.fab", source, &session).expect("prepare layout source");
    let analysis = analyze_source(
        &prepared.session,
        "layout.fab",
        source,
        AnalyzeRequest { cli_program: None, contract: prepared.contract },
    )
    .expect("analyze layout source");
    let relaid = "# comment\n\nfunctio duplex(numerus x) → numerus { redde (x + x) }\n";
    super::compile::revalidate_emitted_output(&session, "layout.fab", relaid, &analysis, &latin)
        .expect("layout, trivia, and redundant parentheses do not change meaning");
}

#[test]
fn plain_fac_block_keeps_its_breakable_scope() {
    let source = "incipit {\n    varia numerus m ← 0\n    fac {\n        fac {\n            m ↑\n            si m ≥ 2 {\n                rumpe\n            }\n        }\n    } dum m < 6\n    nota m\n}\n";
    let emitted = assert_idempotent(source);
    assert_eq!(emitted.matches("fac {").count(), 2, "{emitted}");
}

#[test]
fn elige_attached_cape_stays_attached() {
    let source = "functio electio(bivalens malum) → numerus {\n    elige malum {\n        casu verum {\n            iace \"x\"\n        }\n        ceterum {\n            redde 2\n        }\n    } cape err {\n        redde 0\n    }\n}\n\nincipit {\n    nota electio(verum)\n}\n";
    let emitted = assert_idempotent(source);
    assert!(emitted.contains("} cape err {"), "{emitted}");
    assert!(!emitted.contains("fac {"), "{emitted}");
}

#[test]
fn float_literals_keep_their_authored_digits() {
    let source =
        "incipit {\n    fixum fractus parvus ← 1e-5\n    fixum fractus magnus ← 1.0e15\n    nota parvus, magnus\n}\n";
    let emitted = assert_idempotent(source);
    assert!(emitted.contains("← 1e-5"), "{emitted}");
    assert!(emitted.contains("← 1.0e15"), "{emitted}");
}

#[test]
fn record_field_order_is_not_a_meaning_change() {
    // The CLI args record is a field set keyed by analysis-local symbols; a
    // conversion that interns `verbose` before `nomen` must still prove the
    // same program.
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/cli/cli.fab");
    let source = &fs::read_to_string(&path).expect("read corpus/cli/cli.fab");
    let session = Session::new(
        Config::default()
            .with_target(Target::HirFaber)
            .with_dev_locale_root(),
    );
    let la = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("installed la pack");
    let result = reemit_faber(&session, &la, "record-order.fab", source);
    assert!(
        result.success(),
        "{:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn mixed_bare_and_qualified_variant_references_stay_provable() {
    // `casu Fons.Mala` is qualified while `finge Mala` is bare. The spelling
    // sidecar is keyed by variant, so the re-emitter falls back to bare for
    // both; the variant is the same, so the program is unchanged.
    let source = "discretio Fons {\n    Mala {\n        textus detail\n    }\n}\n\nfunctio fac_malam() → Fons {\n    redde finge Mala { detail = \"x\" }\n}\n\nfunctio lege(Fons fons) → textus {\n    discerne fons {\n        casu Fons.Mala fixum detail {\n            redde detail\n        }\n    }\n}\n";
    assert_idempotent(source);
}
