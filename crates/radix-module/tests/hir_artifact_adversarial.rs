#![cfg(feature = "hir-fhir")]

//! Adversarial fixture tests for HIR artifact loading (UNIT-U4.3).
//!
//! Integration-test counterparts to the U2.4 unit-level adversarial tests
//! in `crates/radix/src/hir/artifact.rs`. Only fixture classes NOT already
//! covered by U2.4 are included — see U4.3-ADVERSARIAL-PREP.md test matrix.
//!
//! Every test asserts the exact `HirArtifactError` variant with field
//! values (never `is_err()`). All tests run in CI, none are `#[ignore]`
//! (CSO E5).
//!
//! FREEZES CITED
//! =============
//! - **F7**: Fail-closed structured errors; never panic.
//! - **CSO E5**: Adversarial fixtures required as gate; must run in CI.

use radix_hir::{HirFunction, HirId, HirItem, HirItemKind, HirModule, LibraryRegistry};
use radix_hir_fhir::HirArtifactError;
use radix_hir_fhir::{
    FunctionFactTableWire, FunctionProvenanceTableWire, ResolverSnapshot, SCHEMA_VERSION, SourceIdentity,
};
use radix_lexer::Symbol;
use radix_module::hir::artifact::load_fhir;
use radix_types::{DefId, TypeId, TypeTable};

// ── Shared helpers (duplicated from crates/radix/src/hir/artifact.rs tests) ──

/// Construct a minimal valid HirArtifact for adversarial test setup.
///
/// This is a direct copy of `minimal_test_artifact()` from the unit test
/// module. Keeping it inline avoids importing private helpers across the
/// unit-test / integration-test boundary.
fn minimal_test_artifact() -> radix_hir_fhir::HirArtifact {
    let types = TypeTable::new();
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
        span: radix_lexer::Span::default(),
    };
    let hir = HirModule {
        items: vec![item],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };

    radix_hir_fhir::HirArtifact {
        schema_version: SCHEMA_VERSION,
        source_identity: SourceIdentity { content_hash: "abc123".to_string(), relative_path: "test.faber".to_string() },
        hir,
        presentation: radix_hir::HirPresentation {
            owners: Vec::new(),
            attachments: Vec::new(),
            block_ends: Vec::new(),
            program_end: radix_hir::HirModuleEnd {
                id: radix_hir::HirSourceAnchorId(0),
                span: radix_lexer::Span::default(),
            },
            entry_start_anchor: None,
            entry_explicit: false,
            guard_si_ids: Vec::new(),
            elige_expr_ids: Vec::new(),
            glyph_ids: Vec::new(),
            numeric_twin_ids: Vec::new(),
            empty_dense_ids: Vec::new(),
            inferred_constructor_ids: Vec::new(),
            axis_sugar: Vec::new(),
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
        },
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
        function_facts: FunctionFactTableWire { entries: Vec::new(), entry: None },
        resolved_uses: Vec::new(),
        analysis_stamp: 1,
        gpu_builtins: Vec::new(),
        capability_provenance: FunctionProvenanceTableWire::default(),
        provider_units: std::collections::BTreeMap::new(),
        shape_obligations: radix_hir_fhir::ShapeObligationTableWire::default(),
        kernel_requirements: Default::default(),
        tensor_edge_policies: Default::default(),
        display_tokens: radix_hir_fhir::DisplayTokensWire {
            true_: "true".to_string(),
            false_: "false".to_string(),
            none: "none".to_string(),
            tuple: "tuple".to_string(),
        },
    }
}

/// Serialize a HirArtifact to postcard bytes for adversarial tests.
fn serialize_artifact(artifact: &radix_hir_fhir::HirArtifact) -> Vec<u8> {
    postcard::to_allocvec(artifact).expect("serialize test artifact")
}

// ── Adversarial tests (missing classes vs U2.4) ────────────────────────────

/// Malformed: garbage bytes that cannot decode as any valid postcard payload.
///
/// Feeds a long run of 0xFF bytes after a valid u32 prefix to trigger a
/// postcard deserialization error that is NOT `DeserializeUnexpectedEnd`
/// (which would map to `Truncated`). The resulting `Malformed(_)` carries
/// the inner `postcard::Error`.
#[test]
fn hir_artifact_adversarial_malformed() {
    // sequence: the current schema version (valid varint), then unending
    // 0xFF varint that causes DeserializeBadVarint → Malformed.
    let mut bytes = vec![SCHEMA_VERSION as u8];
    bytes.extend(std::iter::repeat_n(0xFFu8, 256));
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::Malformed(_)) => {} // expected: any Malformed variant
        Err(other) => panic!("expected Malformed, got {other:?}"),
        Ok(_) => panic!("expected Malformed, got Ok"),
    }
}

