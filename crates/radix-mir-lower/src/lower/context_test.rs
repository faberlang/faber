//! `record_package_import_identities` binding-lookup unit tests.
//!
//! The package-identity map is keyed by the import's effective binding (the
//! alias when one is spelled, else the imported name — see `import_binding`
//! on the program surface and the `PackageImportIdentities` contract), and
//! the resolver installs the file interface under that same binding. The
//! recording pass must therefore resolve both the identity map and the
//! resolver's exports through the effective binding, never through the
//! import's original name (the R03 shape, review need).
//!
//! A named-aliased import is the failing half: its HIR item keeps the
//! original spelling in `name` and the binding in `alias`. A wildcard-alias
//! import is the companion half: the HIR item spells the binding in both
//! slots, so a name-based lookup happens to agree — the test pins that the
//! effective-binding lookup keeps it recording.

use super::*;
use radix_hir::{HirId, HirImportItem, HirModule, LibraryRegistry, TensorEdgePolicyTable};
use radix_runtime_contract::display_tokens::DisplayTokens;
use radix_semantic::analyzed_application::PackageImportIdentities;
use radix_semantic::file_interface::{
    FileExport, FileExportKind, FileInterface, InterfaceCallable, InterfaceTypeSnapshot,
};
use radix_semantic::passes::kernel_requirements::KernelRequirementsReport;
use radix_semantic::{
    AnalysisStamp, AnnotationContractMetadata, FunctionFactTable, FunctionProvenanceTable,
    QualifiedIdentityTable, RadixLaneMetadata, ResolvedUseIndex, Resolver, ShapeObligationTable,
};
use radix_syntax::Visibility;
use std::collections::BTreeMap;

const PRODUCT: &str = "probe";
const VERB: &str = "saluta";
const TARGET_SEGMENTS: [&str; 1] = ["ops"];

fn binding_identity() -> MirExternalFunctionIdentity {
    MirExternalFunctionIdentity::new(
        PRODUCT,
        TARGET_SEGMENTS.iter().map(|s| (*s).to_owned()).collect(),
        VERB,
    )
}

/// A non-generic `textus → textus` verb export, the shape
/// `record_package_import_identities` records a concrete identity for.
fn verb_callable() -> InterfaceCallable {
    InterfaceCallable {
        type_params: Vec::new(),
        tensor_edge_policy_requirements: Vec::new(),
        params: Vec::new(),
        ret: Box::new(InterfaceTypeSnapshot::Primitive(Primitive::Textus)),
        err: None,
        is_async: false,
        is_generator: false,
        device: None,
    }
}

/// One analyzed unit whose only item is one local sibling import. The HIR
/// item and identity-map shapes mirror the real pipeline: the semantic
/// lowering keeps the original spelling in `name` (the alias, when spelled,
/// in `alias`; for wildcards the binding in both slots), and
/// `package_import_identities_for_file` keys `imports` by the effective
/// binding.
struct ImportUnit {
    interner: Interner,
    types: TypeTable,
    resolver: Resolver,
    hir: HirModule,
    identities: PackageImportIdentities,
    import_item_def_id: DefId,
}

