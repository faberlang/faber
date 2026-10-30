use super::cli_bind::{FmirTextCliBinding, patch_fmir_text_cli_record};
use super::cli_plan::{fmir_text_cli_value_type, fmir_text_runtime_cli_value_type};
use super::lower::{append_shifted_parts, checked_shifted_function_id};
use super::manifest::escape_manifest_value;
use super::requirements::{is_known_fmir_kernel_requirement, is_known_fmir_runtime_requirement};
use super::*;
use radix_mir::MirTempId;
use radix_module::lexer::Span;
use std::path::Path;

// The SOURCE-IDENTITY FNV-1a 64-bit constants (ddpp0-contract §FnvRemoval
// "Separate surface" — the S1/S2 source-identity surface, NOT the removed
// backend-artifact provenance): the local `fnv1a64` in mir/mod.rs uses the
// standard FNV-1a offset/prime; these test constants pin that standard so
// the `fnv1a64_*` unit tests below prove the source-identity primitive.
const FNV1A64_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A64_PRIME: u64 = 0x0100_0000_01b3;

// ── Constant / configuration tests ──────────────────────────────────────────

#[test]
fn package_mir_target_scena_is_configured() {
    assert_eq!(PACKAGE_MIR_TARGET_NAME, "scena");
}

#[test]
fn fmir_text_target_name_is_configured() {
    assert_eq!(FMIR_TEXT_TARGET_NAME, "fmir-text");
    assert_eq!(FMIR_TARGET_NAME, "fmir");
}

#[test]
fn fmir_artifact_dir_names_are_configured() {
    assert_eq!(PACKAGE_MIR_ARTIFACT_DIR, "faber-mir");
}

#[test]
fn fmir_manifest_and_image_file_names_are_configured() {
    assert_eq!(PACKAGE_MIR_MANIFEST_FILE, "image.toml");
    assert_eq!(FMIR_TEXT_IMAGE_FILE, "image.fmir.txt");
    assert_eq!(FMIR_IMAGE_FILE, "image.fmir");
}

#[test]
fn fnv1a64_constants_are_standard() {
    // Standard FNV-1a 64-bit offset and prime values.
    assert_eq!(FNV1A64_OFFSET, 0xcbf2_9ce4_8422_2325);
    assert_eq!(FNV1A64_PRIME, 0x0100_0000_01b3);
}

#[test]
fn package_mir_synthetic_def_base_is_above_hir_range() {
    assert_eq!(PACKAGE_MIR_SYNTHETIC_DEF_BASE, 2_000_000_000);
}

#[test]
fn package_mir_append_uses_sparse_prefix_max_for_dispatch_id() {
    let mut merged = MirProgram {
        functions: vec![stub_mir_function(0), stub_mir_function(2)],
    };
    let mut appended = MirProgram {
        functions: vec![stub_mir_function(0)],
    };
    let mut merged_environments = Vec::new();
    let mut appended_environments = Vec::new();

    let offset = append_shifted_parts(
        &mut merged,
        &mut merged_environments,
        &mut appended,
        &mut appended_environments,
        Path::new("sparse.fab"),
    )
    .expect("sparse prefix append");
    let dispatch = checked_shifted_function_id(MirFunctionId(0), offset, Path::new("sparse.fab"))
        .expect("selected dispatch id");

    assert_eq!(offset, 3);
    assert_eq!(dispatch, MirFunctionId(3));
    assert_eq!(
        merged
            .functions
            .iter()
            .map(|function| function.id)
            .collect::<Vec<_>>(),
        vec![MirFunctionId(0), MirFunctionId(2), MirFunctionId(3)]
    );
    assert!(
        merged
            .functions
            .iter()
            .any(|function| function.id == dispatch),
        "selected CLI dispatch must target the uniquely appended function"
    );
}

#[test]
fn package_mir_append_keeps_dense_offset_equivalent() {
    let mut merged = MirProgram {
        functions: vec![stub_mir_function(0), stub_mir_function(1)],
    };
    let mut appended = MirProgram {
        functions: vec![stub_mir_function(0)],
    };
    let mut merged_environments = Vec::new();
    let mut appended_environments = Vec::new();

    let offset = append_shifted_parts(
        &mut merged,
        &mut merged_environments,
        &mut appended,
        &mut appended_environments,
        Path::new("dense.fab"),
    )
    .expect("dense prefix append");

    assert_eq!(offset, 2);
    assert_eq!(
        merged
            .functions
            .iter()
            .map(|function| function.id)
            .collect::<Vec<_>>(),
        vec![MirFunctionId(0), MirFunctionId(1), MirFunctionId(2)]
    );
}