/// Schema-v1 payload: version prefix 1 is rejected before field layout.
///
/// A schema-v1 artifact carries no presentation field; the loader reads the
/// version prefix first and must reject it with `UnsupportedSchemaVersion`
/// rather than attempting (and failing) to interpret the v1 field layout.
#[test]
fn hir_artifact_adversarial_schema_v1_rejected() {
    // 0x01 = schema_version=1 (v1 artifact prefix), then garbage payload.
    // The version prefix is read before full payload interpretation.
    let mut bytes = vec![0x01u8];
    bytes.extend(std::iter::repeat_n(0xAAu8, 64));
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::UnsupportedSchemaVersion { found, expected }) => {
            assert_eq!(found, 1);
            assert_eq!(expected, SCHEMA_VERSION);
        }
        Err(other) => panic!("expected UnsupportedSchemaVersion(found=1), got {other:?}"),
        Ok(_) => panic!("expected UnsupportedSchemaVersion(found=1), got Ok"),
    }
}

/// Dangling TypeId: a `TypeId` reference exceeds the type arena size.
///
/// Sets the function's `err_ty` to `Some(TypeId(100))`, well past the
/// primitive-only type table (~15 entries). Validated during the type
/// arena walk (gap 4.2) in `validate_referential_integrity`.
#[test]
fn hir_artifact_adversarial_dangling_type_id() {
    let mut artifact = minimal_test_artifact();
    if let HirItemKind::Function(ref mut f) = artifact.hir.items[0].kind {
        f.err_ty = Some(TypeId(100));
    }
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::DanglingIndex { kind, index, arena_size }) => {
            assert_eq!(kind, "TypeId");
            assert_eq!(index, 100);
            assert!(arena_size > 0, "type arena should not be empty");
        }
        Err(other) => panic!("expected DanglingIndex(TypeId, 100), got {other:?}"),
        Ok(_) => panic!("expected DanglingIndex(TypeId, 100), got Ok"),
    }
}

/// Dangling Symbol: a `Symbol` index exceeds the interner length.
///
/// The minimal artifact has an interner of length 2. Setting the
/// function name to `Symbol(5)` exceeds the interner and triggers
/// `DanglingIndex { kind: "Symbol" }`.
#[test]
fn hir_artifact_adversarial_dangling_symbol() {
    let mut artifact = minimal_test_artifact();
    if let HirItemKind::Function(ref mut f) = artifact.hir.items[0].kind {
        f.name = Symbol(5);
    }
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::DanglingIndex { kind, index, arena_size }) => {
            assert_eq!(kind, "Symbol");
            assert_eq!(index, 5);
            assert_eq!(arena_size, 2);
        }
        Err(other) => panic!("expected DanglingIndex(Symbol, 5), got {other:?}"),
        Ok(_) => panic!("expected DanglingIndex(Symbol, 5), got Ok"),
    }
}

/// Version mismatch at the bottom boundary: schema_version = 0.
///
/// U2.4 covers `found: 99`; this test adds the boundary case where
/// schema_version is 0 (plausible sentinel / unset value).
#[test]
fn hir_artifact_adversarial_version_mismatch_zero() {
    let mut artifact = minimal_test_artifact();
    artifact.schema_version = 0;
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::UnsupportedSchemaVersion { found, expected }) => {
            assert_eq!(found, 0);
            assert_eq!(expected, SCHEMA_VERSION);
        }
        Err(other) => panic!("expected UnsupportedSchemaVersion(found=0), got {other:?}"),
        Ok(_) => panic!("expected UnsupportedSchemaVersion(found=0), got Ok"),
    }
}

/// Schema-v1 rejection at the adjacent boundary: schema_version = 1.
///
/// The schema-v2 clean break (F3) rejects the immediately-previous version
/// with a dedicated `UnsupportedSchemaVersion` error; there is no v1
/// compatibility decoder.
#[test]
fn hir_artifact_adversarial_version_mismatch_one() {
    let mut artifact = minimal_test_artifact();
    artifact.schema_version = 1;
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::UnsupportedSchemaVersion { found, expected }) => {
            assert_eq!(found, 1);
            assert_eq!(expected, SCHEMA_VERSION);
        }
        Err(other) => panic!("expected UnsupportedSchemaVersion(found=1), got {other:?}"),
        Ok(_) => panic!("expected UnsupportedSchemaVersion(found=1), got Ok"),
    }
}

