//! Re-emission fidelity round-trips driven from source text.
//!
//! Each test compiles a source document through the full canonical path
//! (parse → lower → analyze → emit, presentation attached) and locks two
//! contracts: the emitted spelling for the construct under test, and
//! idempotence (re-emitting the emitted source is byte-identical).

use crate::Target;
use crate::driver::reemit_faber;
use crate::driver::{AnalyzeRequest, Config, ModuleImportContract, Session, analyze_source};
use crate::file_interface::{FileExport, FileExportKind, FileInterface, InterfaceEnumVariant, InterfaceNominalExport};
use crate::import_resolve::library_home_test_support;
use crate::program::compile::prepare_program_module_for_name;
use crate::semantic::passes::visibility_annotation::VisibilityTier;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn latin_source_session() -> Session {
    Session::new(Config::default().with_latin_locale().with_dev_locale_root())
}

fn emit_once(source: &str) -> String {
    let session = latin_source_session();
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
fn reduction_surface_round_trips_in_latin_english_and_simplified_chinese() {
    let cases = [
        (
            "la",
            "reducta via summa ex",
            "functio total(tf32[4] xs) → f32 { redde reducta via summa ex xs }\n",
            "functio total(tf32[4] xs) → f32 { redde reducta via unknown ex xs }\n",
            "functio total(tf32[4] xs) → f32 { redde reducta ex xs }\n",
            "summa, factum, maxima, minima, argmaxima, argminima, omnia, quilibet, numeratio, prima",
        ),
        (
            "en",
            "reduce via sum from",
            "fn total(tf32[4] xs) → f32 { return reduce via sum from xs }\n",
            "fn total(tf32[4] xs) → f32 { return reduce via unknown from xs }\n",
            "fn total(tf32[4] xs) → f32 { return reduce from xs }\n",
            "sum, product, max, min, argmax, argmin, all, any, count, first",
        ),
        (
            "zh-Hans",
            "归约 经由 求和 取自",
            "函数 total(tf32[4] xs) → f32 { 返回 归约 经由 求和 取自 xs }\n",
            "函数 total(tf32[4] xs) → f32 { 返回 归约 经由 unknown 取自 xs }\n",
            "函数 total(tf32[4] xs) → f32 { 返回 归约 取自 xs }\n",
            "求和, 求积, 最大, 最小, 最大值索引, 最小值索引, 全部, 任一, 计数, 第一个",
        ),
    ];
    for (locale, surface, source, invalid_source, missing_via_source, operators) in cases {
        let pack = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(locale))
            .unwrap_or_else(|error| panic!("{locale} pack loads: {error:?}"));
        let session = Session::new(
            Config::default()
                .with_latin_locale()
                .with_resolved_locale_pack(pack.clone()),
        );
        let emit = |input: &str| {
            let result = reemit_faber(&session, &pack, "reducta.fab", input);
            assert!(
                result.success(),
                "{locale}: {:?}",
                result
                    .diagnostics
                    .iter()
                    .map(|diagnostic| (diagnostic.code, diagnostic.issue(), diagnostic.args.clone()))
                    .collect::<Vec<_>>()
            );
            result.output.expect("re-emitted program").code
        };
        let first = emit(source);
        assert!(first.contains(surface), "{locale}: {first}");
        assert_eq!(emit(&first), first, "{locale} reduction output is not idempotent");

        for (invalid, issue) in [
            (invalid_source, "reducta_unknown_operator"),
            (missing_via_source, "reducta_requires_via"),
        ] {
            let diagnostics = match analyze_source(&session, "reducta-invalid.fab", invalid, AnalyzeRequest::default())
            {
                Ok(unit) => unit.diagnostics,
                Err(diagnostics) => diagnostics,
            };
            let diagnostic = diagnostics
                .iter()
                .find(|diagnostic| diagnostic.issue() == Some(issue))
                .unwrap_or_else(|| panic!("{locale}: missing {issue} parse diagnostic: {diagnostics:?}"));
            assert_eq!(diagnostic.code, Some("PARSE001"), "{locale}: {issue}");
            assert_eq!(
                diagnostic
                    .args
                    .iter()
                    .find(|arg| arg.name == "operators")
                    .map(|arg| arg.value.as_str()),
                Some(operators),
                "{locale} {issue} operator list"
            );
        }
    }
}

#[test]
fn scan_surface_round_trips_with_total_order_and_thread_body() {
    let source = "+++\nlocale = \"la\"\n+++\n@ nucleum\nfunctio prefix(tensor<f32, [4]> xs) → tensor<f32, [4]> {\n    redde scan via summa total exclusive ex xs apud [i] order descendens filum lane sit value { redde value }\n}\n";
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_dev_locale_root()
            .with_latin_locale(),
    );
    let pack = crate::locale::latin_locale_pack();
    let emit = |input: &str| {
        let result = reemit_faber(&session, &pack, "scan.fab", input);
        assert!(
            result.success(),
            "compile failed: {:?}",
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code, diagnostic.issue()))
                .collect::<Vec<_>>()
        );
        result.output.expect("emit should produce output").code
    };
    let emitted = emit(source);
    assert_eq!(emit(&emitted), emitted, "scan output is not idempotent");
    assert!(
        emitted.contains("scan via summa total exclusive ex xs apud [i] order descendens filum lane sit value"),
        "canonical scan form missing from output: {emitted}"
    );
}

