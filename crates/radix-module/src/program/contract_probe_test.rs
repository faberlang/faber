//! Scratch probe (to be refined into the regression).

use crate::file_interface::FileExportKind;
use crate::semantic::SemanticParamMode;
use crate::codegen::Target;
use std::fs;
use std::path::PathBuf;

fn english_config() -> crate::driver::Config {
    let path = crate::locale::workspace_locale_pack_path("en");
    let pack = crate::locale::LocalePack::from_toml_path(path).expect("load English locale pack");
    crate::driver::Config::default()
        .with_target(Target::HirFaber)
        .with_resolved_locale_pack(pack)
}

#[test]
fn probe_contract_mut_mode_for_relative_consumer_paths() {
    let root = std::env::var("PROBE_ROOT").map_or_else(
        |_| {
            let dir = tempfile::TempDir::new().expect("tmp");
            let root = dir.path().to_path_buf();
            fs::create_dir_all(root.join("src")).expect("mkdir");
            fs::write(
                root.join("src/prng.fab"),
                "@ public\nfn next(mut list<b64> state) → u64 {\n    return 0\n}\n",
            )
            .expect("write lib");
            fs::write(
                root.join("src/main.fab"),
                "import from \"./prng\" prng\n\nmain {\n    mut list<b64> rng ← empty\n    print prng.next(mut rng)\n}\n",
            )
            .expect("write main");
            root
        },
        PathBuf::from,
    );
    let source = fs::read_to_string(root.join("src/main.fab")).expect("read");

    std::env::set_current_dir(&root).expect("chdir");

    for name in ["src/main.fab", "./src/main.fab"] {
        // Replicate the contract walk's parse.
        let peeled = crate::driver::peel_raw_source(name, &source);
        println!("--- peel {name}: ok={}" , peeled.is_ok());
        if let Ok(peeled) = peeled {
            let pack = crate::driver::default_code_pack(&crate::driver::Config::default()).expect("pack");
            let lex = crate::lexer::lex_with_locale_pack(peeled.body, &pack);
            println!("    lex success={}", lex.success());
            let parse_result = crate::parser::parse(lex);
            println!("    parse success={}", parse_result.success());
            if let Some(program) = &parse_result.program {
                println!("    stmts={}", program.statements.len());
            } else {
                println!("    program=None");
            }
        }
        let resolved = crate::import_resolve::resolve_namespace_import(
            &crate::driver::Session::new(english_config()),
            std::path::Path::new(name),
            "./b",
            None,
        );
        println!("    resolve ./b from {name}: {:?}", resolved);
        let session = crate::driver::Session::new(english_config());
        let prepared = crate::program::prepare_program_module_for_name(name, &source, &session).expect("prepare");
        let interface = prepared.contract.file_interfaces.get("prng");
        match interface {
            None => println!(
                "--- {name}: NO prng interface; exports keys {:?}",
                prepared.contract.namespace_exports.keys().collect::<Vec<_>>()
            ),
            Some(interface) => {
                let next = interface.exports.get("next");
                match next {
                    None => println!(
                        "--- {name}: interface present, no `next`; keys {:?}",
                        interface.exports.keys().collect::<Vec<_>>()
                    ),
                    Some(export) => {
                        let FileExportKind::Function(func) = &export.kind else { panic!("not fn") };
                        let mode = func.params.first().map(|p| p.mode);
                        println!("--- {name}: next param mode {:?}", mode);
                        assert_eq!(
                            mode,
                            Some(SemanticParamMode::MutRef),
                            "consumer path {name} must carry MutRef"
                        );
                    }
                }
            }
        }
    }

    // Full pipeline: reemit the consumer exactly as `faber format` does.
    let names: Vec<String> = std::env::var("PROBE_NAMES")
        .map(|names| names.split(';').map(str::to_owned).collect())
        .unwrap_or_else(|_| vec!["src/main.fab".to_owned(), "./src/main.fab".to_owned()]);
    for name in &names {
        let session = crate::driver::Session::new(english_config());
        let emit_pack = crate::locale::workspace_locale_pack_path("en");
        let emit_pack = crate::locale::LocalePack::from_toml_path(emit_pack).expect("emit pack");
        let result = crate::driver::reemit_faber(&session, &emit_pack, name, &source);
        let codes: Vec<&str> = result
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .map(|d| d.code.as_deref().unwrap_or("?"))
            .collect();
        println!("--- reemit {name}: output={} errors={codes:?}", result.output.is_some());
        for d in &result.diagnostics {
            if d.is_error() {
                println!("    {} @ {:?} :: {} :: {:?}", d.code.as_deref().unwrap_or("?"), d.span, d.message, d.args);
            }
        }
        if result.output.is_none() {
            // Reproduce the codegen call without revalidation to inspect the text.
            let prepared = crate::program::prepare_program_module_for_name(name, &source, &session).expect("prepare");
            let mut s2 = prepared.session;
            s2.config.keep_authored_constants = true;
            s2.config.target = crate::codegen::Target::HirFaber;
            let analysis = crate::driver::analyze_source(
                &s2,
                name,
                &source,
                crate::driver::AnalyzeRequest { cli_program: None, contract: prepared.contract },
            )
            .expect("analyze");
            let imported_spellings = analysis.resolver.imported_spellings(&analysis.interner);
            let surface = crate::locale::KeywordSurface::locale(&emit_pack);
            let emitted = crate::codegen::faber::FaberCodegen::new(&analysis.types, &analysis.interner)
                .with_imported_spellings(
                    &imported_spellings.by_type,
                    &imported_spellings.by_def,
                    &imported_spellings.by_def_pattern,
                )
                .generate(
                    &analysis.hir,
                    &analysis.presentation,
                    &crate::codegen::faber::LocaleKeywordBridge(surface),
                )
                .expect("codegen");
            println!("--- emitted {name}:\n{}\n---", emitted.code);
        }
    }
}
