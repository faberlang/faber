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

    let hir = fixture.result.hir.as_ref().expect("hir");

    if let Some(entry) = &hir.entry
        && let Some(found) = scan_statements(fixture, &entry.statements, source_name)
    {
        return found;
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
fn optional_union_preserves_authored_ranged_lista_form() {
    let fixture = analyze_ok(concat!(
        "functio show(lista<numerus, magnitudo 0…3> ∪ nihil xs) → vacuum {\n",
        "    nota xs\n",
        "}\n",
    ));
    let form = fixture
        .result
        .presentation
        .authored_types
        .iter()
        .map(|authored| &authored.form)
        .find(|form| matches!(fixture.result.types.get(form.ty), Type::Option(_)))
        .expect("nullable list parameter form");
    let child = single_type_child(form);
    // `0…3` is the bounded list of capacity 3 semantically; only the authored
    // form remembers that the author wrote a range.
    assert!(matches!(
        fixture.result.types.get(child.ty),
        Type::BoundedArray { .. }
    ));
    assert!(
        child.args.iter().any(|arg| matches!(arg, HirTypeFormArg::Range(range)
            if matches!(range.lo, radix_hir::HirRangeBound::Literal(ref value) if value == "0")
                && matches!(range.hi, radix_hir::HirRangeBound::Literal(ref value) if value == "3"))),
        "the ranged-list child should keep its authored endpoints: {:?}",
        child.args
    );
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
            HirTypeFormArg::Size(_) | HirTypeFormArg::Name(_) | HirTypeFormArg::Range(_) => None,
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
fn sermo_authored_head_is_named() {
    // The conversation word is a builtin authored head: `sermo<O, R>` names
    // the frame definition and keeps both authored child forms. Finalize
    // erases only the applied semantic witness to the bare carrier `sermo`,
    // canonicalizing the children instead of dropping them.
    let source = concat!(
        "@ call 'salutatio:greeting'\n",
        "fn greeting() → string {\n",
        "    return \"salve\"\n",
        "}\n",
        "\n",
        "main {\n",
        "    let channel<none, string> g ← call 'salutatio:greeting'\n",
        "    let channel loose ← g\n",
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
    let (form, local_ty) = first_declared_form(&fixture, "g");
    let HirTypeFormHead::Named { def_id, path } = &form.head else {
        panic!("expected named head, got {:?}", form.head);
    };
    assert_eq!(*def_id, sermo_def, "named head names the sermo definition");
    assert!(!path.is_empty(), "named head keeps the authored spelling");
    let [HirTypeFormArg::Type(opener), HirTypeFormArg::Type(reply)] = form.args.as_slice() else {
        panic!(
            "expected the two authored children `none, string`, got {:?}",
            form.args
        );
    };
    for (child, expected, name) in [
        (opener, Primitive::Nihil, "none"),
        (reply, Primitive::Textus, "string"),
    ] {
        assert_eq!(
            child.head,
            HirTypeFormHead::Semantic(TypeFormLevel::LONG),
            "{name} keeps its authored semantic head"
        );
        assert!(child.args.is_empty(), "{name} is a leaf form");
        assert!(
            matches!(fixture.result.types.get(child.ty), Type::Primitive(prim) if *prim == expected),
            "{name} keeps its semantic witness, got {:?}",
            fixture.result.types.get(child.ty)
        );
    }
    assert_eq!(Some(form.ty), local_ty, "witness equals HIR type");
    assert!(
        matches!(fixture.result.types.get(form.ty), Type::Struct(def) if *def == sermo_def),
        "finalized witness is the bare sermo carrier"
    );

    // A separately authored bare `sermo` has no arguments to keep.
    let (loose_form, loose_ty) = first_declared_form(&fixture, "loose");
    assert!(
        matches!(&loose_form.head, HirTypeFormHead::Named { def_id, .. } if *def_id == sermo_def),
        "bare sermo names the same definition, got {:?}",
        loose_form.head
    );
    assert!(
        loose_form.args.is_empty(),
        "a bare sermo authoress no children, got {:?}",
        loose_form.args
    );
    assert_eq!(Some(loose_form.ty), loose_ty, "witness equals HIR type");
}

/// A typed conversation annotation keeps the two type arguments the author
/// wrote as authored child forms, even though the semantic witness is erased to
/// the bare carrier.
#[test]
fn typed_sermo_annotation_keeps_its_authored_children() {
    let fixture = analyze_ok(concat!(
        "@ ad 'numeri:twice'\n",
        "functio twice(numerus n) → numerus {\n",
        "    redde n * 2\n",
        "}\n",
        "\n",
        "incipit {\n",
        "    sit sermo<numerus, numerus> t ← ad 'numeri:twice' (5)\n",
        "    sit sermo loose ← t\n",
        "}\n"
    ));

    let sermo_def = crate::builtins::frame_types::sermo_def_id(&fixture.result.resolver)
        .expect("sermo builtin is registered");
    let (form, local_ty) = first_declared_form(&fixture, "t");
    assert!(
        matches!(&form.head, HirTypeFormHead::Named { def_id, .. } if *def_id == sermo_def),
        "named head names the sermo definition, got {:?}",
        form.head
    );
    assert_eq!(
        form.args.len(),
        2,
        "the typed annotation keeps its two authored children, got {:?}",
        form.args
    );
    for arg in &form.args {
        let HirTypeFormArg::Type(child) = arg else {
            panic!("expected a type child, got {arg:?}");
        };
        assert!(
            matches!(
                fixture.result.types.get(child.ty),
                Type::Primitive(Primitive::Numerus)
            ),
            "child witnesses the authored `numerus`"
        );
    }
    assert_eq!(Some(form.ty), local_ty, "witness equals HIR type");
    assert!(
        matches!(fixture.result.types.get(form.ty), Type::Struct(def) if *def == sermo_def),
        "finalized witness is the bare sermo carrier"
    );

    let (loose_form, _) = first_declared_form(&fixture, "loose");
    assert!(
        loose_form.args.is_empty(),
        "the separately authored bare sermo stays bare, got {:?}",
        loose_form.args
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

/// An imported type keeps its `Named` head: a selectively imported name and a
/// qualified `module.Type` path both record the imported definition beside
/// the spelled path, exactly as a locally declared type does.
#[test]
fn imported_authored_type_carries_named_heads() {
    use crate::file_interface::{
        FileExport, FileExportKind, FileInterface, InterfaceStructExport, InterfaceStructField,
        InterfaceTypeSnapshot,
    };
    use std::collections::BTreeMap;

    let interface = || {
        let mut interface = FileInterface::new();
        interface.insert(FileExport {
            name: "Circle".to_owned(),
            kind: FileExportKind::Struct(InterfaceStructExport {
                is_json: false,
                implements: Vec::new(),
                name: "Circle".to_owned(),
                params: Vec::new(),
                fields: vec![InterfaceStructField {
                    name: "radius".to_owned(),
                    ty: InterfaceTypeSnapshot::Primitive(Primitive::Numerus),
                    optional: false,
                    required: true,
                    default: None,
                    modifier: None,
                    visibility: radix_syntax::MemberVisibility::Publica,
                }],
                methods: Vec::new(),
                annotation_contract: None,
            }),
        });
        interface
    };
    let public_circle = |key: &str| {
        (
            key.to_owned(),
            BTreeMap::from([(
                "Circle".to_owned(),
                crate::passes::visibility_annotation::VisibilityTier::Publica,
            )]),
        )
    };
    let fixture = crate::test_support::analyze_source_with_import_contract(
        concat!(
            "importa ex \"./geo\" fixum Circle\n",
            "importa ex \"./geo\" geo\n",
            "functio probe(Circle picked, geo.Circle qualified) {\n",
            "    sit Circle first ← picked\n",
            "    sit geo.Circle second ← qualified\n",
            "}\n"
        ),
        BTreeMap::from([public_circle("geo"), public_circle("./geo")]),
        BTreeMap::from([
            ("geo".to_owned(), interface()),
            ("./geo".to_owned(), interface()),
        ]),
    );
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);

    for (local, expected_path) in [("first", vec!["Circle"]), ("second", vec!["geo", "Circle"])] {
        let (form, local_ty) = first_declared_form(&fixture, local);
        let HirTypeFormHead::Named { def_id, path } = &form.head else {
            panic!("{local}: expected a Named head, got {:?}", form.head);
        };
        let spelled: Vec<String> = path
            .iter()
            .map(|symbol| fixture.interner.resolve(*symbol).to_owned())
            .collect();
        assert_eq!(spelled, expected_path, "{local}: spelled path");
        assert!(
            matches!(
                fixture.result.types.get(form.ty),
                Type::Struct(witness_def) if witness_def == def_id
            ),
            "{local}: head definition names the witness struct"
        );
        assert_eq!(Some(form.ty), local_ty, "{local}: witness equals HIR type");
    }
}
