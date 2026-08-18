//! Shared `#[cfg(test)]` stub artifact for codec and validation tests.
//!
//! Mirrors the minimal artifact used by the `radix` loader tests so the
//! format crate can prove encode/decode and fail-closed behavior standalone.

#![cfg(test)]

use crate::artifact::{
    FunctionFactTableWire, HirArtifact, ResolverSnapshot, SourceIdentity, SCHEMA_VERSION,
};
use radix_hir::{
    HirFunction, HirId, HirItem, HirItemKind, HirModule, HirModuleEnd, HirPresentation,
    HirSourceAnchorId, LibraryRegistry,
};
use radix_lexer::{Span, Symbol};
use radix_types::{DefId, TypeTable};

/// A minimal schema-v2 artifact that passes referential-integrity validation.
pub(crate) fn stub_artifact() -> HirArtifact {
    let item = HirItem {
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
            annotations: Vec::new(),
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
        interner: vec!["main".to_string(), "test".to_string()],
        resolver: ResolverSnapshot {
            namespace_exports: Vec::new(),
            file_interfaces: Vec::new(),
            used_namespaces: Vec::new(),
            imported_nominal_types: Vec::new(),
            ambiguous_imported_nominal_types: Vec::new(),
        },
        libraries: LibraryRegistry::default(),
        function_facts: FunctionFactTableWire {
            entries: Vec::new(),
            entry: None,
        },
        resolved_uses: Vec::new(),
        analysis_stamp: 42,
        gpu_builtins: Vec::new(),
    }
}
