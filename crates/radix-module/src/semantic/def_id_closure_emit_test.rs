//! Pipeline remainder: rust emit table must see locale-pack aliases.

#[cfg(feature = "hir-rust")]
#[test]
fn rust_locale_pack_type_alias_def_reaches_emit_table() {
    use crate::codegen::Target;
    use crate::codegen::rust::generate_def_table;
    use crate::semantic::{
        DEF_ID_CLOSURE_CODE, PassConfig, analyze, analyze_resolved_uses, check_def_id_closure, emit_def_table_name,
        resolved_reference_defs,
    };
    use handroll::{FxHashMap, FxHashSet};

    let pack_path = crate::locale::workspace_locale_pack_path("th-TH");
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
    let aliases = result
        .resolver
        .locale_pack_type_alias_names(&parsed.interner);
    assert_eq!(aliases.get(&def_id), Some(&surface));

    let imported = FxHashMap::default();
    let emit = generate_def_table(&hir, &parsed.interner, std::iter::empty(), &imported, aliases.keys().copied());
    assert!(
        emit.contains(&def_id),
        "locale-pack TypeAlias DefId {def_id:?} must reach generate_def_table"
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
