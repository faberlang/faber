//! `regio` (en `module`) survives Faber re-emission and anchors the module
//! doc (DECISIONS D7.3, D7.6, D7.7).
//!
//! The declaration re-emits first, in the target locale, with every comment
//! above it. The analyzed module records only the comment block directly
//! above the declaration as the module doc; a block separated from it by a
//! blank line is a note.

use crate::driver::reemit_faber;
use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::locale::LocalePack;

fn pack(locale: &str) -> LocalePack {
    LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(locale))
        .unwrap_or_else(|err| panic!("installed {locale} pack should load: {err:?}"))
}

fn session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::HirFaber)
            .with_dev_locale_root(),
    )
}

fn reemit(source: &str, to: &str) -> String {
    let result = reemit_faber(&session(), &pack(to), "test.fab", source);
    assert!(
        result.success(),
        "source must re-emit to {to}:\n{source}\n{:?}",
        result
            .diagnostics
            .iter()
            .map(|d| (d.code, d.issue(), d.args.clone()))
            .collect::<Vec<_>>()
    );
    result
        .output
        .expect("successful re-emit must carry output")
        .code
}

const LA_BODY: &str = "# Nota, non doctrina.\n\n# Geometria plana.\n# Puncta et lineae.\nregio geometria\n\n@ publica\nfunctio unum() → numerus {\n    redde 1\n}\n";

fn with_locale(locale: &str, body: &str) -> String {
    format!("+++\nlocale = \"{locale}\"\n+++\n{body}")
}

#[test]
fn la_format_keeps_regio_and_its_comments_first() {
    let formatted = reemit(&with_locale("la", LA_BODY), "la");
    assert_eq!(formatted.trim_end(), LA_BODY.trim_end());
}

#[test]
fn convert_to_en_spells_module_and_back() {
    let en = reemit(&with_locale("la", LA_BODY), "en");
    assert!(en.contains("# Puncta et lineae.\nmodule geometria\n"), "{en}");
    let back = reemit(&with_locale("en", &en), "la");
    assert_eq!(back.trim_end(), LA_BODY.trim_end());
}

#[test]
fn module_doc_is_the_block_directly_above_regio() {
    let analysis = analyze_source(
        &session(),
        "test.fab",
        &with_locale("la", LA_BODY),
        AnalyzeRequest { cli_program: None, contract: crate::driver::ModuleImportContract::default() },
    )
    .unwrap_or_else(|diagnostics| panic!("analyzes: {diagnostics:?}"));
    let region = analysis
        .presentation
        .region
        .as_ref()
        .expect("regio recorded on the analyzed module");
    assert_eq!(analysis.interner.resolve(region.name), "geometria");
    let doc = region
        .doc
        .iter()
        .map(|line| analysis.interner.resolve(*line).trim().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(doc, ["Geometria plana.", "Puncta et lineae."]);
}

#[test]
fn comment_block_separated_by_blank_line_is_not_module_doc() {
    let source = "+++\nlocale = \"la\"\n+++\n# Nota tantum.\n\nregio geometria\n";
    let analysis = analyze_source(
        &session(),
        "test.fab",
        source,
        AnalyzeRequest { cli_program: None, contract: crate::driver::ModuleImportContract::default() },
    )
    .unwrap_or_else(|diagnostics| panic!("analyzes: {diagnostics:?}"));
    let region = analysis
        .presentation
        .region
        .as_ref()
        .expect("regio recorded");
    assert!(region.doc.is_empty(), "{:?}", region.doc);
}

#[test]
fn file_without_regio_has_no_module_doc_anchor() {
    let source = "+++\nlocale = \"la\"\n+++\n# Geometria plana.\nfunctio unum() → numerus {\n    redde 1\n}\n";
    let analysis = analyze_source(
        &session(),
        "test.fab",
        source,
        AnalyzeRequest { cli_program: None, contract: crate::driver::ModuleImportContract::default() },
    )
    .unwrap_or_else(|diagnostics| panic!("analyzes: {diagnostics:?}"));
    assert!(analysis.presentation.region.is_none());
}
