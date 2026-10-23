//! Authored type-form capture tests (authored-type-forms ATF-3).
//!
//! Contract: declaration-owned type positions record a form tree in the
//! presentation sidecar whose root witness equals the HIR type at the same
//! site, whose long/short levels match the authored spelling, whose alias
//! uses carry the authored name beside the semantic `DefId`, and whose
//! witnesses are canonicalized after inference.

use crate::test_support::{AnalyzedFixture, analyze_ok};
use radix_hir::{
    HirItemKind, HirOwnerRef, HirStatement, HirStatementKind, HirTypeForm, HirTypeFormArg,
    HirTypeFormHead, HirTypeSlot,
};
use radix_syntax::TypeFormLevel;
use radix_types::{NumericWidth, Primitive, Type};

/// (form, HIR type) of the first local binding named `source_name`, scanning
/// the entry block and every function body.
fn first_declared_form<'a>(
    fixture: &'a AnalyzedFixture,
    source_name: &str,
) -> (&'a HirTypeForm, Option<radix_types::TypeId>) {
    let hir = fixture.result.hir.as_ref().expect("hir");

    fn scan_statements<'a>(
        fixture: &'a AnalyzedFixture,
        statements: &'a [HirStatement],
        source_name: &str,
    ) -> Option<(&'a HirTypeForm, Option<radix_types::TypeId>)> {
        for stmt in statements {
            let HirStatementKind::Local(local) = &stmt.kind else {
                continue;
            };
            if fixture.interner.resolve(local.name) != source_name {
                continue;
            }
            let site = radix_hir::HirTypeSite {
                owner: HirOwnerRef::Def(local.def_id),
                slot: HirTypeSlot::Declared,
            };
            let form = fixture
                .result
                .presentation
                .authored_type_at(&site)
                .unwrap_or_else(|| panic!("no authored form recorded for local {source_name}"));
            return Some((form, local.ty));
        }
        None
    }

    if let Some(entry) = &hir.entry {
        if let Some(found) = scan_statements(fixture, &entry.statements, source_name) {
            return found;
        }
    }
    for item in &hir.items {
        let HirItemKind::Function(function) = &item.kind else {
            continue;
        };
        let Some(body) = &function.body else {
            continue;
        };
        if let Some(found) = scan_statements(fixture, &body.statements, source_name) {
            return found;
        }
    }
    panic!("local {source_name} not found in entry block or function bodies");
}

fn single_type_child(form: &HirTypeForm) -> &HirTypeForm {
    assert_eq!(
        form.args.len(),
        1,
        "expected one child, got {:?}",
        form.args
    );
    let HirTypeFormArg::Type(child) = &form.args[0] else {
        panic!("expected a type child, got {:?}", form.args);
    };
    child
}

#[test]
fn declared_forms_match_authored_spelling_and_canonical_witness() {
    let fixture = analyze_ok(concat!(
        "functio probe() {\n",
        "    sit lista<u32> long_form ← vacua\n",
        "    sit lu32 fused ← vacua\n",
        "}\n"
    ));

    // Long root stays LONG; its bare-width element is SHORT over the same
    // sized-numeric witness the fused spelling records.
    let (long_form, long_ty) = first_declared_form(&fixture, "long_form");
    assert_eq!(
        long_form.head,
        HirTypeFormHead::Semantic(TypeFormLevel::LONG)
    );
    let long_child = single_type_child(long_form);
    assert_eq!(
        long_child.head,
        HirTypeFormHead::Semantic(TypeFormLevel::SHORT)
    );
    assert!(matches!(
        fixture.result.types.get(long_child.ty),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::U32)
    ));
    assert_eq!(Some(long_form.ty), long_ty, "witness equals HIR type");

    // Fused lista sugar: root SHORT, element SHORT, identical witnesses.
    let (fused_form, fused_ty) = first_declared_form(&fixture, "fused");
    assert_eq!(
        fused_form.head,
        HirTypeFormHead::Semantic(TypeFormLevel::SHORT)
    );
    let fused_child = single_type_child(fused_form);
    assert_eq!(
        fused_child.head,
        HirTypeFormHead::Semantic(TypeFormLevel::SHORT)
    );
    assert_eq!(long_child.ty, fused_child.ty, "same semantic child type");
    assert_eq!(long_ty, fused_ty, "same semantic root type");
}

#[test]
fn tensor_forms_record_shape_children() {
    let fixture = analyze_ok(concat!(
        "functio probe() {\n",
        "    sit tensor<f32, [2, 2]> board ← vacua\n",
        "}\n"
    ));
    let (form, _) = first_declared_form(&fixture, "board");
    assert_eq!(form.head, HirTypeFormHead::Semantic(TypeFormLevel::LONG));
    assert_eq!(form.args.len(), 2, "element + figura shape");
    assert!(matches!(&form.args[1], HirTypeFormArg::Size(_)));
}

#[test]
fn hole_annotations_record_infer_heads() {
    let fixture = analyze_ok("functio probe() {\n    sit _ holed ← 1\n}");
    let (form, _) = first_declared_form(&fixture, "holed");
    assert_eq!(form.head, HirTypeFormHead::Infer);
    assert!(form.args.is_empty(), "hole heads carry no children");
}