/// `ordinata via Dir (ex | de) source [body]` re-emits at the source locale,
/// the loop binder stays the loop's, the words stay plain identifiers
/// elsewhere, and every target types the node.
#[test]
fn sort_expression_round_trips_in_latin_and_english_and_typechecks() {
    let cases = [
        (
            "la",
            "functio f(lista<numerus> xs, tabula<textus, numerus> m) → vacuum {\n    sit lista<numerus> a ← ordinata via ascendens ex xs\n    sit lista<textus> k ← ordinata via descendens de m\n    sit lista<numerus> b ← ordinata via ascendens ex xs sit p {\n        redde p\n    }\n    sit lista<textus> s ← ordinata via ascendens de m\n    itera ex s sit n {\n        scribe n\n    }\n    varia numerus ordinata ← 1\n    varia numerus ascendens ← 2\n    varia lista<numerus> ys ← [3, 1]\n    ys.ordina()\n}\n",
            [
                "ordinata via ascendens ex xs",
                "ordinata via descendens de m",
                "ordinata via ascendens ex xs sit p {",
            ],
        ),
        (
            "en",
            "fn f(list<int> xs, map<string, int> m) → none {\n    let list<int> a ← sort via asc from xs\n    let list<string> k ← sort via desc ref m\n    let list<int> b ← sort via asc from xs let p {\n        return p\n    }\n    let list<string> s ← sort via asc ref m\n    for from s let n {\n        print n\n    }\n    mut int sort ← 1\n    mut int asc ← 2\n    mut list<int> ys ← [3, 1]\n    ys.sort()\n}\n",
            [
                "sort via asc from xs",
                "sort via desc ref m",
                "sort via asc from xs let p {",
            ],
        ),
    ];
    for (locale, source, surfaces) in cases {
        let source = if locale == "la" {
            format!("+++\nlocale = \"la\"\n+++\n{source}")
        } else {
            source.to_owned()
        };
        let pack = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(locale))
            .unwrap_or_else(|error| panic!("{locale} pack loads: {error:?}"));
        let session = Session::new(
            Config::default()
                .with_latin_locale()
                .with_resolved_locale_pack(pack.clone()),
        );
        let emit = |input: &str| {
            let result = reemit_faber(&session, &pack, "sort.fab", input);
            assert!(
                result.success(),
                "{locale}: {:?}",
                result
                    .diagnostics
                    .iter()
                    .map(|diagnostic| (diagnostic.code, diagnostic.issue()))
                    .collect::<Vec<_>>()
            );
            result.output.expect("re-emitted program").code
        };
        let first = emit(&source);
        for surface in surfaces {
            assert!(first.contains(surface), "{locale}: missing `{surface}` in\n{first}");
        }
        assert_eq!(emit(&first), first, "{locale} sort output is not idempotent");

        // Typecheck types the node on every target: no sort issue is raised.
        let mut check_session = Session::new(
            Config::default()
                .with_latin_locale()
                .with_resolved_locale_pack(pack.clone()),
        );
        check_session.config.target = Target::HirRust;
        let diagnostics = match analyze_source(&check_session, "sort-check.fab", &first, AnalyzeRequest::default()) {
            Ok(unit) => unit.diagnostics,
            Err(diagnostics) => diagnostics,
        };
        assert!(
            !diagnostics.iter().any(|diagnostic| diagnostic
                .issue()
                .is_some_and(|issue| issue.starts_with("sort_"))),
            "{locale}: unexpected sort diagnostic: {diagnostics:?}"
        );
    }
}

#[test]
fn trap_body_comments_survive_round_trip() {
    for source in [
        "functio f() { sit ∪ r ← capta {\n# trap body\n1\n} }\n",
        "functio f() { sit ∪ r ← capta {\n# preamble\nsit numerus x ← 1\n# trap body\n# second tail comment\nx\n# block end\n} }\n",
    ] {
        let first = emit_once(source);
        let second = emit_once(&first);
        for comment in source.lines().filter(|line| line.starts_with('#')) {
            assert!(first.contains(comment), "first format lost `{comment}`:\n{first}");
            assert!(second.contains(comment), "second format lost `{comment}`:\n{second}");
        }
        assert_eq!(first, second, "trap formatting must be idempotent");
    }
}

#[test]
fn access_marked_parameter_forms_keep_their_inner_type() {
    let source = "genus Box { varia numerus value }\nfunctio update(varia Box box, de lista<numerus> items, varia numerus count, de li32 compact) { tacet }\n";
    let emitted = assert_idempotent(source);
    assert!(emitted.contains("varia Box box"), "{emitted}");
    assert!(emitted.contains("de lista<numerus> items"), "{emitted}");
    assert!(emitted.contains("varia numerus count"), "{emitted}");
    assert!(emitted.contains("de li32 compact"), "{emitted}");
}

/// N7c/N7d: the bare width marker is the only numeric spelling (the parser
/// rejects the wrapped forms), in every type position including a `casu` type
/// pattern; bare `numerus` / `fractus` stay and the policy word keeps its own
/// spelling.
#[test]
fn bare_numeric_markers_re_emit_unchanged() {
    let source = "functio f(u8 a, f32 b, numerus n, exactus<u16> c, lista<i32> xs, u8 ∪ textus u) → f64 {\n    discerne u {\n        casu u8 sit v { tacet }\n        casu textus { tacet }\n    }\n    sit d64 d ← 1\n    redde b ↦ f64\n}\n";
    let emitted = assert_idempotent(source);
    for expected in [
        "f(u8 a, f32 b, numerus n, exactus<u16> c, lista<i32> xs,",
        "casu u8 sit v",
        "sit d64 d",
        "b ↦ f64",
    ] {
        assert!(emitted.contains(expected), "missing `{expected}` in:\n{emitted}");
    }
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
            .with_latin_locale()
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
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_locale_pack(latin.clone()),
    );
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
    let diagnostic =
        crate::driver::faber_format::compile::revalidate_emitted_output(&session, &name, &shortened, &analysis, &latin)
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
    let source = "functio swap() → numerus {\n    sit lista<exactus<u32>> first ← vacua\n    sit lista<u32> second ← vacua\n    redde 0\n}\n";
    let latin = crate::locale::latin_locale_pack();
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_locale_pack(latin.clone()),
    );
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
        .replace("lista<exactus<u32>> first", "lista<u32> first")
        .replace("lista<u32> second", "lista<exactus<u32>> second");
    let diagnostic = crate::driver::faber_format::compile::revalidate_emitted_output(
        &session,
        "swapped-sites.fab",
        &swapped,
        &analysis,
        &latin,
    )
    .expect_err("swapping same-typed authored forms must fail the occurrence-aware gate");
    assert_eq!(diagnostic.code, Some("CODEGEN006"));
    assert_eq!(diagnostic.issue(), Some("authored_type_form_map_diverged"));
}

