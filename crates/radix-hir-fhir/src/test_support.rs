//! Shared `#[cfg(test)]` stub artifact for codec and validation tests.
//!
//! Mirrors the minimal artifact used by the `radix` loader tests so the
//! format crate can prove encode/decode and fail-closed behavior standalone.

#![cfg(test)]

use crate::artifact::{
    DisplayTokensWire, FunctionFactTableWire, FunctionProvenanceTableWire, HirArtifact,
    ResolverSnapshot, SCHEMA_VERSION, SourceIdentity,
};
use radix_hir::{
    HirFunction, HirId, HirItem, HirItemKind, HirModule, HirModuleEnd, HirPresentation,
    HirSourceAnchorId, LibraryRegistry,
};
use radix_lexer::{Span, Symbol};
use radix_types::{DefId, TypeTable};
use std::collections::BTreeMap;

/// A minimal schema-v2 artifact that passes referential-integrity validation.
pub(crate) fn stub_artifact() -> HirArtifact {
    let item = HirItem {
        annotations: Vec::new(),
        id: HirId(0),
        def_id: DefId(0),
        kind: HirItemKind::Function(Box::new(HirFunction {
            name: Symbol(0),
            type_params: Vec::new(),
            params: Vec::new(),
            cli_args: None,
            ret_ty: None,
            err_ty: None,
            body: None,
            posture: radix_syntax::CallablePosture::SyncFinite,
            is_async: false,
            is_generator: false,
            is_kernel: false,
            nucleum_fragment: false,
            is_vertex: false,
            is_fragment: false,
            test: None,
        })),
        span: Span::default(),
    };
    let hir = HirModule {
        items: vec![item],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let presentation = HirPresentation {
        owners: Vec::new(),
        attachments: Vec::new(),
        block_ends: Vec::new(),
        program_end: HirModuleEnd {
            id: HirSourceAnchorId(0),
            span: Span::default(),
        },
        entry_start_anchor: None,
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        discard_binding_stmts: Vec::new(),
        destructure_bindings: Vec::new(),
        await_assignment_bindings: Vec::new(),
        compile_time_locals: Vec::new(),
        ordo_enums: Vec::new(),
        variant_paths: Vec::new(),
        authored_types: Vec::new(),
        tuple_label_accesses: Vec::new(),
        insere_exprs: Vec::new(),
        function_modifier_bindings: Vec::new(),
        region: None,
    };
    let types = TypeTable::new();
    HirArtifact {
        schema_version: SCHEMA_VERSION,
        source_identity: SourceIdentity {
            content_hash: "abc123".to_string(),
            relative_path: "test.faber".to_string(),
        },
        hir,
        presentation,
        cli_program: None,
        types: types.snapshot(),
        resolved_generic_calls: Vec::new(),
        radix_lanes: Vec::new(),
        interner: vec!["main".to_string(), "test".to_string()],
        resolver: ResolverSnapshot {
            namespace_exports: Vec::new(),
            file_interfaces: Vec::new(),
            used_namespaces: Vec::new(),
            imported_nominal_types: Vec::new(),
            ambiguous_imported_nominal_types: Vec::new(),
            conversion_rows: Vec::new(),
        },
        libraries: LibraryRegistry::default(),
        function_facts: FunctionFactTableWire {
            entries: Vec::new(),
            entry: None,
        },
        resolved_uses: Vec::new(),
        analysis_stamp: 42,
        gpu_builtins: Vec::new(),
        capability_provenance: FunctionProvenanceTableWire::default(),
        provider_units: BTreeMap::new(),
        shape_obligations: crate::artifact::ShapeObligationTableWire::default(),
        kernel_requirements: crate::artifact::KernelRequirementsWire::default(),
        tensor_edge_policies: crate::artifact::TensorEdgePolicyTableWire::default(),
        display_tokens: DisplayTokensWire {
            true_: "true".to_string(),
            false_: "false".to_string(),
            none: "none".to_string(),
            tuple: "tuple".to_string(),
        },
    }
}

/// Artifact whose HIR carries one selective import (`importa ex "…" fixum …`).
///
/// The `HirImport.selective` marker is additive with `#[serde(default)]`
/// (EOT-2M), so the wire carries it without a schema ratchet: v4 artifacts
/// written before the field decode with `selective == false` (legacy
/// namespace shape) and v4 artifacts written after carry it explicitly.
pub(crate) fn selective_import_artifact() -> HirArtifact {
    use radix_hir::HirImportItem;
    use radix_types::USER_DEF_ID_BASE;

    let mut artifact = stub_artifact();
    artifact.interner = vec![
        "main".to_string(),
        "test".to_string(),
        "norma:consolum".to_string(),
        "dic".to_string(),
        "output".to_string(),
        "funde".to_string(),
    ];
    let import = radix_hir::HirImport {
        path: Symbol(2),
        visibility: radix_syntax::Visibility::Privata,
        items: vec![
            HirImportItem {
                def_id: DefId(USER_DEF_ID_BASE + 40),
                name: Symbol(3),
                alias: Some(Symbol(4)),
            },
            HirImportItem {
                def_id: DefId(USER_DEF_ID_BASE + 41),
                name: Symbol(5),
                alias: None,
            },
        ],
        selective: true,
    };
    artifact.hir.items.push(radix_hir::HirItem {
        annotations: Vec::new(),
        id: HirId(1),
        def_id: DefId(USER_DEF_ID_BASE + 42),
        kind: radix_hir::HirItemKind::Import(import),
        span: Span::default(),
    });
    artifact
}

/// Artifact whose HIR carries one `⇇` copy-into statement in the function body.
pub(crate) fn transfer_artifact() -> HirArtifact {
    use radix_hir::{HirBlock, HirStatement, HirStatementKind};

    let mut artifact = stub_artifact();
    artifact.interner = vec!["main".to_string(), "test".to_string(), "funde".to_string()];
    let transfer = radix_hir::HirExpression {
        id: HirId(7),
        ty: None,
        kind: radix_hir::HirExpressionKind::CopyInto {
            target: Box::new(radix_hir::HirExpression {
                id: HirId(8),
                ty: None,
                kind: radix_hir::HirExpressionKind::Path(DefId(0)),
                span: Span::default(),
            }),
            value: Box::new(radix_hir::HirExpression {
                id: HirId(9),
                ty: None,
                kind: radix_hir::HirExpressionKind::Literal(radix_hir::HirLiteral::String(Symbol(
                    2,
                ))),
                span: Span::default(),
            }),
        },
        span: Span::default(),
    };
    let radix_hir::HirItemKind::Function(function) = &mut artifact.hir.items[0].kind else {
        panic!("stub artifact must carry a function item");
    };
    function.body = Some(HirBlock {
        statements: vec![HirStatement {
            id: HirId(11),
            kind: HirStatementKind::Expr(transfer),
            span: Span::default(),
        }],
        expr: None,
        span: Span::default(),
        ..Default::default()
    });
    artifact
}
