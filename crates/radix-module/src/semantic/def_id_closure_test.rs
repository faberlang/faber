use super::*;
use crate::hir::DefId;

#[test]
fn gaps_empty_when_sets_agree() {
    let referenced = [DefId(1), DefId(7)].into_iter().collect();
    let emit = [DefId(1), DefId(7), DefId(9)].into_iter().collect();
    assert!(def_id_closure_gaps(&referenced, &emit).is_empty());
}

#[test]
fn gaps_name_missing_defs_in_id_order() {
    let referenced = [DefId(12), DefId(3), DefId(8)].into_iter().collect();
    let emit = [DefId(8)].into_iter().collect();
    assert_eq!(def_id_closure_gaps(&referenced, &emit), vec![DefId(3), DefId(12)]);
}

#[test]
fn diagnostic_names_the_missing_def() {
    let diag = missing_emit_def_diagnostic(DefId(4127), Some("Cognita"), "render.fab");
    assert_eq!(diag.code, Some(DEF_ID_CLOSURE_CODE));
    assert_eq!(diag.issue(), Some(DEF_ID_CLOSURE_ISSUE));
    assert_eq!(diag.file, "render.fab");
    assert!(diag
        .args
        .iter()
        .any(|arg| arg.name == "def_id" && arg.value == "4127"));
    assert!(diag
        .args
        .iter()
        .any(|arg| arg.name == "def_name" && arg.value == "Cognita"));
}

#[test]
fn check_reports_only_the_missing_side() {
    let referenced = [DefId(1), DefId(2)].into_iter().collect();
    let emit = [DefId(1)].into_iter().collect();
    let diags = check_def_id_closure(
        &referenced,
        &emit,
        |def_id| (def_id.0 == 2).then(|| "Known".to_owned()),
        "main.fab",
    );
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, Some(DEF_ID_CLOSURE_CODE));
    assert_eq!(diags[0].issue(), Some(DEF_ID_CLOSURE_ISSUE));
    assert!(diags[0]
        .args
        .iter()
        .any(|arg| arg.name == "def_name" && arg.value == "Known"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_locale_pack_type_alias_def_reaches_emit_table() {
    use crate::codegen::rust::emit_def_table;
    use crate::codegen::Target;
    use crate::semantic::{analyze, analyze_resolved_uses, PassConfig};
    use rustc_hash::FxHashMap;

    let pack_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/locale/th-TH/pack.toml");
    let pack = crate::locale::LocalePack::from_toml_path(&pack_path).expect("installed th-TH pack should load");
    let source = "ฟังก์ชัน ก(จำนวน n) → จำนวน {\n  คืน n\n}\n";
    let lex = crate::lexer::lex_with_locale_pack(source, &pack);
    let mut parsed = crate::parser::parse(lex);
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let program = parsed.program.expect("program");
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(pack);
    let result = analyze(&program, &config, &mut parsed.interner);
    assert!(
        result.diagnostics.iter().all(|diag| !diag.is_error()),
        "analysis errors: {:?}",
        result.diagnostics
    );
    let hir = result.hir.expect("hir");
    let soan = parsed.interner.intern("จำนวน");
    let def_id = result
        .resolver
        .lookup_type(soan)
        .expect("th-TH numerus surface จำนวน must register a TypeAlias DefId");
    let surface = parsed.interner.resolve(soan).to_owned();
    let aliases = result.resolver.locale_pack_type_alias_names(&parsed.interner);
    assert_eq!(aliases.get(&def_id), Some(&surface));

    let imported = FxHashMap::default();
    let emit = emit_def_table(
        &hir,
        &parsed.interner,
        std::iter::empty(),
        &imported,
        aliases.keys().copied(),
    );
    assert!(
        emit.contains(&def_id),
        "locale-pack TypeAlias DefId {def_id:?} must reach emit_def_table"
    );

    let referenced = [def_id].into_iter().collect();
    let missing = check_def_id_closure(
        &referenced,
        &FxHashSet::default(),
        |id| emit_def_table_name(id, &aliases, &imported).map(str::to_owned),
        "main.fab",
    );
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].code, Some(DEF_ID_CLOSURE_CODE));
    assert!(
        missing[0]
            .args
            .iter()
            .any(|arg| arg.name == "def_name" && arg.value == surface),
        "DEFID001 must name the pack alias, got {:?}",
        missing[0].args
    );

    let uses = analyze_resolved_uses(&hir, &result.types, &crate::semantic::QualifiedIdentityTable::new());
    let live = resolved_reference_defs(&uses);
    let admitted = check_def_id_closure(
        &live,
        &emit,
        |id| emit_def_table_name(id, &aliases, &imported).map(str::to_owned),
        "main.fab",
    );
    assert!(
        admitted.is_empty(),
        "admitted pack aliases must not trip DEFID001: {admitted:?}"
    );
}