#[test]
fn optional_union_over_function_alias_keeps_the_alias_spelling() {
    // This uses the neighboring optional-call-flatten exemplar's English
    // function-alias and local-binding syntax. Reconciliation may flatten the
    // alias witness to its function payload, but the authored `Op ∪ none`
    // stays named.
    let pack = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
        .expect("en pack should load");
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_resolved_locale_pack(pack.clone()),
    );
    let emit = |source: &str| {
        let result = reemit_faber(&session, &pack, "optional-alias.fab", source);
        assert!(
            result.success(),
            "optional function alias should re-emit: {:?}",
            result
                .diagnostics
                .iter()
                .map(|diagnostic| (diagnostic.code, diagnostic.issue()))
                .collect::<Vec<_>>()
        );
        result.output.expect("successful output").code
    };
    let source = "type Op = (int) → int ∪ none\n\nfn maybe(int x) → int ∪ none {\n    return null\n}\n\nmain {\n    let Op ∪ none f ← maybe\n}\n";
    let first = emit(source);
    assert!(
        first.contains("let Op ∪ none f"),
        "optional function alias must retain its authored spelling:\n{first}"
    );
    assert_eq!(emit(&first), first, "optional alias output must be idempotent");
}

#[test]
fn canonical_revalidation_rejects_analysis_errors() {
    let latin = crate::locale::latin_locale_pack();
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_locale_pack(latin.clone()),
    );
    let source = "functio noop() → vacuum { }\n";
    let prepared = prepare_program_module_for_name("revalidation.fab", source, &session).expect("prepare valid source");
    let analysis = analyze_source(
        &prepared.session,
        "revalidation.fab",
        source,
        AnalyzeRequest { cli_program: None, contract: prepared.contract },
    )
    .expect("analyze valid source");
    let diagnostic = crate::driver::faber_format::compile::revalidate_emitted_output(
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
    // The bundle is only a binding here (no CLI program, so it is `ignotum`);
    // it is printed whole because `ignotum` has no fields to project.
    let code = assert_idempotent("functio greet(textus nomen) argumenta args → vacuum {\n    scribe args\n}\n");
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
fn conversio_via_clause_round_trips() {
    // D11.9: a convert hint is a `via` clause after the target, never a type
    // argument, and the canonical emitter keeps it there.
    //
    // D5.7 retired top-level `fixum`; this binding moves inside `incipit`
    // (the clause spelling under test is unrelated to item vs. local scope).
    let code = assert_idempotent("incipit {\n    sit u32 le ← |de ad be ef|[0‥4] ↦ u32 via Be\n}\n");
    assert!(
        code.contains("↦ u32 via Be"),
        "conversio hint must re-emit as a via clause:\n{code}"
    );
}

#[test]
fn littera_code_via_clause_round_trips() {
    // D10.6 `n ↦ littera via Code`: the target stays the plain scalar head,
    // never a float-family width wrap.
    let code = assert_idempotent("incipit {\n    sit littera c ← 65 ↦ littera via Code ⊥ '?'\n    scribe c\n}\n");
    assert!(
        code.contains("↦ littera via Code ⊥ '?'"),
        "the Code hint must re-emit as a via clause on the littera head:\n{code}"
    );
}

#[test]
fn register_target_via_clause_round_trips() {
    // A register target keeps its hint in the clause, not as a bracketed tail.
    let code = assert_idempotent(
        "functio out(vector<u32, magnitudo 4> v) → lista<b8, magnitudo 16> {\n    redde v ↦ lista<b8, magnitudo 16> via Le\n}\n",
    );
    assert!(
        code.contains("↦ lista<b8, magnitudo 16> via Le"),
        "register hint must re-emit as a via clause:\n{code}"
    );
}

#[test]
fn empty_at_a_dense_slot_reemits_as_empty_in_each_locale() {
    // The checker rewrites `empty` at a dense slot into the typed zero; the
    // formatter prints what the author wrote, at the source's own locale, not
    // the `Type {}` it became.
    let cases = [
        (
            "la",
            "functio f() → vacuum {\n    varia tf32[2, 3] blank ← vacua\n}\n",
            "blank ← vacua",
            "{}",
        ),
        (
            "en",
            "fn f() → void {\n    mut tf32[2, 3] blank ← empty\n}\n",
            "blank ← empty",
            "{}",
        ),
        (
            "en",
            "fn f() → void {\n    mut vf32[4] lanes ← empty\n    mut mf32[2, 2] grid ← empty\n}\n",
            "grid ← empty",
            "{}",
        ),
    ];
    for (locale, source, expected, forbidden) in cases {
        let pack = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(locale))
            .unwrap_or_else(|error| panic!("{locale} pack loads: {error:?}"));
        let session = Session::new(
            Config::default()
                .with_latin_locale()
                .with_resolved_locale_pack(pack.clone()),
        );
        let emit = |input: &str| {
            let result = reemit_faber(&session, &pack, "empty_dense.fab", input);
            assert!(
                result.success(),
                "{locale}: {:?}",
                result
                    .diagnostics
                    .iter()
                    .map(|diag| (diag.code, diag.issue()))
                    .collect::<Vec<_>>()
            );
            result.output.expect("emit should produce output").code
        };
        let first = emit(source);
        assert!(first.contains(expected), "{locale}: `empty` must survive:\n{first}");
        assert!(!first.contains(forbidden), "{locale}: no typed-zero braces:\n{first}");
        assert_eq!(first, emit(&first), "{locale}: re-emission must be idempotent");
    }
}