#[test]
fn package_mir_append_reports_function_id_overflow() {
    let mut merged = MirProgram {
        functions: vec![stub_mir_function(u32::MAX)],
    };
    let mut appended = MirProgram {
        functions: vec![stub_mir_function(0)],
    };
    let mut merged_environments = Vec::new();
    let mut appended_environments = Vec::new();

    let diagnostics = append_shifted_parts(
        &mut merged,
        &mut merged_environments,
        &mut appended,
        &mut appended_environments,
        Path::new("overflow.fab"),
    )
    .expect_err("exhausted function id space must fail closed");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].issue(),
        Some("package_mir_function_id_overflow")
    );
    assert_eq!(merged.functions[0].id, MirFunctionId(u32::MAX));
    assert_eq!(appended.functions[0].id, MirFunctionId(0));
}

fn stub_mir_function(id: u32) -> MirFunction {
    let span = Span::default();
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span,
            },
            span,
        }],
        return_ty: MirType::semantic(TypeId(0)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span,
    }
}

// ── definition-source identity ──────────────────────────────────────────────

#[test]
fn package_mir_link_allows_same_source_monomorphs() {
    // Two instances of one generic (nn.linear<2,2,2> and nn.linear<4,4,4>)
    // share the declaration source; they are not a rewrite collision.
    let span = Span::new(16509, 16663);
    let mut first = stub_mir_function(35);
    first.source = Some(DefId(2_000_000_143));
    first.name = Some(Symbol(490));
    first.span = span;
    first.return_ty = MirType::semantic(TypeId(1357));
    let mut second = stub_mir_function(37);
    second.source = Some(DefId(2_000_000_143));
    second.name = Some(Symbol(490));
    second.span = span;
    second.return_ty = MirType::semantic(TypeId(1352));

    super::lower::ensure_unique_definition_sources(
        &MirProgram {
            functions: vec![first, second],
        },
        Path::new("nn.fab"),
    )
    .expect("same-name same-span monomorphs share a source");
}

#[test]
fn package_mir_monomorphization_key_distinguishes_shapes() {
    // Instantiation identity is (source, type_args, shape_args). Two
    // instances of one generic at [2] and [8] must not collide in the
    // package-link instance index (MIXED-SHAPE-MIR-LINK).
    let source = DefId(2_000_000_143);
    let two = radix_mir::MirMonomorphizationKey {
        source,
        type_args: Vec::new(),
        shape_args: vec![radix_module::semantic::IndexId(2)],
    };
    let eight = radix_mir::MirMonomorphizationKey {
        source,
        type_args: Vec::new(),
        shape_args: vec![radix_module::semantic::IndexId(8)],
    };
    assert_ne!(two, eight, "distinct shapes must not share a key");
    let mut index = std::collections::HashMap::new();
    index.insert(two.clone(), MirFunctionId(1));
    index.insert(eight.clone(), MirFunctionId(2));
    assert_eq!(index.get(&two), Some(&MirFunctionId(1)));
    assert_eq!(index.get(&eight), Some(&MirFunctionId(2)));
    assert_eq!(index.len(), 2);
}

#[test]
fn package_mir_link_rejects_distinct_defs_on_one_source() {
    let mut first = stub_mir_function(1);
    first.source = Some(DefId(2_000_000_143));
    first.name = Some(Symbol(1));
    first.span = Span::new(10, 20);
    let mut second = stub_mir_function(2);
    second.source = Some(DefId(2_000_000_143));
    second.name = Some(Symbol(2));
    second.span = Span::new(40, 50);

    let diagnostics = super::lower::ensure_unique_definition_sources(
        &MirProgram {
            functions: vec![first, second],
        },
        Path::new("nn.fab"),
    )
    .expect_err("distinct declarations must not share a source");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, Some("PKG001"));
    assert_eq!(
        diagnostics[0].issue(),
        Some("package_mir_duplicate_function_source")
    );
    assert!(
        diagnostics[0]
            .args
            .iter()
            .any(|arg| arg.name == "def_id" && arg.value == "2000000143"),
        "collision diagnostic must name the shared source def, got {:?}",
        diagnostics[0].args
    );
}

