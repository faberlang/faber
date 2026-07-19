use super::*;
use radix::lexer::Span;
use radix::mir::MirTempId;
use std::path::Path;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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
    assert_eq!(FMIR_BIN_ARTIFACT_DIR, "exe");
}

#[test]
fn fmir_manifest_and_image_file_names_are_configured() {
    assert_eq!(PACKAGE_MIR_MANIFEST_FILE, "image.toml");
    assert_eq!(FMIR_TEXT_IMAGE_FILE, "image.fmir.txt");
    assert_eq!(FMIR_IMAGE_FILE, "image.fmir");
}

#[test]
fn fmir_bin_entrypoint_and_runner_names_are_configured() {
    assert_eq!(FMIR_BIN_ENTRYPOINT_FILE, "run");
    assert_eq!(FMIR_BIN_RUNNER_CRATE_DIR, "runner");
    assert_eq!(FMIR_BIN_RUNNER_TARGET_DIR, "runner-target");
    assert_eq!(FMIR_BIN_RUNNER_PACKAGE_NAME, "faber-fmir-bin-runner");
}

#[test]
fn fnv1a64_constants_are_standard() {
    // Standard FNV-1a 64-bit offset and prime values.
    assert_eq!(FNV1A64_OFFSET, 0xcbf29ce484222325);
    assert_eq!(FNV1A64_PRIME, 0x100000001b3);
}