#[test]
fn axis_sugar_reemits_as_authored() {
    // Typecheck expands `t.sum(axis n)` / `t.mean(axis n)` into the grouped
    // `__axis` reduction; the formatter must print the authored call and stay
    // a fixed point.
    let pack = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
        .unwrap_or_else(|error| panic!("en pack loads: {error:?}"));
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_resolved_locale_pack(pack.clone()),
    );
    let emit = |input: &str| {
        let result = reemit_faber(&session, &pack, "axis_sugar.fab", input);
        assert!(
            result.success(),
            "{:?}",
            result
                .diagnostics
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        );
        result.output.expect("emit should produce output").code
    };
    let source =
        "fn f(tf32[2, 3] g) → void {\n    let tf32[3] d ← g.sum(axis 0)\n    let tf32[2] m ← g.mean(axis 1)\n}\n";
    let first = emit(source);
    assert!(first.contains("g.sum(axis 0)"), "authored sum axis must survive:\n{first}");
    assert!(first.contains("g.mean(axis 1)"), "authored mean axis must survive:\n{first}");
    assert!(!first.contains("__axis"), "the expansion must not leak:\n{first}");
    assert_eq!(first, emit(&first), "re-emission must be idempotent");
}

#[test]
fn itera_coordinates_survive_reemission() {
    // `itera ex src apud [i] fixum x` — the coordinate binding is
    // loop-visible; dropping it breaks bodies that reference it.
    //
    // D5.7 retired top-level `fixum`; the tensor binding moves inside
    // `incipit` (the coordinate spelling under test is unrelated to scope).
    let code = assert_idempotent(
        "incipit {\n    sit tensor<f32, [3]> linea ← vacua\n    itera ex linea apud [i] sit n {\n        scribe i\n    }\n}\n",
    );
    assert!(
        code.contains("itera ex linea apud [i] sit n"),
        "itera apud coordinates must survive:\n{code}"
    );
}

#[test]
fn nested_filum_metadata_survives_reemission() {
    // K8-R: each `filum` lane binder rides its own loop head; dropping or
    // moving one changes which iterations the independence proof composes.
    let code = assert_idempotent(
        "incipit {\n    varia tensor<numerus, [2]> rows ← vacua\n    varia tensor<numerus, [3]> cols ← vacua\n    varia tensor<numerus, [2, 3]> grid ← vacua\n    itera ex rows apud [i] filum outer sit row {\n        itera ex cols apud [j] filum inner sit col {\n            grid[[i, j]] ← row + col\n        }\n    }\n}\n",
    );
    assert!(
        code.contains("itera ex rows apud [i] filum outer sit row")
            && code.contains("itera ex cols apud [j] filum inner sit col"),
        "nested filum lane binders must survive:\n{code}"
    );
}

#[test]
fn scalar_math_and_vector_lift_survive_reemission() {
    // A2-LIFT: the scalar math method keeps its spelling, and the vector
    // operators keep their operand order and parentheses (`s - v` is not
    // `v - s`; `-(v * s)` is not `(-v) * s`).
    let code = assert_idempotent(
        "functio f(f32 x, vector<f32, magnitudo 3> v) → vector<f32, magnitudo 3> {\n    redde x.radix() - (v * x.sinus()) / x\n}\nfunctio g(f32 x, vector<f32, magnitudo 3> v) → vector<f32, magnitudo 3> {\n    redde -(v + x)\n}\n",
    );
    assert!(
        code.contains("x.radix()") && code.contains("x.sinus()"),
        "scalar math methods must survive:\n{code}"
    );
    assert!(
        code.contains("x.radix() - (v * x.sinus()) / x") || code.contains("x.radix() - v * x.sinus() / x"),
        "vector lift operand order must survive:\n{code}"
    );
    assert!(code.contains("-(v + x)"), "negated sum keeps its parentheses:\n{code}");
}

#[test]
fn radical_glyphs_survive_reemission_on_scalar_and_tensor() {
    // `√`, `∛` and `∜` are prefix sugar for the sqrt/cbrt method twins and the
    // nested sqrt pair; re-emission keeps each authored glyph (the fourth root
    // never prints as `radix().radix()`), and a nested authored `√ √ x` stays
    // two square roots.
    let code = assert_idempotent(
        "functio s(f32 x) → f32 {\n    redde √ x + ∛ x + ∜ x\n}\nfunctio t(tensor<f32, [3]> v) → tensor<f32, [3]> {\n    redde √ v + ∛ v + ∜ v\n}\nfunctio u(f32 x) → f32 {\n    redde √ √ x\n}\nfunctio w(f32 x) → f32 {\n    redde ∜ (x + 1.0)\n}\n",
    );
    for expected in [
        "redde √ x + ∛ x + ∜ x",
        "redde √ v + ∛ v + ∜ v",
        "redde √ √ x",
        "redde ∜ (x + 1.0)",
    ] {
        assert!(code.contains(expected), "`{expected}` must survive:\n{code}");
    }
    assert!(!code.contains("radix().radix()"), "∜ must not respell as nested roots:\n{code}");
}

#[test]
fn template_hole_labels_survive_reemission() {
    // `§{label}` holes lower to the positional `§` form; re-emission restores
    // the authored template text and, for labeled actuals, the labels in
    // authored order. Repeated labels, numbered, plain and mixed holes all
    // keep their text.
    let source = concat!(
        "functio f() → textus {\n",
        "    sit textus labeled ← \"§{greet} §{who}\"(who: \"mundus\", greet: \"salve\")\n",
        "    sit textus repeated ← \"§{n} et §{n}\"(n: \"x\")\n",
        "    sit textus erased ← \"§{greet} mundus\"(\"salve\")\n",
        "    sit textus mixed ← \"§{greet} §\"(\"salve\", \"mundus\")\n",
        "    sit textus numbered ← \"§1 et §0 et §0\"(\"a\", \"b\")\n",
        "    sit textus plain ← \"§ et §\"(\"a\", \"b\")\n",
        "    sit _ captured ← `ubi id = §{id}`(id: \"42\")\n",
        "    redde labeled\n",
        "}\n",
    );
    let code = assert_idempotent(source);
    for expected in [
        "\"§{greet} §{who}\"(who: \"mundus\", greet: \"salve\")",
        "\"§{n} et §{n}\"(n: \"x\")",
        "\"§{greet} mundus\"(\"salve\")",
        "\"§{greet} §\"(\"salve\", \"mundus\")",
        "\"§1 et §0 et §0\"(\"a\", \"b\")",
        "\"§ et §\"(\"a\", \"b\")",
        "`ubi id = §{id}`(id: \"42\")",
    ] {
        assert!(code.contains(expected), "`{expected}` must survive:\n{code}");
    }
}

