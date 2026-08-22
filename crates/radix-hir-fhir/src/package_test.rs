use super::*;
use crate::test_support::stub_artifact;

fn stub_module(path: &str, is_entry: bool) -> FhirPackageModule {
    let artifact = stub_artifact();
    let unit = crate::encode(&artifact).expect("encode stub unit");
    FhirPackageModule {
        path: path.to_owned(),
        module_segments: vec![path.to_owned()],
        is_entry,
        export_names: vec![],
        local_links: vec![],
        library_imports: vec![],
        source_hash: artifact.source_identity.content_hash.clone(),
        unit_schema_version: SCHEMA_VERSION,
        unit,
    }
}

fn stub_package() -> FhirPackage {
    FhirPackage {
        package_schema_version: PACKAGE_SCHEMA_VERSION,
        identity: PackageIdentityWire {
            name: "exemplum".to_owned(),
            version: "1.0.0".to_owned(),
            edition: "2026".to_owned(),
        },
        entry_path: "src/main.fab".to_owned(),
        entry_frontmatter: None,
        modules: vec![stub_module("src/main.fab", true)],
        dependencies: vec![PackageDependencyWire {
            name: "norma".to_owned(),
            version: "1.0.0".to_owned(),
            lock_identity: "path:/tmp/norma".to_owned(),
            checksum: None,
        }],
    }
}

fn encode(pkg: &FhirPackage) -> Vec<u8> {
    encode_package(pkg).expect("encode package")
}

#[test]
fn package_round_trip_preserves_package() {
    let pkg = stub_package();
    let decoded = decode_package(&encode(&pkg)).expect("decode package");
    assert_eq!(decoded.identity, pkg.identity);
    assert_eq!(decoded.entry_path, pkg.entry_path);
    assert_eq!(decoded.modules.len(), 1);
    assert_eq!(decoded.modules[0].path, "src/main.fab");
    assert!(decoded.modules[0].is_entry);
    assert_eq!(decoded.modules[0].unit.schema_version, SCHEMA_VERSION);
    assert_eq!(decoded.dependencies, pkg.dependencies);
}

#[test]
fn package_repeated_encode_is_byte_identical() {
    let pkg = stub_package();
    assert_eq!(encode(&pkg), encode(&pkg));
}

#[test]
fn package_rejects_garbage_bytes_fail_closed() {
    // Valid package-version prefix (1) followed by garbage: the envelope
    // postcard decode must fail closed (Malformed or Truncated), never
    // panic, and never return a package.
    let result = decode_package(b"\x01\x00\x00\x00\xff\xfe\xfd\x02");
    assert!(
        matches!(
            result,
            Err(FhirPackageError::Malformed(_) | FhirPackageError::Truncated)
        ),
        "garbage after valid version prefix must fail closed, got {result:?}"
    );
}

#[test]
fn package_rejects_truncated_payload() {
    let bytes = encode(&stub_package());
    let truncated = &bytes[..bytes.len() / 2];
    assert!(
        matches!(decode_package(truncated), Err(FhirPackageError::Truncated)),
        "mid-payload truncation must be classified as Truncated"
    );
}

#[test]
fn package_rejects_unsupported_package_schema_version() {
    let mut pkg = stub_package();
    pkg.package_schema_version = 99;
    match decode_package(&encode(&pkg)) {
        Err(FhirPackageError::UnsupportedPackageSchemaVersion {
            found: 99,
            expected: 1,
        }) => {}
        other => panic!("expected UnsupportedPackageSchemaVersion(99), got {other:?}"),
    }
}

#[test]
fn package_rejects_unsupported_unit_schema_version() {
    let mut pkg = stub_package();
    pkg.modules[0].unit_schema_version = 99;
    match decode_package(&encode(&pkg)) {
        Err(FhirPackageError::UnsupportedUnitSchemaVersion {
            module,
            found: 99,
            expected: SCHEMA_VERSION,
        }) => assert_eq!(module, "src/main.fab"),
        other => panic!("expected UnsupportedUnitSchemaVersion(99), got {other:?}"),
    }
}

#[test]
fn package_rejects_malformed_unit() {
    let mut pkg = stub_package();
    pkg.modules[0].unit = vec![0xde, 0xad, 0xbe, 0xef];
    assert!(matches!(
        decode_package(&encode(&pkg)),
        Err(FhirPackageError::MalformedUnit { module, .. }) if module == "src/main.fab"
    ));
}

#[test]
fn package_rejects_duplicate_module_paths() {
    let mut pkg = stub_package();
    pkg.modules.push(stub_module("src/main.fab", false));
    assert!(matches!(
        decode_package(&encode(&pkg)),
        Err(FhirPackageError::DuplicateModulePath { path }) if path == "src/main.fab"
    ));
}