// ── fnv1a64 ────────────────────────────────────────────────────────────────

#[test]
fn fnv1a64_empty_input_produces_offset() {
    assert_eq!(fnv1a64(b""), FNV1A64_OFFSET);
}

#[test]
fn fnv1a64_single_byte_produces_deterministic_hash() {
    let hash_a = fnv1a64(b"a");
    let hash_b = fnv1a64(b"b");
    assert_ne!(
        hash_a, hash_b,
        "different bytes must produce different hashes"
    );
    // Re-run to verify determinism.
    assert_eq!(fnv1a64(b"a"), hash_a);
}

#[test]
fn fnv1a64_same_input_produces_same_hash() {
    let input = b"hello, fmir image";
    assert_eq!(fnv1a64(input), fnv1a64(input));
}

#[test]
fn fnv1a64_different_inputs_produce_different_hashes() {
    assert_ne!(fnv1a64(b"abc"), fnv1a64(b"xyz"));
}

#[test]
fn fnv1a64_long_input_is_deterministic() {
    let long = b"The quick brown fox jumps over the lazy dog. The quick brown fox jumps over the lazy dog.";
    assert_eq!(fnv1a64(long), fnv1a64(long));
}

// ── escape_manifest_value ──────────────────────────────────────────────────

#[test]
fn escape_manifest_value_passes_through_plain_text() {
    assert_eq!(escape_manifest_value("hello"), "hello");
}

#[test]
fn escape_manifest_value_escapes_backslashes() {
    assert_eq!(escape_manifest_value("a\\b"), "a\\\\b");
}

#[test]
fn escape_manifest_value_escapes_double_quotes() {
    assert_eq!(escape_manifest_value("say \"hello\""), "say \\\"hello\\\"");
}

#[test]
fn escape_manifest_value_escapes_both_backslashes_and_quotes() {
    assert_eq!(
        escape_manifest_value("path\\to\\\"file\""),
        "path\\\\to\\\\\\\"file\\\""
    );
}

#[test]
fn escape_manifest_value_escapes_empty_string() {
    assert_eq!(escape_manifest_value(""), "");
}

// ── relative_or_display ────────────────────────────────────────────────────

#[test]
fn relative_or_display_strips_root_prefix() {
    let root = Path::new("/home/user/project");
    let path = Path::new("/home/user/project/src/main.fab");
    assert_eq!(relative_or_display(root, path), "src/main.fab");
}

#[test]
fn relative_or_display_returns_full_path_when_not_under_root() {
    let root = Path::new("/home/user/project");
    let path = Path::new("/tmp/other-file.fab");
    assert_eq!(relative_or_display(root, path), "/tmp/other-file.fab");
}

#[test]
fn relative_or_display_returns_filename_when_path_is_root() {
    let root = Path::new("/home/user/project");
    assert_eq!(relative_or_display(root, root), "");
}

#[test]
fn relative_or_display_handles_nested_subdirectory() {
    let root = Path::new("/a/b");
    let path = Path::new("/a/b/c/d/e.fab");
    assert_eq!(relative_or_display(root, path), "c/d/e.fab");
}

// ── is_known_fmir_runtime_requirement ──────────────────────────────────────

#[test]
fn is_known_fmir_runtime_requirement_accepts_host_argv() {
    assert!(is_known_fmir_runtime_requirement("host:argv"));
    assert!(is_known_fmir_runtime_requirement("host:exit"));
    assert!(is_known_fmir_runtime_requirement("host:stdout"));
}

#[test]
fn is_known_fmir_runtime_requirement_accepts_host_fs_and_env() {
    assert!(is_known_fmir_runtime_requirement("host:fs"));
    assert!(is_known_fmir_runtime_requirement("host:env"));
    assert!(is_known_fmir_runtime_requirement("host:cwd"));
    assert!(is_known_fmir_runtime_requirement("host:pid"));
    assert!(is_known_fmir_runtime_requirement("host:random"));
    assert!(is_known_fmir_runtime_requirement("host:process"));
}

#[test]
fn is_known_fmir_runtime_requirement_accepts_host_stdin_and_stderr() {
    assert!(is_known_fmir_runtime_requirement("host:stdin"));
    assert!(is_known_fmir_runtime_requirement("host:stderr"));
}