#[test]
fn reduction_sum_reemits_source_shape() {
    let code = assert_idempotent(
        "functio total(tensor<f32, [8]> a) → f32 {\n    redde reducta via summa ex a apud [i] sit s { redde s }\n}\n",
    );
    assert!(
        code.contains("reducta via summa ex a apud [i] sit s"),
        "sum reduction must re-emit its source shape:\n{code}"
    );
}

#[test]
fn reduction_max_coalesce_parenthesizes_binary_source() {
    let code = assert_idempotent(
        "functio peak(tensor<f32, [4]> row, tensor<f32, [4]> scale) → f32 {\n    redde reducta via maxima ex (row ⊙ scale) vel 0.0\n}\n",
    );
    assert!(
        code.contains("reducta via maxima ex (row ⊙ scale) vel 0.0"),
        "extremum source must stay grouped before the vel tail:\n{code}"
    );
}

#[test]
fn reduction_as_a_binary_operand_stays_grouped() {
    // `reducta via summa ex row / 4.0` re-parses as a sum of `row / 4.0`.
    let code = assert_idempotent(
        "functio mean(tensor<f32, [4]> row) → f32 {\n    redde (reducta via summa ex row) / 4.0\n}\n",
    );
    assert!(
        code.contains("(reducta via summa ex row) / 4.0"),
        "reduction operand must stay grouped before the operator:\n{code}"
    );
}

#[test]
fn inference_hole_locals_stay_holes() {
    // `sit x ← …` must not materialize the inferred type: a materialized
    // bounded-lista annotation narrows later spread legality on re-check.
    //
    // D5.7 retired top-level `fixum`; this binding moves inside `incipit`
    // (the hole spelling under test is unrelated to item vs. local scope).
    let code = assert_idempotent("incipit {\n    sit _ numeri ← [1, 2, 3]\n    scribe numeri\n}\n");
    assert!(code.contains("sit _ numeri ←"), "inference hole must stay a hole:\n{code}");
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
        "incipit {\n    sit ratio<g: f32, w: numerus> point ← ratio<g: f32, w: numerus> { g = 1.0, w = 2 }\n    scribe point.g\n    scribe point.w\n}\n",
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
    let session = Session::new(Config::default().with_latin_locale().with_locale_pack(pack));
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
        "union-hole.fab",
        "main {\n    mut ∪ mixed ← 1\n    mixed ← \"salve\"\n}\n",
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
    assert!(code.contains("mut ∪ mixed ←"), "union hole must stay a union hole:\n{code}");
}

#[test]
fn nondum_availability_projects_through_en_pack() {
    // The availability family is a contextual keyword: `unstable` under en.
    let pack_path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack loads");
    let session = Session::new(Config::default().with_latin_locale().with_locale_pack(pack));
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
    let session = Session::new(Config::default().with_latin_locale().with_locale_pack(pack));
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
    let session = Session::new(Config::default().with_latin_locale().with_locale_pack(pack));
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
    // la fixture: `sit T _ ← expr` is a discard binding, not a bare effect
    // call; the arrow count must survive (gradus generation.fab elision).
    let la = assert_idempotent("functio vacua() → numerus {\n    sit numerus _ ← 6 * 7\n    redde 1\n}\n");
    assert!(la.contains("sit numerus _ ← 6 * 7"), "la discard binding must survive:\n{la}");

    // en fixture: `let T _ ← expr` keeps the authored declaration form.
    let pack_path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed en pack loads");
    let session = Session::new(Config::default().with_latin_locale().with_locale_pack(pack));
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
        "discard-en.fab",
        "fn probe() → int {\n    let int _ ← 6 * 7\n    return 1\n}\n",
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
    assert!(code.contains("let int _ ← 6 * 7"), "en discard binding must survive:\n{code}");
}