#[test]
fn package_rejects_unsorted_module_table() {
    let mut pkg = stub_package();
    pkg.modules = vec![
        stub_module("src/zeta.fab", false),
        stub_module("src/main.fab", true),
    ];
    assert!(matches!(
        decode_package(&encode(&pkg)),
        Err(FhirPackageError::Unsorted { what }) if what == "module table"
    ));
}

#[test]
fn package_rejects_missing_entry() {
    let mut pkg = stub_package();
    pkg.entry_path = "src/absent.fab".to_owned();
    match decode_package(&encode(&pkg)) {
        Err(FhirPackageError::MissingEntry { entry }) => {
            assert_eq!(entry, "src/absent.fab");
        }
        other => panic!("expected MissingEntry, got {other:?}"),
    }
}

#[test]
fn package_rejects_entry_flag_inconsistent_with_entry_path() {
    let mut pkg = stub_package();
    pkg.modules[0].is_entry = false;
    assert!(matches!(
        decode_package(&encode(&pkg)),
        Err(FhirPackageError::MissingEntry { .. })
    ));
}

#[test]
fn package_rejects_mismatched_content_hash() {
    let mut pkg = stub_package();
    pkg.modules[0].source_hash = "deadbeef".to_owned();
    assert!(matches!(
        decode_package(&encode(&pkg)),
        Err(FhirPackageError::MismatchedContentHash { module, .. })
            if module == "src/main.fab"
    ));
}

#[test]
fn package_rejects_duplicate_dependencies() {
    let mut pkg = stub_package();
    pkg.dependencies.push(PackageDependencyWire {
        name: "norma".to_owned(),
        version: "2.0.0".to_owned(),
        lock_identity: "path:/tmp/norma2".to_owned(),
        checksum: None,
    });
    assert!(matches!(
        decode_package(&encode(&pkg)),
        Err(FhirPackageError::MissingDependencyArtifact { name, .. }) if name == "norma"
    ));
}

#[test]
fn package_rejects_unsorted_dependencies() {
    let mut pkg = stub_package();
    pkg.dependencies.push(PackageDependencyWire {
        name: "triga".to_owned(),
        version: "1.0.0".to_owned(),
        lock_identity: "path:/tmp/triga".to_owned(),
        checksum: None,
    });
    pkg.dependencies.reverse();
    assert!(matches!(
        decode_package(&encode(&pkg)),
        Err(FhirPackageError::Unsorted { what }) if what == "dependencies"
    ));
}

#[test]
fn package_rejects_dangling_local_link() {
    let mut pkg = stub_package();
    pkg.modules[0].local_links = vec![LocalLinkWire {
        binding: "ut".to_owned(),
        target: "src/absent.fab".to_owned(),
    }];
    match decode_package(&encode(&pkg)) {
        Err(FhirPackageError::DanglingModuleRef { from, to }) => {
            assert_eq!(from, "src/main.fab");
            assert_eq!(to, "src/absent.fab");
        }
        other => panic!("expected DanglingModuleRef, got {other:?}"),
    }
}

#[test]
fn package_rejects_duplicate_local_link_binding() {
    let mut pkg = stub_package();
    pkg.modules = vec![
        stub_module("src/aux.fab", false),
        stub_module("src/main.fab", true),
    ];
    pkg.modules[1].local_links = vec![
        LocalLinkWire {
            binding: "ut".to_owned(),
            target: "src/aux.fab".to_owned(),
        },
        LocalLinkWire {
            binding: "ut".to_owned(),
            target: "src/aux.fab".to_owned(),
        },
    ];
    assert!(matches!(
        decode_package(&encode(&pkg)),
        Err(FhirPackageError::DanglingModuleRef { .. })
    ));
}

#[test]
fn package_accepts_valid_local_link() {
    let mut pkg = stub_package();
    pkg.modules = vec![
        stub_module("src/aux.fab", false),
        stub_module("src/main.fab", true),
    ];
    pkg.modules[1].local_links = vec![LocalLinkWire {
        binding: "ut".to_owned(),
        target: "src/aux.fab".to_owned(),
    }];
    let decoded = decode_package(&encode(&pkg)).expect("valid link decodes");
    assert_eq!(decoded.modules[1].local_links.len(), 1);
}

#[test]
fn package_unit_failing_referential_integrity_is_rejected() {
    let artifact = stub_artifact();
    let mut unit = crate::encode(&artifact).expect("encode stub unit");
    // Flip a byte deep inside the payload — unit decode must fail closed
    // rather than panic.
    let mid = unit.len() / 2;
    unit[mid] ^= 0xff;
    let mut pkg = stub_package();
    pkg.modules[0].unit = unit;
    assert!(
        decode_package(&encode(&pkg)).is_err(),
        "tampered unit must be rejected"
    );
}