#[test]
fn is_known_fmir_runtime_requirement_rejects_unknown_host_prefix() {
    assert!(!is_known_fmir_runtime_requirement("host:unknown"));
    assert!(!is_known_fmir_runtime_requirement("host:"));
}

#[test]
fn is_known_fmir_runtime_requirement_rejects_empty_and_garbage() {
    assert!(!is_known_fmir_runtime_requirement(""));
    assert!(!is_known_fmir_runtime_requirement("not-a-requirement"));
    assert!(!is_known_fmir_runtime_requirement(
        "kernel:unknown_module.verb"
    ));
}

// ── is_known_fmir_kernel_requirement ────────────────────────────────────────

#[test]
fn is_known_fmir_kernel_requirement_rejects_non_kernel_prefix() {
    assert!(!is_known_fmir_kernel_requirement("host:argv"));
    assert!(!is_known_fmir_kernel_requirement(""));
    assert!(!is_known_fmir_kernel_requirement("kernel:"));
}

#[test]
fn is_known_fmir_kernel_requirement_rejects_missing_verb() {
    assert!(!is_known_fmir_kernel_requirement("kernel:solum"));
    assert!(!is_known_fmir_kernel_requirement("kernel:tempus"));
}

#[test]
fn package_mir_accepts_octeti_option_operand_and_rest_operand() {
    use super::cli_values::{
        cli_record_type, is_package_mir_supported_operand, is_package_mir_supported_option,
    };
    use radix_module::cli::{CliOperand, CliOption, CliType};
    let mut interner = radix_module::lexer::Interner::default();
    let data = interner.intern("data");
    let operand = |rest| CliOperand {
        binding: "data".to_owned(),
        binding_symbol: data,
        ty: CliType::Octeti,
        rest,
        description: None,
        global: false,
        default: None,
        span: Span::default(),
    };
    // A plain and a final rest octeti operand are both package-MIR operands.
    assert!(is_package_mir_supported_operand(&operand(false), true));
    assert!(is_package_mir_supported_operand(&operand(true), true));
    assert!(!is_package_mir_supported_operand(&operand(true), false));

    let option = CliOption {
        binding: "data".to_owned(),
        binding_symbol: data,
        ty: CliType::Octeti,
        long: Some("data".to_owned()),
        short: None,
        flag: false,
        description: None,
        global: false,
        default: None,
        span: Span::default(),
    };
    assert!(is_package_mir_supported_option(&option));

    // The record field type is `list<b8>`; a rest operand is a list of those.
    let mut types = radix_module::semantic::TypeTable::new();
    let bytes = types.octeti_list();
    assert_eq!(cli_record_type(&mut types, &CliType::Octeti, false), bytes);
    let rows = types.array(bytes);
    assert_eq!(cli_record_type(&mut types, &CliType::Octeti, true), rows);
}

// ── fmir_text_cli_value_type ───────────────────────────────────────────────

#[test]
fn fmir_text_cli_value_type_maps_textus_and_ignotum() {
    assert!(matches!(
        fmir_text_cli_value_type(&radix_module::cli::CliType::Textus),
        Some(FmirTextCliValueType::Textus)
    ));
    assert!(matches!(
        fmir_text_cli_value_type(&radix_module::cli::CliType::Ignotum),
        Some(FmirTextCliValueType::Textus)
    ));
}

#[test]
fn fmir_text_cli_value_type_maps_numeric_types() {
    assert!(matches!(
        fmir_text_cli_value_type(&radix_module::cli::CliType::Numerus),
        Some(FmirTextCliValueType::Numerus)
    ));
    assert!(matches!(
        fmir_text_cli_value_type(&radix_module::cli::CliType::Fractus),
        Some(FmirTextCliValueType::Fractus)
    ));
}

#[test]
fn fmir_text_cli_value_type_maps_bivalens() {
    assert!(matches!(
        fmir_text_cli_value_type(&radix_module::cli::CliType::Bivalens),
        Some(FmirTextCliValueType::Bivalens)
    ));
}

#[test]
fn fmir_text_cli_value_type_maps_lista_textus() {
    assert!(matches!(
        fmir_text_cli_value_type(&radix_module::cli::CliType::ListaTextus),
        Some(FmirTextCliValueType::ListaTextus)
    ));
}