#[test]
fn await_assignment_and_morphology_keep_their_authored_forms() {
    let code = assert_idempotent(
        "functio fetch() fiet → numerus {\n    redde 7\n}\n\nfunctio use_both() fiet → numerus {\n    sit numerus arrow ↢ fetch()\n    figendum numerus morphology ← fetch()\n    redde arrow + morphology\n}\n",
    );
    assert!(
        code.contains("sit numerus arrow ↢ fetch()"),
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
    let session = Session::new(Config::default().with_latin_locale().with_locale_pack(pack));
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("emission locale pack"),
        "ordo-en.fab",
        "enum Level {\n    One = 1,\n    Two = 2\n}\n\nmain {\n    let Level level ← One\n    print level\n}\n",
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
    let source = "incipit {\n    sit _ xs ← [1, 2, 3, 4]\n    sit [a, _, ceteri r] ← xs\n    varia [c, d] ← [5, 6]\n    nota a\n    nota r\n    nota c + d\n}\n";
    let emitted = assert_idempotent(source);
    assert!(emitted.contains("sit [a, _, ceteri r] ← xs\n"), "{emitted}");
    assert!(emitted.contains("varia [c, d] ← [5, 6]\n"), "{emitted}");
    assert!(!emitted.contains("__destructura"), "{emitted}");
}

#[test]
fn object_destructuring_reemits_as_pattern() {
    let source = "genus Punctum {\n    varia numerus x\n    varia numerus y\n}\nfunctio fac() → Punctum {\n    redde Punctum { x = 1, y = 2 }\n}\nincipit {\n    sit {x, y ut z} ← fac()\n    nota x + z\n}\n";
    let emitted = assert_idempotent(source);
    assert!(emitted.contains("sit {x, y ut z} ← fac()\n"), "{emitted}");
}

#[test]
fn itera_destructuring_binder_reemits_as_pattern() {
    let source = "incipit {\n    sit _ m ← [[1, 2, 3], [4, 5, 6]]\n    itera ex m sit [caput, ceteri cauda] {\n        nota caput\n        nota cauda\n    }\n}\n";
    let emitted = assert_idempotent(source);
    assert!(emitted.contains("itera ex m sit [caput, ceteri cauda] {\n"), "{emitted}");
    assert!(!emitted.contains("__itera_element"), "{emitted}");
}

#[test]
fn ordo_members_keep_their_authored_qualification() {
    let source = "ordo Color {\n    rubrum,\n    viridis\n}\n\nincipit {\n    sit Color color ← Color.rubrum\n    sit Color bare ← viridis\n    elige color {\n        casu Color.rubrum {\n            nota \"rubrum\"\n        }\n        casu viridis {\n            nota \"viridis\"\n        }\n    }\n    nota bare\n}\n";
    let emitted = assert_idempotent(source);
    assert!(emitted.contains("ordo Color {"), "{emitted}");
    assert!(emitted.contains("sit Color color ← Color.rubrum"), "{emitted}");
    assert!(emitted.contains("casu Color.rubrum {"), "{emitted}");
}

#[test]
fn canonical_revalidation_refuses_an_emitter_that_changes_meaning() {
    let source = "functio duplex(numerus x) → numerus {\n    redde x + x\n}\n";
    let latin = crate::locale::latin_locale_pack();
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_locale_pack(latin.clone()),
    );
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
    let diagnostic = crate::driver::faber_format::compile::revalidate_emitted_output(
        &session,
        "meaning.fab",
        &broken,
        &analysis,
        &latin,
    )
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
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_locale_pack(latin.clone()),
    );
    let prepared = prepare_program_module_for_name("layout.fab", source, &session).expect("prepare layout source");
    let analysis = analyze_source(
        &prepared.session,
        "layout.fab",
        source,
        AnalyzeRequest { cli_program: None, contract: prepared.contract },
    )
    .expect("analyze layout source");
    let relaid = "# comment\n\nfunctio duplex(numerus x) → numerus { redde (x + x) }\n";
    crate::driver::faber_format::compile::revalidate_emitted_output(&session, "layout.fab", relaid, &analysis, &latin)
        .expect("layout, trivia, and redundant parentheses do not change meaning");
}

/// Both passes include the unmodified D8.9 HIR meaning proof.
fn emit_closure_body_twice(source: &str, locale: &str) -> String {
    let pack = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(locale))
        .expect("reader pack loads");
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_resolved_locale_pack(pack.clone()),
    );
    let emit = |source: &str| {
        let result = reemit_faber(&session, &pack, "closure.fab", source);
        assert!(result.success(), "{source}\n{:?}", result.diagnostics);
        result.output.expect("closure output").code
    };
    let first = emit(source);
    assert_eq!(emit(&first), first, "closure emission must be idempotent");
    first
}

#[test]
fn closure_body_breakability_legacy_blocks_round_trip_with_and_without_comments() {
    for (locale, function, keyword, ty, return_word) in [
        ("en", "fn", "lambda", "int", "return"),
        ("la", "functio", "clausura", "numerus", "redde"),
    ] {
        for params in [String::new(), format!("{ty} x"), format!("{ty} x, {ty} y")] {
            for comment in ["", "# lambda body\n"] {
                let source =
                    format!("{function} f() {{ {keyword} {params} → {ty} {{\n{comment}{return_word} 1\n}} }}\n");
                let emitted = emit_closure_body_twice(&source, locale);
                assert!(emitted.contains(&format!("{keyword} ")), "{emitted}");
                assert!(!emitted.contains('∴'), "{emitted}");
                assert_eq!(
                    emitted.matches("# lambda body").count(),
                    usize::from(!comment.is_empty()),
                    "{emitted}"
                );
            }
        }
    }
}

#[test]
fn closure_body_breakability_explicit_fac_blocks_remain_compact() {
    for (locale, function, ty, return_word, fac) in [
        ("en", "fn", "int", "return", "do"),
        ("la", "functio", "numerus", "redde", "fac"),
    ] {
        let source = format!("{function} f() {{ () → {ty} ∴ {fac} {{\n# lambda body\n{return_word} 1\n}} }}\n");
        let emitted = emit_closure_body_twice(&source, locale);
        assert!(emitted.contains(&format!("∴ {fac} {{")), "{emitted}");
        assert_eq!(emitted.matches("# lambda body").count(), 1, "{emitted}");
    }
}

#[test]
fn closure_body_breakability_revalidation_rejects_added_fac_scope() {
    let source = "functio f() { clausura → numerus { redde 1 } }\n";
    let latin = crate::locale::latin_locale_pack();
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_locale_pack(latin.clone()),
    );
    let prepared = prepare_program_module_for_name("closure.fab", source, &session).expect("prepare closure");
    let analysis = analyze_source(
        &prepared.session,
        "closure.fab",
        source,
        AnalyzeRequest { cli_program: None, contract: prepared.contract },
    )
    .expect("analyze closure");
    let changed = "functio f() { () → numerus ∴ fac { redde 1 } }\n";
    let diagnostic = crate::driver::faber_format::compile::revalidate_emitted_output(
        &session,
        "closure.fab",
        changed,
        &analysis,
        &latin,
    )
    .expect_err("adding a breakable closure scope changes HIR meaning");
    assert_eq!(diagnostic.code, Some("CODEGEN006"));
    assert_eq!(diagnostic.issue(), Some("hir_meaning_diverged"));
}