// ── Schema-v2 presentation sidecar adversarial classes (U-4) ─────────────

/// Attachment whose anchor names no registered owner → DanglingIndex.
#[test]
fn hir_artifact_adversarial_dangling_attachment_anchor() {
    let mut artifact = minimal_test_artifact();
    artifact.presentation = radix_hir::HirPresentation {
        owners: vec![radix_hir::HirTriviaOwner {
            id: radix_hir::HirSourceAnchorId(1),
            kind: radix_hir::HirTriviaOwnerKind::Statement,
            semantic: None,
        }],
        attachments: vec![radix_hir::HirTriviaAttachment {
            anchor: radix_hir::HirSourceAnchorId(9),
            leading: Vec::new(),
        }],
        block_ends: Vec::new(),
        program_end: radix_hir::HirModuleEnd {
            id: radix_hir::HirSourceAnchorId(0),
            span: radix_lexer::Span::default(),
        },
        entry_start_anchor: None,
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        axis_sugar: Vec::new(),
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
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::DanglingIndex { kind, index, arena_size }) => {
            assert_eq!(kind, "HirSourceAnchorId");
            assert_eq!(index, 9);
            assert_eq!(arena_size, 1);
        }
        Err(other) => panic!("expected DanglingIndex(HirSourceAnchorId), got {other:?}"),
        Ok(_) => panic!("expected DanglingIndex(HirSourceAnchorId), got Ok"),
    }
}

/// Duplicate owner anchors → InvalidPresentation(duplicate_owner_anchor).
#[test]
fn hir_artifact_adversarial_duplicate_owner_anchor() {
    let mut artifact = minimal_test_artifact();
    artifact.presentation = radix_hir::HirPresentation {
        owners: vec![
            radix_hir::HirTriviaOwner {
                id: radix_hir::HirSourceAnchorId(1),
                kind: radix_hir::HirTriviaOwnerKind::Statement,
                semantic: None,
            },
            radix_hir::HirTriviaOwner {
                id: radix_hir::HirSourceAnchorId(1),
                kind: radix_hir::HirTriviaOwnerKind::Item,
                semantic: None,
            },
        ],
        attachments: Vec::new(),
        block_ends: Vec::new(),
        program_end: radix_hir::HirModuleEnd {
            id: radix_hir::HirSourceAnchorId(0),
            span: radix_lexer::Span::default(),
        },
        entry_start_anchor: None,
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        axis_sugar: Vec::new(),
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
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::InvalidPresentation { issue, anchor }) => {
            assert_eq!(issue, "duplicate_owner_anchor");
            assert_eq!(anchor, 1);
        }
        Err(other) => panic!("expected InvalidPresentation(duplicate_owner_anchor), got {other:?}"),
        Ok(_) => panic!("expected InvalidPresentation(duplicate_owner_anchor), got Ok"),
    }
}

/// Two attachments naming the same owner → InvalidPresentation.
#[test]
fn hir_artifact_adversarial_duplicate_attachment_owner() {
    let mut artifact = minimal_test_artifact();
    artifact.presentation = radix_hir::HirPresentation {
        owners: vec![radix_hir::HirTriviaOwner {
            id: radix_hir::HirSourceAnchorId(1),
            kind: radix_hir::HirTriviaOwnerKind::Statement,
            semantic: None,
        }],
        attachments: vec![
            radix_hir::HirTriviaAttachment { anchor: radix_hir::HirSourceAnchorId(1), leading: Vec::new() },
            radix_hir::HirTriviaAttachment { anchor: radix_hir::HirSourceAnchorId(1), leading: Vec::new() },
        ],
        block_ends: Vec::new(),
        program_end: radix_hir::HirModuleEnd {
            id: radix_hir::HirSourceAnchorId(0),
            span: radix_lexer::Span::default(),
        },
        entry_start_anchor: None,
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        axis_sugar: Vec::new(),
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
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::InvalidPresentation { issue, anchor }) => {
            assert_eq!(issue, "duplicate_attachment_owner");
            assert_eq!(anchor, 1);
        }
        Err(other) => panic!("expected InvalidPresentation(duplicate_attachment_owner), got {other:?}"),
        Ok(_) => panic!("expected InvalidPresentation(duplicate_attachment_owner), got Ok"),
    }
}