#[test]
fn fmir_text_runtime_cli_value_type_maps_rest_textus_to_lista() {
    let operand = radix_module::cli::CliOperand {
        binding: "files".to_owned(),
        binding_symbol: radix_module::lexer::Interner::default().intern("files"),
        ty: radix_module::cli::CliType::Textus,
        rest: true,
        description: None,
        global: false,
        default: None,
        span: Span::default(),
    };
    assert!(matches!(
        fmir_text_runtime_cli_value_type(&operand),
        Some(FmirTextCliValueType::ListaTextus)
    ));
}

#[test]
fn fmir_text_cli_value_type_returns_none_for_unsupported_list_types() {
    assert!(fmir_text_cli_value_type(&radix_module::cli::CliType::Octeti).is_none());
    assert!(fmir_text_cli_value_type(&radix_module::cli::CliType::ListaNumerus).is_none());
}

// ── validate_package_mir_manifest ───────────────────────────────────────────

#[test]
fn validate_package_mir_manifest_accepts_valid_manifest() {
    let manifest = format!(
        r#"version = {PACKAGE_MIR_ARTIFACT_VERSION}
target = "{PACKAGE_MIR_TARGET_NAME}"

[package]
name = "test-package"

entry = "main.fab"

[runtime]
"#
    );
    let result = validate_package_mir_manifest(&manifest, Path::new("/fake/path"));
    assert!(result.is_ok());
}

#[test]
fn validate_package_mir_manifest_rejects_missing_version() {
    let manifest = r#"
target = "scena"
entry = "main.fab"

[runtime]
"#;
    let result = validate_package_mir_manifest(manifest, Path::new("/fake/path"));
    assert!(result.is_err());
}

#[test]
fn validate_package_mir_manifest_rejects_missing_target() {
    let manifest = format!(
        r#"version = {PACKAGE_MIR_ARTIFACT_VERSION}
entry = "main.fab"

[runtime]
"#
    );
    let result = validate_package_mir_manifest(&manifest, Path::new("/fake/path"));
    assert!(result.is_err());
}

#[test]
fn validate_package_mir_manifest_rejects_missing_entry() {
    let manifest = format!(
        r#"version = {PACKAGE_MIR_ARTIFACT_VERSION}
target = "{PACKAGE_MIR_TARGET_NAME}"

[runtime]
"#,
    );
    let result = validate_package_mir_manifest(&manifest, Path::new("/fake/path"));
    assert!(result.is_err());
}

#[test]
fn validate_package_mir_manifest_rejects_missing_runtime_section() {
    let manifest = format!(
        r#"version = {PACKAGE_MIR_ARTIFACT_VERSION}
target = "{PACKAGE_MIR_TARGET_NAME}"
entry = "main.fab"
"#,
    );
    let result = validate_package_mir_manifest(&manifest, Path::new("/fake/path"));
    assert!(result.is_err());
}

#[test]
fn validate_package_mir_manifest_rejects_wrong_version() {
    let manifest = r#"
version = 42
target = "scena"
entry = "main.fab"

[runtime]
"#;
    let result = validate_package_mir_manifest(manifest, Path::new("/fake/path"));
    assert!(result.is_err());
}

#[test]
fn validate_package_mir_manifest_rejects_wrong_target() {
    let manifest = format!(
        r#"version = {PACKAGE_MIR_ARTIFACT_VERSION}
target = "fmir"
entry = "main.fab"

[runtime]
"#,
    );
    let result = validate_package_mir_manifest(&manifest, Path::new("/fake/path"));
    assert!(result.is_err());
}

// ── patch_fmir_text_cli_record (decoy superset) ────────────────────────────

/// Shared setup for the decoy-superset patching tests.
fn decoy_test_setup(interner: &mut Interner) -> (MirProgram, FmirTextCliSection, Symbol) {
    let run_entry = interner.intern("run_entry");
    let name = interner.intern("name");
    let extra = interner.intern("extra");
    let build_time = interner.intern("build-time");
    let decoy_extra = interner.intern("decoy-extra");
    let ty = MirType::semantic(TypeId(0));
    let span = Span::default();
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: Some(run_entry),
            params: Vec::new(),
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![
                    record_construct(
                        MirTempId(0),
                        ty,
                        span,
                        vec![
                            MirNamedOperand {
                                name,
                                value: MirOperand::Constant(MirConstant::String(build_time)),
                            },
                            MirNamedOperand {
                                name: extra,
                                value: MirOperand::Constant(MirConstant::String(decoy_extra)),
                            },
                        ],
                    ),
                    record_construct(
                        MirTempId(1),
                        ty,
                        span,
                        vec![MirNamedOperand {
                            name,
                            value: MirOperand::Constant(MirConstant::String(build_time)),
                        }],
                    ),
                ],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(None),
                    span,
                },
                span,
            }],
            return_ty: ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        }],
    };
    let cli = FmirTextCliSection {
        root: FmirTextCliRootSection {
            record: "args".to_owned(),
            operand: vec![FmirTextCliOperand {
                field: "name".to_owned(),
                ty: FmirTextCliValueType::Textus,
            }],
        },
    };
    (program, cli, name)
}