#[test]
fn plain_fac_block_keeps_its_breakable_scope() {
    let source = "incipit {\n    varia numerus m ← 0\n    fac {\n        fac {\n            m ↑\n            si m ≥ 2 {\n                rumpe\n            }\n        }\n    } dum m ≺ 6\n    nota m\n}\n";
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
        "incipit {\n    sit fractus parvus ← 1e-5\n    sit fractus magnus ← 1.0e15\n    nota parvus, magnus\n}\n";
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
    let session = latin_source_session();
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
    let source = "discretio Fons {\n    Mala {\n        textus detail\n    }\n}\n\nfunctio fac_malam() → Fons {\n    redde finge Mala { detail = \"x\" }\n}\n\nfunctio lege(Fons fons) → textus {\n    discerne fons {\n        casu Fons.Mala sit detail {\n            redde detail\n        }\n    }\n}\n";
    assert_idempotent(source);
}

const FLOAT_SLOT_INTEGER_SOURCE: &str = "genus Intervallum {\n    sit fractus imum\n    sit fractus summum\n    functio medium() → fractus {\n        redde (ego.imum + ego.summum) ÷ 2\n    }\n}\n\nfunctio cubus() → Intervallum {\n    redde Intervallum { imum = 0, summum = 255 }\n}\n";

/// web3: an integer literal in a `fractus` slot is retyped to a float constant
/// by typecheck; its re-emitted `2.0` text re-parses as literal text. Both are
/// the same float, so conversion must succeed in every locale.
#[test]
fn integer_literal_in_float_slot_converts_to_every_locale() {
    let session = latin_source_session();
    for id in ["en", "la", "ar", "hi", "th-TH", "vi", "zh-Hans", "zh-Hant"] {
        let pack = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(id))
            .unwrap_or_else(|error| panic!("installed {id} pack should load: {error:?}"));
        let result = reemit_faber(&session, &pack, "float-slot.fab", FLOAT_SLOT_INTEGER_SOURCE);
        assert!(
            result.success(),
            "{id}: integer literal in a float slot must convert: {:?}",
            result
                .diagnostics
                .iter()
                .map(|diag| (diag.code, diag.issue(), diag.args.clone()))
                .collect::<Vec<_>>()
        );
        let code = result.output.expect("converted output").code;
        assert!(code.contains("2.0") && code.contains("255.0"), "{id}: {code}");
    }
}

/// The float-literal normalization compares values, not spellings: a
/// different float value in the emitted text is still refused.
#[test]
fn float_literal_normalization_still_refuses_a_different_value() {
    let latin = crate::locale::latin_locale_pack();
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_locale_pack(latin.clone()),
    );
    let prepared = prepare_program_module_for_name("float.fab", FLOAT_SLOT_INTEGER_SOURCE, &session)
        .expect("prepare float source");
    let analysis = analyze_source(
        &prepared.session,
        "float.fab",
        FLOAT_SLOT_INTEGER_SOURCE,
        AnalyzeRequest { cli_program: None, contract: prepared.contract },
    )
    .expect("analyze float source");
    let faithful = emit_once(FLOAT_SLOT_INTEGER_SOURCE);
    crate::driver::faber_format::compile::revalidate_emitted_output(
        &session,
        "float.fab",
        &faithful,
        &analysis,
        &latin,
    )
    .expect("the faithful re-emission is the same program");
    for (from, to) in [("255.0", "254.0"), ("÷ 2.0", "÷ 3.0")] {
        assert!(faithful.contains(from), "{faithful}");
        let changed = faithful.replace(from, to);
        let diagnostic = crate::driver::faber_format::compile::revalidate_emitted_output(
            &session,
            "float.fab",
            &changed,
            &analysis,
            &latin,
        )
        .expect_err("a different float value changes meaning");
        assert_eq!(diagnostic.issue(), Some("hir_meaning_diverged"), "{from}");
    }
}

#[test]
fn literal_only_float_expressions_keep_the_authors_text() {
    // F9 ruling 31 folds them for every compile; format and convert print the
    // author's expression, and the meaning re-validation still holds.
    let source =
        "incipit {\n    sit fractus c ← 0.1 + 0.2\n    sit fractus g ← (0.1 + 0.2) * 10.0\n    nota 1.0 / 3.0\n}\n";
    let emitted = assert_idempotent(source);
    for text in ["0.1 + 0.2", "(0.1 + 0.2) * 10.0", "1.0 / 3.0"] {
        assert!(emitted.contains(text), "{text}: {emitted}");
    }
}

#[test]
fn template_application_keeps_guillemets_when_the_text_holds_quotes() {
    // inf-json (U8b): a template with an embedded `"` must stay `«…»` in the
    // reader-locale emit, or the emitted text no longer lexes.
    let source = "incipit {\n    sit textus payload ← «{\"n\":§}»(\"7\")\n    nota payload\n}\n";
    let emitted = assert_idempotent(source);
    assert!(emitted.contains("«{\"n\":§}»(\"7\")"), "{emitted}");
}

/// A left operand of the same precedence regroups under the parser's left fold,
/// so `a + b - c` re-emits without parens. A leading `(` would otherwise read
/// as call syntax after a statement-head `nota`.
#[test]
fn same_precedence_left_chain_re_emits_without_parens() {
    let source = "incipit {\n    sit numerus m ← 9\n    nota m + 1 - 1\n    nota m - 1 - 1\n    nota m / 2 * 3\n    nota m - (1 - 1)\n}\n";
    let emitted = assert_idempotent(source);
    for expected in ["nota m + 1 - 1", "nota m - 1 - 1", "nota m / 2 * 3", "nota m - (1 - 1)"] {
        assert!(emitted.contains(expected), "missing `{expected}` in:\n{emitted}");
    }
}