/// CommentLine text Symbol beyond the interner table → DanglingIndex.
#[test]
fn hir_artifact_adversarial_dangling_comment_symbol() {
    let mut artifact = minimal_test_artifact();
    artifact.presentation = radix_hir::HirPresentation {
        owners: vec![radix_hir::HirTriviaOwner {
            id: radix_hir::HirSourceAnchorId(1),
            kind: radix_hir::HirTriviaOwnerKind::Statement,
            semantic: None,
        }],
        attachments: vec![radix_hir::HirTriviaAttachment {
            anchor: radix_hir::HirSourceAnchorId(1),
            leading: vec![radix_hir::HirTrivia::CommentLine { text: Symbol(50), span: radix_lexer::Span::default() }],
        }],
        block_ends: Vec::new(),
        program_end: radix_hir::HirModuleEnd {
            id: radix_hir::HirSourceAnchorId(0),
            span: radix_lexer::Span::default(),
        },
        entry_start_anchor: None,
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        axis_sugar: Vec::new(),
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
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::DanglingIndex { kind, index, arena_size }) => {
            assert_eq!(kind, "Symbol");
            assert_eq!(index, 50);
            assert_eq!(arena_size, 2);
        }
        Err(other) => panic!("expected DanglingIndex(Symbol), got {other:?}"),
        Ok(_) => panic!("expected DanglingIndex(Symbol), got Ok"),
    }
}

/// Schema-v2 no-trivia artifact path stays deterministic byte-for-byte.
#[test]
fn hir_artifact_adversarial_no_trivia_determinism() {
    let artifact = minimal_test_artifact();
    let bytes1 = serialize_artifact(&artifact);
    let bytes2 = serialize_artifact(&artifact);
    assert_eq!(bytes1, bytes2, "no-trivia serialization must be deterministic");
    let loaded = load_fhir(&bytes1).expect("no-trivia schema-v2 artifact must load");
    assert_eq!(loaded.presentation, artifact.presentation);
}

// ── Structural-end anchor consistency classes (U-7 closeout hardening) ────

/// Two block ends sharing one anchor id → InvalidPresentation.
///
/// U-4 advisory: `validate_presentation` did not cross-check `block_ends` /
/// `program_end` ids against the owners registry. Lowering allocates every
/// source anchor from one arena, so a duplicated end anchor is corruption.
#[test]
fn hir_artifact_adversarial_duplicate_block_end_anchor() {
    let mut artifact = minimal_test_artifact();
    artifact.presentation = radix_hir::HirPresentation {
        owners: Vec::new(),
        attachments: Vec::new(),
        block_ends: vec![
            radix_hir::HirBlockEnd { id: radix_hir::HirSourceAnchorId(1), span: radix_lexer::Span::default() },
            radix_hir::HirBlockEnd { id: radix_hir::HirSourceAnchorId(1), span: radix_lexer::Span::default() },
        ],
        program_end: radix_hir::HirModuleEnd {
            id: radix_hir::HirSourceAnchorId(0),
            span: radix_lexer::Span::default(),
        },
        entry_start_anchor: None,
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        axis_sugar: Vec::new(),
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
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::InvalidPresentation { issue, anchor }) => {
            assert_eq!(issue, "duplicate_block_end_anchor");
            assert_eq!(anchor, 1);
        }
        Err(other) => panic!("expected InvalidPresentation(duplicate_block_end_anchor), got {other:?}"),
        Ok(_) => panic!("expected InvalidPresentation(duplicate_block_end_anchor), got Ok"),
    }
}

/// Program-end anchor colliding with a block-end anchor → InvalidPresentation.
#[test]
fn hir_artifact_adversarial_program_end_collides_with_block_end() {
    let mut artifact = minimal_test_artifact();
    artifact.presentation = radix_hir::HirPresentation {
        owners: Vec::new(),
        attachments: Vec::new(),
        block_ends: vec![radix_hir::HirBlockEnd {
            id: radix_hir::HirSourceAnchorId(1),
            span: radix_lexer::Span::default(),
        }],
        program_end: radix_hir::HirModuleEnd {
            id: radix_hir::HirSourceAnchorId(1),
            span: radix_lexer::Span::default(),
        },
        entry_start_anchor: None,
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        axis_sugar: Vec::new(),
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
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::InvalidPresentation { issue, anchor }) => {
            assert_eq!(issue, "program_end_collides_with_block_end");
            assert_eq!(anchor, 1);
        }
        Err(other) => panic!("expected InvalidPresentation(program_end_collides_with_block_end), got {other:?}"),
        Ok(_) => panic!("expected InvalidPresentation(program_end_collides_with_block_end), got Ok"),
    }
}