#[test]
fn alias_uses_carry_named_heads_and_alias_bodies_stay_semantic() {
    let fixture = analyze_ok(concat!(
        "typus Word = modulus<u32>\n",
        "functio probe() {\n",
        "    sit Word w ← 1\n",
        "}\n"
    ));

    // Alias body: Semantic LONG over the sized-numeric witness.
    let body = fixture
        .result
        .presentation
        .authored_types
        .iter()
        .find(|authored| authored.site.slot == HirTypeSlot::AliasBody)
        .expect("alias body recorded");
    assert_eq!(
        body.form.head,
        HirTypeFormHead::Semantic(TypeFormLevel::LONG)
    );
    assert!(matches!(
        fixture.result.types.get(body.form.ty),
        Type::ModularWord(NumericWidth::U32)
    ));

    // Alias use: Named head carrying the authored spelling beside the identity.
    let (use_form, local_ty) = first_declared_form(&fixture, "w");
    let HirTypeFormHead::Named { path, .. } = &use_form.head else {
        panic!("expected named head, got {:?}", use_form.head);
    };
    let spelled: Vec<String> = path
        .iter()
        .map(|symbol| fixture.interner.resolve(*symbol).to_owned())
        .collect();
    assert_eq!(spelled, vec!["Word".to_string()]);
    assert_eq!(Some(use_form.ty), local_ty, "witness equals HIR type");
}

/// An authored `iuncta` element label rides the labeled child form, and the
/// by-label read records its authored label against the rewritten index node.
#[test]
fn iuncta_labels_ride_the_form_and_by_label_reads_record_their_label() {
    let fixture = analyze_ok(concat!(
        "functio probe() {\n",
        "    sit iuncta<gx: numerus, gw: numerus> par ← iuncta<numerus, numerus> [1, 2]\n",
        "    sit numerus w ← par.gw\n",
        "}\n"
    ));

    let (form, _) = first_declared_form(&fixture, "par");
    let labels: Vec<Option<String>> = form
        .args
        .iter()
        .map(|arg| match arg {
            HirTypeFormArg::Type(child) => child
                .label
                .map(|symbol| fixture.interner.resolve(symbol).to_owned()),
            HirTypeFormArg::Size(_) | HirTypeFormArg::Name(_) => None,
        })
        .collect();
    assert_eq!(labels, vec![Some("gx".to_owned()), Some("gw".to_owned())]);

    let read_labels: Vec<String> = fixture
        .result
        .presentation
        .tuple_label_accesses
        .iter()
        .map(|access| fixture.interner.resolve(access.label).to_owned())
        .collect();
    assert_eq!(read_labels, vec!["gw".to_owned()]);
}

#[test]
fn sermo_authored_head_keeps_only_authored_children() {
    // `sermo<O, R>` names the builtin frame definition and finalizes to the
    // bare carrier. Its authored form keeps both type arguments; a bare
    // `sermo` annotation has no authored children.
    let source = concat!(
        "@ call 'salutatio:greeting'\n",
        "fn greeting() → string {\n",
        "    return \"salve\"\n",
        "}\n",
        "\n",
        "main {\n",
        "    let channel<none, string> g ← call 'salutatio:greeting'\n",
        "    let channel bare ← g\n",
        "}\n",
    );
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("installed en pack should load"),
    );
    let fixture = crate::test_support::analyze_source_with_config(source, config);
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);

    let sermo_def = crate::builtins::frame_types::sermo_def_id(&fixture.result.resolver)
        .expect("sermo builtin is registered");
    let (typed_form, typed_ty) = first_declared_form(&fixture, "g");
    let HirTypeFormHead::Named { def_id, path } = &typed_form.head else {
        panic!("expected named head, got {:?}", typed_form.head);
    };
    assert_eq!(*def_id, sermo_def, "named head names the sermo definition");
    assert!(!path.is_empty(), "named head keeps the authored spelling");
    assert_eq!(
        typed_form.args.len(),
        2,
        "typed sermo retains both authored arguments"
    );
    assert!(
        typed_form
            .args
            .iter()
            .all(|arg| matches!(arg, HirTypeFormArg::Type(_)))
    );
    assert_eq!(Some(typed_form.ty), typed_ty, "witness equals HIR type");
    assert!(
        matches!(fixture.result.types.get(typed_form.ty), Type::Struct(def) if *def == sermo_def),
        "finalized witness is the bare sermo carrier"
    );

    let (bare_form, bare_ty) = first_declared_form(&fixture, "bare");
    let HirTypeFormHead::Named { def_id, path } = &bare_form.head else {
        panic!("expected named head, got {:?}", bare_form.head);
    };
    assert_eq!(
        *def_id, sermo_def,
        "bare named head names the sermo definition"
    );
    assert!(
        !path.is_empty(),
        "bare named head keeps the authored spelling"
    );
    assert!(
        bare_form.args.is_empty(),
        "bare sermo has no authored arguments"
    );
    assert_eq!(Some(bare_form.ty), bare_ty, "witness equals HIR type");
    assert!(
        matches!(fixture.result.types.get(bare_form.ty), Type::Struct(def) if *def == sermo_def),
        "bare annotation also resolves to the sermo carrier"
    );
}

#[test]
fn witness_canonicalization_survives_inference_solving() {
    let fixture = analyze_ok("functio probe() {\n    sit u32 bare ← 1\n}");
    let (form, local_ty) = first_declared_form(&fixture, "bare");
    assert_eq!(form.head, HirTypeFormHead::Semantic(TypeFormLevel::SHORT));
    assert_eq!(Some(form.ty), local_ty);
    assert!(matches!(
        fixture.result.types.get(form.ty),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::U32)
    ));
}