/// The Latin spelling of `reshape` is the method name `forma`; it is not a use
/// of the builtin `forma` genus, so the re-analysis must not inject that item
/// (the meaning gate would refuse the file as `hir_meaning_diverged`).
#[test]
fn latin_forma_method_name_is_not_a_forma_genus_use() {
    let source = "incipit {\n    sit lista<f32> flat ← [1.0, 2.0, 3.0, 4.0]\n    sit tf32[2, 2] grid ← flat ↦ tf32[2, 2]\n    sit tensor<f32, [2, 2]> reshaped ← grid.forma([_, 2])\n    nota reshaped.magnitudines()\n}\n";
    let emitted = assert_idempotent(source);
    assert!(emitted.contains("grid.forma([_, 2])"), "{emitted}");
}

/// A typed conversation `sermo<O, R>` keeps its authored type arguments: the
/// witness is the erased bare carrier, so only the authored form has them.
#[test]
fn typed_sermo_annotation_keeps_its_arguments() {
    let source = "@ ad 'numeri:twice'\nfunctio twice(numerus n) → numerus {\n    redde n * 2\n}\n\nincipit {\n    sit sermo<numerus, numerus> t ← ad 'numeri:twice' (5)\n    sit sermo loose ← t\n    nota loose ↦ numerus\n}\n";
    let emitted = assert_idempotent(source);
    assert!(emitted.contains("sermo<numerus, numerus> t"), "{emitted}");
    assert!(emitted.contains("sermo loose"), "{emitted}");
}

/// Re-emits `source` at the English reader locale and asserts the output is a
/// fixed point; returns the first emission.
fn emit_en_idempotent(source: &str) -> String {
    let pack = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
        .unwrap_or_else(|error| panic!("en pack loads: {error:?}"));
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_resolved_locale_pack(pack.clone()),
    );
    let emit = |input: &str| {
        let result = reemit_faber(&session, &pack, "inferred_constructor.fab", input);
        assert!(
            result.success(),
            "{:?}",
            result
                .diagnostics
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        );
        result.output.expect("emit should produce output").code
    };
    let first = emit(source);
    assert_eq!(first, emit(&first), "re-emission must be idempotent");
    first
}

#[test]
fn inferred_constructor_map_entries_reemit_as_the_hole() {
    let code = emit_en_idempotent("fn pondus() → map<string, int> {\n    return _ { \"x\": 7 }\n}\n");
    assert!(
        code.contains("return _ { \"x\": 7 }"),
        "map entries must re-emit as the hole:\n{code}"
    );
}

#[test]
fn inferred_constructor_empty_map_reemits_as_the_hole() {
    let code = emit_en_idempotent("fn f() → void {\n    mut map<string, int> m ← _ {}\n}\n");
    assert!(code.contains("m ← _ {}"), "empty map hole must re-emit as the hole:\n{code}");
}

#[test]
fn inferred_constructor_tensor_zero_reemits_as_the_hole() {
    let code = emit_en_idempotent("fn f() → void {\n    mut tf32[2, 3] blank ← _ {}\n}\n");
    assert!(
        code.contains("blank ← _ {}"),
        "tensor zero hole must re-emit as the hole:\n{code}"
    );
}

#[test]
fn inferred_constructor_genus_fields_reemit_as_the_hole() {
    let code = emit_en_idempotent(
        "class Punctum {\n    mut int x = 0\n    mut int y = 0\n}\n\nfn origo() → Punctum {\n    return _ { x = 7, y = 9 }\n}\n",
    );
    assert!(
        code.contains("return _ { x = 7, y = 9 }"),
        "genus hole must re-emit as the hole:\n{code}"
    );
}

#[test]
fn inferred_constructor_at_an_optional_slot_reemits_as_the_hole() {
    let code = emit_en_idempotent(
        "class Punctum {\n    mut int x = 0\n    mut int y = 0\n}\n\nfn origo() → Punctum ∪ none {\n    return _ { x = 7, y = 9 }\n}\n\nfn pondus() → map<string, int> ∪ none {\n    return _ { \"a\": 3 }\n}\n",
    );
    assert!(
        code.contains("return _ { x = 7, y = 9 }"),
        "optional genus hole must re-emit as the hole:\n{code}"
    );
    assert!(
        code.contains("return _ { \"a\": 3 }"),
        "optional map hole must re-emit as the hole:\n{code}"
    );
}

#[test]
fn first_reduction_reemits_with_meaning_intact() {
    let source =
        "functio elige(tf32[3] x) → fractus ∪ nihil {\n    redde reducta via prima ex x sit v { redde v ≻ 0.0 }\n}\n";
    let session = latin_source_session();
    let emit_pack = crate::locale::latin_locale_pack();
    let result = reemit_faber(&session, &emit_pack, "first_reduction.fab", source);
    assert!(
        result.success(),
        "re-emit refused: {:?}",
        result
            .diagnostics
            .iter()
            .map(|diag| (diag.code, diag.issue(), &diag.args))
            .collect::<Vec<_>>()
    );
    let emitted = result.output.expect("emit should produce output").code;
    assert!(emitted.contains("reducta via prima ex x"), "first operator kept: {emitted}");
}

#[test]
fn list_counts_re_emit_after_the_size_marker() {
    // Need: a list count is written after `magnitudo` (en `size`).
    // The count and the range keep their authored spelling: `size N`, the
    // half-open `size 2‥5` and the inclusive `size 1…∞` (the authored range
    // argument of d3f98601).
    let source = "functio f(lista<numerus, magnitudo 4> a, lista<numerus, magnitudo 2‥5> b, lista<numerus, magnitudo 1…∞> c) → numerus { redde 0 }\n";
    let emitted = assert_idempotent(source);
    assert!(emitted.contains("lista<numerus, magnitudo 4> a"), "{emitted}");
    assert!(emitted.contains("lista<numerus, magnitudo 2‥5> b"), "{emitted}");
    assert!(emitted.contains("lista<numerus, magnitudo 1…∞> c"), "{emitted}");
}