/// A non-end owner reusing an end anchor id → InvalidPresentation.
#[test]
fn hir_artifact_adversarial_end_anchor_reused_as_owner() {
    let mut artifact = minimal_test_artifact();
    artifact.presentation = radix_hir::HirPresentation {
        owners: vec![radix_hir::HirTriviaOwner {
            id: radix_hir::HirSourceAnchorId(1),
            kind: radix_hir::HirTriviaOwnerKind::Statement,
            semantic: None,
        }],
        attachments: Vec::new(),
        block_ends: vec![radix_hir::HirBlockEnd {
            id: radix_hir::HirSourceAnchorId(1),
            span: radix_lexer::Span::default(),
        }],
        program_end: radix_hir::HirModuleEnd {
            id: radix_hir::HirSourceAnchorId(0),
            span: radix_lexer::Span::default(),
        },
        entry_start_anchor: None,
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        axis_sugar: Vec::new(),
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
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::InvalidPresentation { issue, anchor }) => {
            assert_eq!(issue, "end_anchor_reused_as_owner");
            assert_eq!(anchor, 1);
        }
        Err(other) => panic!("expected InvalidPresentation(end_anchor_reused_as_owner), got {other:?}"),
        Ok(_) => panic!("expected InvalidPresentation(end_anchor_reused_as_owner), got Ok"),
    }
}

/// A BlockEnd-kind owner with no backing block-end record → InvalidPresentation.
#[test]
fn hir_artifact_adversarial_block_end_owner_without_end() {
    let mut artifact = minimal_test_artifact();
    artifact.presentation = radix_hir::HirPresentation {
        owners: vec![radix_hir::HirTriviaOwner {
            id: radix_hir::HirSourceAnchorId(1),
            kind: radix_hir::HirTriviaOwnerKind::BlockEnd,
            semantic: None,
        }],
        attachments: Vec::new(),
        block_ends: Vec::new(),
        program_end: radix_hir::HirModuleEnd {
            id: radix_hir::HirSourceAnchorId(0),
            span: radix_lexer::Span::default(),
        },
        entry_start_anchor: None,
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        axis_sugar: Vec::new(),
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
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::InvalidPresentation { issue, anchor }) => {
            assert_eq!(issue, "block_end_owner_without_end");
            assert_eq!(anchor, 1);
        }
        Err(other) => panic!("expected InvalidPresentation(block_end_owner_without_end), got {other:?}"),
        Ok(_) => panic!("expected InvalidPresentation(block_end_owner_without_end), got Ok"),
    }
}

/// A ProgramEnd-kind owner pointing at a different anchor → InvalidPresentation.
#[test]
fn hir_artifact_adversarial_program_end_owner_mismatch() {
    let mut artifact = minimal_test_artifact();
    artifact.presentation = radix_hir::HirPresentation {
        owners: vec![radix_hir::HirTriviaOwner {
            id: radix_hir::HirSourceAnchorId(9),
            kind: radix_hir::HirTriviaOwnerKind::ProgramEnd,
            semantic: None,
        }],
        attachments: Vec::new(),
        block_ends: Vec::new(),
        program_end: radix_hir::HirModuleEnd {
            id: radix_hir::HirSourceAnchorId(0),
            span: radix_lexer::Span::default(),
        },
        entry_start_anchor: None,
        entry_explicit: false,
        guard_si_ids: Vec::new(),
        elige_expr_ids: Vec::new(),
        glyph_ids: Vec::new(),
        numeric_twin_ids: Vec::new(),
        empty_dense_ids: Vec::new(),
        inferred_constructor_ids: Vec::new(),
        axis_sugar: Vec::new(),
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
    let bytes = serialize_artifact(&artifact);
    let result = load_fhir(&bytes);
    match result {
        Err(HirArtifactError::InvalidPresentation { issue, anchor }) => {
            assert_eq!(issue, "program_end_owner_mismatch");
            assert_eq!(anchor, 9);
        }
        Err(other) => panic!("expected InvalidPresentation(program_end_owner_mismatch), got {other:?}"),
        Ok(_) => panic!("expected InvalidPresentation(program_end_owner_mismatch), got Ok"),
    }
}
