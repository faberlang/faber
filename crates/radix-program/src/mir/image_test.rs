//! FMIR image loading: the decode-error diagnostics, the loaders' refusals,
//! the header-to-image round trip, and the validation-metadata carrier.

use super::*;
use radix_mir::{MirClosureEnvironmentId, MirFunctionId, MirLocalId};
use radix_types::TypeId;

const TOOLCHAIN: &str = "9.9.9";

fn path() -> PathBuf {
    PathBuf::from("pkg/image.fmir.txt")
}

fn arg<'a>(diagnostic: &'a Diagnostic, name: &str) -> Option<&'a str> {
    diagnostic
        .args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

fn style(label: &'static str) -> FmirImageErrorStyle {
    FmirImageErrorStyle {
        label,
        parse_verb: "parse",
        parse_issue: Some("parse_issue"),
        version_issue: "version_issue",
    }
}

fn one(error: FmirImageError, label: &'static str) -> Diagnostic {
    let mut diagnostics = fmir_image_decode_error(error, &path(), &style(label));
    assert_eq!(diagnostics.len(), 1);
    diagnostics.remove(0)
}

fn header(target: &str, version: u32) -> FmirImageHeader {
    FmirImageHeader {
        version,
        target: target.to_owned(),
        package_root: ".".to_owned(),
        entry: "main.fab".to_owned(),
        entry_function: "run_entry".to_owned(),
        toolchain: FmirTextToolchainSection {
            faber_cli_version: TOOLCHAIN.to_owned(),
        },
        runtime: FmirTextRuntimeSection {
            requirement: vec!["io".to_owned()],
        },
        sources: FmirTextSourcesSection {
            source: vec![radix_mir_fmir::FmirTextSourceIdentity {
                file: "main.fab".to_owned(),
                hash: "00ff".to_owned(),
            }],
        },
        cli: None,
        exit_code: Some(3),
        types: FmirTextTypesSection {
            table: radix_types::TypeTable::new().snapshot(),
        },
        interner: vec!["alpha".to_owned(), "beta".to_owned()],
        device: None,
    }
}

fn text_image(target: &str, version: u32, program_json: &str) -> String {
    let image = header(target, version).into_text_image(program_json.to_owned());
    encode_text_image(&image).expect("encode text image")
}

fn empty_program_json() -> String {
    serde_json::to_string(&MirProgram::new()).expect("encode program")
}