#[test]
fn package_mir_synthetic_def_base_is_above_hir_range() {
    assert_eq!(PACKAGE_MIR_SYNTHETIC_DEF_BASE, 2_000_000_000);
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
    assert_ne!(hash_a, hash_b, "different bytes must produce different hashes");
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

// ── toml_basic_string_path ──────────────────────────────────────────────────

#[test]
fn toml_basic_string_path_passes_through_simple_path() {
    let path = Path::new("src/main.fab");
    assert_eq!(toml_basic_string_path(path), "src/main.fab");
}

#[test]
fn toml_basic_string_path_escapes_backslashes() {
    let path = Path::new("windows\\path\\file.fab");
    assert_eq!(toml_basic_string_path(path), "windows\\\\path\\\\file.fab");
}

#[test]
fn toml_basic_string_path_escapes_double_quotes() {
    let path = Path::new("path/with\"quote");
    assert_eq!(toml_basic_string_path(path), "path/with\\\"quote");
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
    assert!(!is_known_fmir_runtime_requirement("kernel:unknown_module.verb"));
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

// ── is_bridged_norma_import_path ───────────────────────────────────────────

#[test]
fn is_bridged_norma_import_path_rejects_non_norma_prefix() {
    assert!(!is_bridged_norma_import_path("faber:something"));
    assert!(!is_bridged_norma_import_path(""));
    assert!(!is_bridged_norma_import_path("norma:"));
}

#[test]
fn is_bridged_norma_import_path_rejects_norma_without_module_name() {
    assert!(!is_bridged_norma_import_path("norma:"));
    assert!(!is_bridged_norma_import_path("norma:/solum"));
}

// ── fmir_text_cli_value_type ───────────────────────────────────────────────

#[test]
fn fmir_text_cli_value_type_maps_textus_and_ignotum() {
    assert!(matches!(
        fmir_text_cli_value_type(&radix::cli::CliType::Textus),
        Some(FmirTextCliValueType::Textus)
    ));
    assert!(matches!(
        fmir_text_cli_value_type(&radix::cli::CliType::Ignotum),
        Some(FmirTextCliValueType::Textus)
    ));
}

#[test]
fn fmir_text_cli_value_type_maps_numeric_types() {
    assert!(matches!(
        fmir_text_cli_value_type(&radix::cli::CliType::Numerus),
        Some(FmirTextCliValueType::Numerus)
    ));
    assert!(matches!(
        fmir_text_cli_value_type(&radix::cli::CliType::Fractus),
        Some(FmirTextCliValueType::Fractus)
    ));
}

#[test]
fn fmir_text_cli_value_type_maps_bivalens() {
    assert!(matches!(
        fmir_text_cli_value_type(&radix::cli::CliType::Bivalens),
        Some(FmirTextCliValueType::Bivalens)
    ));
}

#[test]
fn fmir_text_cli_value_type_returns_none_for_list_types() {
    assert!(fmir_text_cli_value_type(&radix::cli::CliType::Octeti).is_none());
    assert!(fmir_text_cli_value_type(&radix::cli::CliType::ListaTextus).is_none());
    assert!(fmir_text_cli_value_type(&radix::cli::CliType::ListaNumerus).is_none());
}

// ── library_identity_label ─────────────────────────────────────────────────

#[test]
fn library_identity_label_formats_builtin_provider() {
    use radix::hir::LibraryProvider;
    let identity = radix::hir::LibraryIdentity {
        provider: LibraryProvider::Builtin("norma".to_owned()),
        module_path: vec!["solum".to_string()],
    };
    assert_eq!(library_identity_label(&identity), "norma:solum");
}

#[test]
fn library_identity_label_formats_package_provider() {
    use radix::hir::LibraryProvider;
    let identity = radix::hir::LibraryIdentity {
        provider: LibraryProvider::Package("my-lib".to_owned()),
        module_path: vec!["foo".to_string(), "bar".to_string()],
    };
    assert_eq!(
        library_identity_label(&identity),
        "my-lib:foo/bar"
    );
}

#[test]
fn library_identity_label_handles_multi_segment_module_path() {
    use radix::hir::LibraryProvider;
    let identity = radix::hir::LibraryIdentity {
        provider: LibraryProvider::Builtin("norma".to_owned()),
        module_path: vec!["http".to_string(), "v1".to_string(), "client".to_string()],
    };
    assert_eq!(
        library_identity_label(&identity),
        "norma:http/v1/client"
    );
}

// ── is_bridged_norma_module ────────────────────────────────────────────────

#[test]
fn is_bridged_norma_module_returns_false_for_non_norma_builtin() {
    use radix::hir::LibraryProvider;
    let identity = radix::hir::LibraryIdentity {
        provider: LibraryProvider::Builtin("faber".to_owned()),
        module_path: vec!["solum".to_string()],
    };
    assert!(!is_bridged_norma_module(&identity));
}

#[test]
fn is_bridged_norma_module_returns_false_for_package_provider() {
    use radix::hir::LibraryProvider;
    let identity = radix::hir::LibraryIdentity {
        provider: LibraryProvider::Package("any-pkg".to_owned()),
        module_path: vec!["solum".to_string()],
    };
    assert!(!is_bridged_norma_module(&identity));
}

// ── validate_package_mir_manifest ───────────────────────────────────────────

#[test]
fn validate_package_mir_manifest_accepts_valid_manifest() {
    let manifest = format!(
        r#"version = {version}
target = "{target}"

[package]
name = "test-package"

entry = "main.fab"

[runtime]
"#,
        version = PACKAGE_MIR_ARTIFACT_VERSION,
        target = PACKAGE_MIR_TARGET_NAME
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
        r#"version = {version}
entry = "main.fab"

[runtime]
"#,
        version = PACKAGE_MIR_ARTIFACT_VERSION
    );
    let result = validate_package_mir_manifest(&manifest, Path::new("/fake/path"));
    assert!(result.is_err());
}

#[test]
fn validate_package_mir_manifest_rejects_missing_entry() {
    let manifest = format!(
        r#"version = {version}
target = "{target}"

[runtime]
"#,
        version = PACKAGE_MIR_ARTIFACT_VERSION,
        target = PACKAGE_MIR_TARGET_NAME
    );
    let result = validate_package_mir_manifest(&manifest, Path::new("/fake/path"));
    assert!(result.is_err());
}

#[test]
fn validate_package_mir_manifest_rejects_missing_runtime_section() {
    let manifest = format!(
        r#"version = {version}
target = "{target}"
entry = "main.fab"
"#,
        version = PACKAGE_MIR_ARTIFACT_VERSION,
        target = PACKAGE_MIR_TARGET_NAME,
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
        r#"version = {version}
target = "fmir"
entry = "main.fab"

[runtime]
"#,
        version = PACKAGE_MIR_ARTIFACT_VERSION,
    );
    let result = validate_package_mir_manifest(&manifest, Path::new("/fake/path"));
    assert!(result.is_err());
}

// ── make_fmir_bin_entrypoint_executable (unix) ─────────────────────────────

#[cfg(unix)]
#[test]
fn make_fmir_bin_entrypoint_executable_sets_755_permissions() {
    let dir = std::env::temp_dir().join(format!(
        "faber-mir-entrypoint-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let entrypoint = dir.join("run");
    std::fs::write(&entrypoint, "#!/bin/sh\necho hello").expect("write entrypoint");

    let result = make_fmir_bin_entrypoint_executable(&entrypoint, &dir);
    assert!(result.is_ok(), "make_fmir_bin_entrypoint_executable should succeed");

    let metadata = std::fs::metadata(&entrypoint).expect("read metadata");
    let mode = metadata.permissions().mode();
    assert_eq!(mode & 0o777, 0o755, "entrypoint should be executable (755)");

    let _ = std::fs::remove_dir_all(&dir);
}

// ── Existing tests ──────────────────────────────────────────────────────────

#[test]
fn fmir_runtime_cli_binding_skips_superset_decoy_record() {
    let mut interner = Interner::default();
    let run_entry = interner.intern("run_entry");
    let name = interner.intern("name");
    let extra = interner.intern("extra");
    let build_time = interner.intern("build-time");
    let decoy_extra = interner.intern("decoy-extra");
    let runtime = interner.intern("runtime");
    let ty = MirType::semantic(TypeId(0));
    let span = Span::default();
    let mut program = MirProgram {
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

    let patched = patch_fmir_text_cli_record(
        &mut program,
        &cli,
        "run_entry",
        &interner,
        &[MirNamedOperand {
            name,
            value: MirOperand::Constant(MirConstant::String(runtime)),
        }],
    );

    assert!(patched);
    assert_eq!(record_field_string(&program, 0, name), Some(build_time));
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

// R0 red artifact contract: adding `MirConstant::UInt(u64)` to the serialized
// MIR schema is an approved clean break, so the package MIR artifact version
// moves 2 → 3 (no dual-format reader). Fails until R2 lands the bump.
#[test]
fn package_mir_artifact_version_is_3_for_unsigned_constant_schema() {
    assert_eq!(
        PACKAGE_MIR_ARTIFACT_VERSION, 3,
        "MirConstant::UInt requires the FMIR artifact version 3 clean break"
    );
}