/// `hir_name`/`hir_alias` set the HIR item spelling; `binding` is the
/// effective binding the pipeline keys the identity map and the resolver's
/// file interface under. When `decoy` is set, the identity map also carries
/// the original name → a distinct target, so a wrong original-name lookup
/// would resolve a decoy instead of the binding's row.
fn import_unit(hir_name: &str, hir_alias: Option<&str>, binding: &str, decoy: bool) -> ImportUnit {
    let mut interner = Interner::new();
    let mut types = TypeTable::new();
    let mut resolver = Resolver::new();

    // The resolver installs the file interface under the effective binding.
    let interfaces = BTreeMap::from([(binding.to_owned(), FileInterface {
        identity: None,
        exports: BTreeMap::from([(
            VERB.to_owned(),
            FileExport { name: VERB.to_owned(), kind: FileExportKind::Function(verb_callable()) },
        )]),
        unavailable: BTreeMap::new(),
        routes: BTreeMap::new(),
    })]);
    resolver
        .set_file_interfaces_from_snapshots(&interfaces, &mut interner, &mut types)
        .expect("file interface installs");

    let path = interner.intern("./ops");
    let name = interner.intern(hir_name);
    let alias = hir_alias.map(|spelling| interner.intern(spelling));

    let mut imports = BTreeMap::from([(
        binding.to_owned(),
        TARGET_SEGMENTS.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
    )]);
    if decoy {
        // What a lookup through the original name would resolve to.
        imports.insert(hir_name.to_owned(), vec!["decoy".to_owned()]);
    }
    let identities = PackageImportIdentities {
        product: PRODUCT.to_owned(),
        module_segments: vec!["main".to_owned()],
        imports,
        device_interfaces: BTreeMap::new(),
    };

    let import_item_def_id = DefId(50);
    let hir = HirModule {
        items: vec![HirItem {
            id: HirId(1),
            def_id: DefId(51),
            annotations: Vec::new(),
            kind: HirItemKind::Import(HirImport {
                path,
                visibility: Visibility::Privata,
                items: vec![HirImportItem { def_id: import_item_def_id, name, alias }],
                selective: false,
            }),
            span: Span::default(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    ImportUnit { interner, types, resolver, hir, identities, import_item_def_id }
}

/// Collected recording facts for the unit's single aliased import item.
struct Recorded {
    /// `verb → synthesized external-definition id` under the import item.
    exports: FxHashMap<Symbol, DefId>,
    /// External identities recorded for those synthesized ids.
    identities: Vec<MirExternalFunctionIdentity>,
    /// Call targets carried on the item's provider import.
    provider_targets: Option<FxHashMap<Symbol, DefId>>,
}

fn collected(unit: &ImportUnit) -> Recorded {
    let view = AnalyzedModuleApplicationView {
        stamp: AnalysisStamp::from_persisted(1),
        hir: &unit.hir,
        types: &unit.types,
        shape_obligations: &ShapeObligationTable::default(),
        kernel_requirements: &KernelRequirementsReport::default(),
        tensor_edge_policies: &TensorEdgePolicyTable::default(),
        resolver: &unit.resolver,
        annotation_contracts: &AnnotationContractMetadata::default(),
        libraries: &LibraryRegistry::default(),
        function_facts: &FunctionFactTable::default(),
        resolved_uses: &ResolvedUseIndex::default(),
        qualified_identities: &QualifiedIdentityTable::default(),
        package_import_identities: Some(&unit.identities),
        provenance: &FunctionProvenanceTable::default(),
        radix_lanes: &RadixLaneMetadata::default(),
        gpu_builtins: &[],
        cli_program: None,
        display_tokens: &DisplayTokens::latin(),
        interner: &unit.interner,
    };
    let maps = LoweringContextMaps::collect(view);
    let exports = maps
        .package_file_interface_exports
        .get(&unit.import_item_def_id)
        .cloned()
        .unwrap_or_default();
    let identities = exports
        .values()
        .filter_map(|def| maps.validation.external_function_identities.get(def).cloned())
        .collect();
    let provider_targets = maps
        .frozen
        .provider_imports
        .get(&unit.import_item_def_id)
        .and_then(|import| import.file_interface_functions.clone());
    Recorded { exports, identities, provider_targets }
}

/// Named alias: `importa ex "./ops" ops ut o` — the identity map is keyed by
/// the binding `o`, so the recording must resolve through `o`. A lookup
/// through the original name `ops` finds only the decoy row (whose module
/// installs no file interface), so today's `import_item.name` lookup records
/// nothing.
#[test]
fn aliased_named_import_records_identity_under_the_binding() {
    let mut unit = import_unit("ops", Some("o"), "o", true);
    let recorded = collected(&unit);

    let verb = unit.interner.intern(VERB);
    assert!(
        recorded.exports.contains_key(&verb),
        "aliased import must record its verb exports under the binding: {:?}",
        recorded.exports
    );
    assert_eq!(recorded.identities, vec![binding_identity()]);
    assert_eq!(
        recorded.provider_targets.map(|targets| targets.len()),
        Some(1),
        "provider import must carry the binding-resolved call targets"
    );
}

/// Wildcard alias: `importa ex "./ops" * ut o` — the HIR item spells the
/// binding in both slots, so the recording must keep finding the identity
/// through the effective binding (a same-value path today).
#[test]
fn wildcard_alias_import_records_identity_under_the_binding() {
    let mut unit = import_unit("o", Some("o"), "o", false);
    let recorded = collected(&unit);

    let verb = unit.interner.intern(VERB);
    assert!(
        recorded.exports.contains_key(&verb),
        "wildcard-alias import must record its verb exports under the binding: {:?}",
        recorded.exports
    );
    assert_eq!(recorded.identities, vec![binding_identity()]);
    assert_eq!(
        recorded.provider_targets.map(|targets| targets.len()),
        Some(1),
        "provider import must carry the binding-resolved call targets"
    );
}