#[test]
fn every_device_and_section_decode_failure_keeps_its_issue_and_numbers() {
    // (error, issue arg, the structured args the diagnostic carries)
    let cases: Vec<(FmirImageError, Option<&str>, Vec<(&str, &str)>)> = vec![
        (
            FmirImageError::Parse("bad toml".into()),
            Some("parse_issue"),
            vec![("detail", "bad toml")],
        ),
        (
            FmirImageError::VersionMismatch {
                actual: 2,
                expected: 7,
            },
            Some("version_issue"),
            vec![("actual", "2"), ("expected", "7")],
        ),
        (
            FmirImageError::TargetMismatch {
                found: "x".into(),
                expected: "fmir",
            },
            None,
            vec![("found", "x"), ("expected", "fmir")],
        ),
        (
            FmirImageError::ToolchainMismatch {
                found: "1.0".into(),
                expected: "2.0".into(),
            },
            None,
            vec![("found", "1.0"), ("expected", "2.0")],
        ),
        (
            FmirImageError::ArtifactDigestMismatch {
                backend: "metal".into(),
                field: "content_sha256",
                stored: "aa".into(),
                computed: "bb".into(),
            },
            None,
            vec![
                ("backend", "metal"),
                ("field", "content_sha256"),
                ("stored", "aa"),
                ("computed", "bb"),
            ],
        ),
        (
            FmirImageError::CompilerInputProvenance {
                backend: "cuda".into(),
                detail: "no parent".into(),
            },
            None,
            vec![("backend", "cuda"), ("detail", "no parent")],
        ),
        (
            FmirImageError::NonAdmittedTargetFeature {
                feature: "warp9".into(),
            },
            None,
            vec![("feature", "warp9")],
        ),
        (
            FmirImageError::TargetIdMismatch {
                backend: "metal".into(),
                target: "sm_90".into(),
                expected: "apple".into(),
            },
            None,
            vec![
                ("backend", "metal"),
                ("target", "sm_90"),
                ("expected", "apple"),
            ],
        ),
        (
            FmirImageError::AbsentArtifactForSelection {
                selection: "cuda".into(),
            },
            None,
            vec![("selection", "cuda")],
        ),
        (
            FmirImageError::ArtifactEncodingMismatch {
                backend: "metal".into(),
                detail: "bom".into(),
            },
            None,
            vec![("backend", "metal"), ("detail", "bom")],
        ),
        (
            FmirImageError::NonAdmittedArtifactBackend {
                backend: "tpu".into(),
            },
            None,
            vec![("backend", "tpu")],
        ),
        (
            FmirImageError::ArtifactAbiVersionMismatch {
                backend: "metal".into(),
                actual: 1,
                accepted: 4,
            },
            Some("fmir_image_artifact_abi_version_unsupported"),
            vec![("backend", "metal"), ("actual", "1"), ("accepted", "4")],
        ),
        (
            FmirImageError::ArtifactReflectionVersionMismatch {
                backend: "metal".into(),
                actual: 5,
                expected: 6,
            },
            Some("fmir_image_artifact_reflection_version_unsupported"),
            vec![("backend", "metal"), ("actual", "5"), ("expected", "6")],
        ),
        (
            FmirImageError::IncompatibleArtifactCombination {
                backend: "metal".into(),
                detail: "mixed".into(),
            },
            Some("fmir_image_artifact_combination_incompatible"),
            vec![("backend", "metal"), ("detail", "mixed")],
        ),
        (
            FmirImageError::DuplicateRequiredFeature {
                feature: "simd".into(),
            },
            Some("fmir_image_artifact_duplicate_required_feature"),
            vec![("feature", "simd")],
        ),
        (
            FmirImageError::DuplicateEntrypoint { entry: "k0".into() },
            Some("fmir_image_artifact_duplicate_entrypoint"),
            vec![("entry", "k0")],
        ),
        (
            FmirImageError::NonAdmittedSelectedBackend {
                backend: "tpu".into(),
            },
            Some("fmir_image_selected_backend_unsupported"),
            vec![("backend", "tpu")],
        ),
        (
            FmirImageError::DeviceProgramVersionMismatch {
                actual: 3,
                expected: 8,
            },
            Some("fmir_image_device_program_version_unsupported"),
            vec![("actual", "3"), ("expected", "8")],
        ),
        (
            FmirImageError::WireProgramInvalid {
                detail: "truncated".into(),
            },
            Some("fmir_image_device_program_invalid"),
            vec![("detail", "truncated")],
        ),
        (
            FmirImageError::ExecutorPlanSectionVersionMismatch {
                actual: 1,
                expected: 2,
            },
            Some("fmir_image_executor_plan_section_version_unsupported"),
            vec![("actual", "1"), ("expected", "2")],
        ),
        (
            FmirImageError::ExecutorPlanSectionInvalid {
                detail: "gap".into(),
            },
            Some("fmir_image_executor_plan_section_invalid"),
            vec![("detail", "gap")],
        ),
        (
            FmirImageError::DistributedSectionVersionMismatch {
                actual: 4,
                expected: 5,
            },
            Some("fmir_image_distributed_section_version_unsupported"),
            vec![("actual", "4"), ("expected", "5")],
        ),
        (
            FmirImageError::DistributedSectionInvalid {
                detail: "rank".into(),
            },
            Some("fmir_image_distributed_section_invalid"),
            vec![("detail", "rank")],
        ),
        (
            FmirImageError::DistributedSectionEquivalenceMismatch {
                detail: "plan".into(),
            },
            Some("fmir_image_distributed_section_equivalence_mismatch"),
            vec![("detail", "plan")],
        ),
        (
            FmirImageError::DistributedSectionHashMismatch {
                expected: "e1".into(),
                recomputed: "r2".into(),
            },
            Some("fmir_image_distributed_section_hash_mismatch"),
            vec![("expected", "e1"), ("recomputed", "r2")],
        ),
        (
            FmirImageError::SessionSectionVersionMismatch {
                actual: 1,
                expected: 2,
            },
            Some("fmir_image_session_section_version_unsupported"),
            vec![("actual", "1"), ("expected", "2")],
        ),
        (
            FmirImageError::SessionSectionInvalid {
                detail: "cadence".into(),
            },
            Some("fmir_image_session_section_invalid"),
            vec![("detail", "cadence")],
        ),
    ];
    for (error, issue, args) in cases {
        let name = format!("{error:?}");
        let diagnostic = one(error, "fmir");
        assert_eq!(arg(&diagnostic, "issue"), issue, "{name}");
        assert_eq!(diagnostic.file, path().display().to_string(), "{name}");
        let expected: Vec<(&str, &str)> = args;
        let carried: Vec<(&str, &str)> = diagnostic
            .args
            .iter()
            .filter(|arg| arg.name != "issue")
            .map(|arg| (arg.name, arg.value.as_str()))
            .collect();
        assert_eq!(carried, expected, "{name}");
    }
}