#[test]
fn fmir_runtime_cli_binding_preserves_decoy_fields() {
    let mut interner = Interner::default();
    let (mut program, cli, name) = decoy_test_setup(&mut interner);
    let runtime = interner.intern("runtime");

    let patched = patch_fmir_text_cli_record(
        &mut program,
        &cli,
        "run_entry",
        &interner,
        &[FmirTextCliBinding::Scalar(MirNamedOperand {
            name,
            value: MirOperand::Constant(MirConstant::String(runtime)),
        })],
    );

    // Superset record (extra field) keeps its original value — only exact
    // field-name matches are replaced.
    assert!(patched);
    let build_time = interner.intern("build-time");
    assert_eq!(record_field_string(&program, 0, name), Some(build_time));
}

#[test]
fn fmir_runtime_cli_binding_patches_matching_fields() {
    let mut interner = Interner::default();
    let (mut program, cli, name) = decoy_test_setup(&mut interner);
    let runtime = interner.intern("runtime");

    let patched = patch_fmir_text_cli_record(
        &mut program,
        &cli,
        "run_entry",
        &interner,
        &[FmirTextCliBinding::Scalar(MirNamedOperand {
            name,
            value: MirOperand::Constant(MirConstant::String(runtime)),
        })],
    );

    // Exact-match record gets the runtime binding value.
    assert!(patched);
    assert_eq!(record_field_string(&program, 1, name), Some(runtime));
}

fn record_construct(
    destination: MirTempId,
    ty: MirType,
    span: Span,
    fields: Vec<MirNamedOperand>,
) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::Construct {
            destination: MirPlace::temp(destination),
            aggregate: MirAggregate {
                kind: MirAggregateKind::Record,
                ty,
                fields: MirAggregateFields::Named(fields),
            },
        },
        span,
    }
}

fn record_field_string(
    program: &MirProgram,
    statement_index: usize,
    field_name: Symbol,
) -> Option<Symbol> {
    let statement = program
        .functions
        .first()?
        .blocks
        .first()?
        .statements
        .get(statement_index)?;
    let MirStatementKind::Construct { aggregate, .. } = &statement.kind else {
        return None;
    };
    let MirAggregateFields::Named(fields) = &aggregate.fields else {
        return None;
    };
    fields.iter().find_map(|field| {
        if field.name != field_name {
            return None;
        }
        match field.value {
            MirOperand::Constant(MirConstant::String(symbol)) => Some(symbol),
            _ => None,
        }
    })
}

// R0 red artifact contract: the operator-authorized package-seam breaks
// moved the package MIR artifact version 4 → 5 (typed complete-program wire,
// S3-A4), 5 → 6 (the Stage 3R F1–F7 frozen contract: semantic identities,
// generations, roots/dependencies, relations, and the independent
// initialization/observation axes, S2), 6 → 7 (the frozen DeviceArtifact
// packet shape, DDCP2-U2), and 7 → 8 (the ordered `MirIntrinsic::Scan`
// payload). No dual-format reader exists; non-current images fail admission
// closed.
#[test]
fn package_mir_artifact_version_is_8_for_carried_semantics_schema() {
    assert_eq!(
        PACKAGE_MIR_ARTIFACT_VERSION, 8,
        "the carried semantics schema require the FMIR artifact version 8 clean break"
    );
}

// ── S6-U8: decomposed-companion reverse-AD seed provisioning ──────────────

// The S6-U8 reverse-AD upstream-seed proof requires faber's device-section
// builder (a `MirSeams::device_section_builder` seam body compiled under
// `device-runtime`), so it stays faber-side with the other device
// tests (`crates/faber/src/package/mir/mir_test.rs`); RP5 moves only the
// compiler-owned tests here.