#[test]
fn a_parse_failure_without_an_issue_code_is_a_plain_diagnostic() {
    let style = FmirImageErrorStyle {
        label: "fmir",
        parse_verb: "decode",
        parse_issue: None,
        version_issue: "v",
    };
    let diagnostics = fmir_image_decode_error(FmirImageError::Parse("eof".into()), &path(), &style);
    assert_eq!(arg(&diagnostics[0], "detail"), Some("eof"));
    assert_eq!(arg(&diagnostics[0], "issue"), None);
}

#[test]
fn the_format_labels_name_the_route() {
    assert_eq!(FmirPackageImageFormat::Source.label(), "source-built FMIR");
    assert_eq!(FmirPackageImageFormat::FmirText.label(), "fmir-text");
    assert_eq!(FmirPackageImageFormat::Fmir.label(), "fmir");
}

#[test]
fn a_text_image_loads_with_the_header_fields_and_empty_validation_metadata() {
    let text = text_image(
        FMIR_TEXT_TARGET_NAME,
        PACKAGE_MIR_ARTIFACT_VERSION,
        &empty_program_json(),
    );
    let image = load_fmir_text_image(&text, &path(), TOOLCHAIN).expect("loads");
    assert_eq!(image.format.label(), "fmir-text");
    assert_eq!(image.diagnostic_path, path());
    assert_eq!(image.entry_function, "run_entry");
    assert_eq!(image.runtime_requirements, vec!["io".to_owned()]);
    assert_eq!(image.interner, vec!["alpha".to_owned(), "beta".to_owned()]);
    assert_eq!(image.exit_code, Some(3));
    assert_eq!(image.source_hashes, vec!["00ff".to_owned()]);
    assert!(image.program.functions.is_empty());
    assert!(image.device.is_none());
    assert!(image.validation.struct_fields.is_empty());
}

#[test]
fn a_binary_image_loads_with_the_header_fields() {
    let image =
        header(FMIR_TARGET_NAME, PACKAGE_MIR_ARTIFACT_VERSION).into_binary_image(MirProgram::new());
    let bytes = encode_binary_image(&image).expect("encode");
    let loaded = load_fmir_image(&bytes, &path(), TOOLCHAIN).expect("loads");
    assert_eq!(loaded.format.label(), "fmir");
    assert_eq!(loaded.runtime_requirements, vec!["io".to_owned()]);
    assert_eq!(loaded.interner, vec!["alpha".to_owned(), "beta".to_owned()]);
    assert_eq!(loaded.exit_code, Some(3));
    assert_eq!(loaded.source_hashes, vec!["00ff".to_owned()]);
}

#[test]
fn the_text_loader_refuses_with_named_diagnostics() {
    let program = empty_program_json();
    let load = |text: &str| {
        load_fmir_text_image(text, &path(), TOOLCHAIN)
            .err()
            .expect("refused")
    };

    let garbage = load("not = [valid");
    assert_eq!(
        arg(&garbage[0], "issue"),
        Some("fmir_text_image_parse_failed")
    );
    assert!(arg(&garbage[0], "detail").is_some());

    let stale = load(&text_image(
        FMIR_TEXT_TARGET_NAME,
        PACKAGE_MIR_ARTIFACT_VERSION + 1,
        &program,
    ));
    assert_eq!(
        arg(&stale[0], "issue"),
        Some("fmir_text_image_version_unsupported")
    );
    assert_eq!(
        arg(&stale[0], "actual"),
        Some((PACKAGE_MIR_ARTIFACT_VERSION + 1).to_string().as_str())
    );

    let wrong_target = load(&text_image(
        FMIR_TARGET_NAME,
        PACKAGE_MIR_ARTIFACT_VERSION,
        &program,
    ));
    assert_eq!(arg(&wrong_target[0], "issue"), None);
    assert_eq!(arg(&wrong_target[0], "found"), Some(FMIR_TARGET_NAME));
    assert_eq!(
        arg(&wrong_target[0], "expected"),
        Some(FMIR_TEXT_TARGET_NAME)
    );

    let good = text_image(
        FMIR_TEXT_TARGET_NAME,
        PACKAGE_MIR_ARTIFACT_VERSION,
        &program,
    );
    let other_toolchain = load_fmir_text_image(&good, &path(), "0.0.1")
        .err()
        .expect("refused");
    assert_eq!(arg(&other_toolchain[0], "issue"), None);
    assert_eq!(arg(&other_toolchain[0], "found"), Some(TOOLCHAIN));
    assert_eq!(arg(&other_toolchain[0], "expected"), Some("0.0.1"));

    let bad_program = load(&text_image(
        FMIR_TEXT_TARGET_NAME,
        PACKAGE_MIR_ARTIFACT_VERSION,
        "{not json",
    ));
    assert_eq!(arg(&bad_program[0], "issue"), None);
    assert!(arg(&bad_program[0], "detail").is_some());
}

#[test]
fn the_binary_loader_refuses_a_stale_version_and_garbage() {
    let stale = header(FMIR_TARGET_NAME, PACKAGE_MIR_ARTIFACT_VERSION + 1)
        .into_binary_image(MirProgram::new());
    let bytes = encode_binary_image(&stale).expect("encode");
    let error = load_fmir_image(&bytes, &path(), TOOLCHAIN)
        .err()
        .expect("refused");
    assert_eq!(
        arg(&error[0], "issue"),
        Some("fmir_image_version_unsupported")
    );
    assert_eq!(
        arg(&error[0], "actual"),
        Some((PACKAGE_MIR_ARTIFACT_VERSION + 1).to_string().as_str())
    );

    let error = load_fmir_image(&[0xff, 0xff], &path(), TOOLCHAIN)
        .err()
        .expect("refused");
    assert!(arg(&error[0], "detail").is_some());
    assert_eq!(arg(&error[0], "issue"), None);
}

#[test]
fn validation_metadata_survives_apply_then_snapshot() {
    let (struct_def, enum_def, variant_def, other) = (DefId(1), DefId(2), DefId(3), DefId(4));
    let (field_a, field_b) = (Symbol(10), Symbol(11));
    let ty = MirType::semantic(TypeId(0));
    let environment = MirClosureEnvironment {
        id: MirClosureEnvironmentId(7),
        function: MirFunctionId(0),
        value_ty: ty,
        environment_local: MirLocalId(0),
        captures: Vec::new(),
    };
    let mut metadata = FmirValidationMetadata::default();
    metadata
        .struct_fields
        .insert(struct_def, HashMap::from([(field_a, ty)]));
    metadata
        .struct_field_order
        .insert(struct_def, vec![field_a, field_b]);
    metadata
        .variant_field_order
        .insert(variant_def, vec![field_b]);
    metadata
        .display_names
        .insert(struct_def, "Point".to_owned());
    metadata
        .struct_type_params
        .insert(struct_def, vec![field_a]);
    metadata
        .struct_contracts
        .insert(struct_def, HashSet::from([other]));
    metadata.enum_type_params.insert(enum_def, vec![field_b]);
    metadata
        .optional_struct_fields
        .insert(struct_def, HashSet::from([field_a]));
    metadata
        .nullable_struct_fields
        .insert(struct_def, HashSet::from([field_b]));
    metadata.opaque_structs.insert(other);
    metadata
        .json_struct_field_keys
        .insert(struct_def, HashMap::from([(field_a, "a-key".to_owned())]));
    metadata.variant_parents.insert(variant_def, enum_def);
    metadata.enum_variants.insert(enum_def, vec![variant_def]);
    metadata.variant_positions.insert(variant_def, 2);
    metadata
        .variant_fields
        .insert(variant_def, HashMap::from([(field_b, ty)]));
    metadata.closure_environments = vec![environment.clone()];
    metadata.ad_routes.insert("/x".to_owned(), struct_def);

    let types = radix_types::TypeTable::new();
    let mut context = radix_mir::MirValidationContext::new(&types);
    apply_validation_metadata(&mut context, &metadata);
    assert_eq!(
        context.display_names.get(&struct_def).map(String::as_str),
        Some("Point")
    );
    assert_eq!(context.variant_positions.get(&variant_def), Some(&2));

    let back = snapshot_validation_metadata(&context);
    assert_eq!(back.struct_fields, metadata.struct_fields);
    assert_eq!(back.struct_field_order, metadata.struct_field_order);
    assert_eq!(back.variant_field_order, metadata.variant_field_order);
    assert_eq!(back.display_names, metadata.display_names);
    assert_eq!(back.struct_type_params, metadata.struct_type_params);
    assert_eq!(back.struct_contracts, metadata.struct_contracts);
    assert_eq!(back.enum_type_params, metadata.enum_type_params);
    assert_eq!(back.optional_struct_fields, metadata.optional_struct_fields);
    assert_eq!(back.nullable_struct_fields, metadata.nullable_struct_fields);
    assert_eq!(back.opaque_structs, metadata.opaque_structs);
    assert_eq!(back.json_struct_field_keys, metadata.json_struct_field_keys);
    assert_eq!(back.variant_parents, metadata.variant_parents);
    assert_eq!(back.enum_variants, metadata.enum_variants);
    assert_eq!(back.variant_positions, metadata.variant_positions);
    assert_eq!(back.variant_fields, metadata.variant_fields);
    assert_eq!(back.closure_environments, vec![environment]);
    assert_eq!(back.ad_routes, metadata.ad_routes);
}
