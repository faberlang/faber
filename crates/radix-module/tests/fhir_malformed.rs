#![cfg(feature = "hir-fhir")]

//! Corrupt real analyzed units at the public loader boundary, before arena rebuild.

use radix_hir_fhir::{HirArtifact, HirArtifactError, ProviderCarriageEntryWire, ProviderUnitWire};
use radix_module::driver::{AnalyzeRequest, Config, Session, analyze_source};
use radix_module::hir::{artifact::load_fhir, serialize::from_analyzed};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn artifact() -> HirArtifact {
    let pack =
        radix_module::locale::LocalePack::from_toml_path(&radix_module::locale::workspace_locale_pack_path("en"))
            .expect("English pack");
    let session = Session::new(Config::default().with_locale_pack(pack));
    let source = r#"
class Cell { mut int value }
fn make<T, size N>(list<T> xs) → tensor<T, [N]> ⇥ string {
    return xs ↦ tensor<T, [N]>
}
fn total(tensor<int, [4]> cells) → int { return 0 }
fn extent(int n) → int { return n }
main {
    let list<int> data ← [5, 6, 7, 8]
    let tensor<int, [4]> row ← make<int, size 4>(data)
    print total(row)
    let int n ← extent(4)
    let tensor<int, [size n]> dynamic ← data ↦ tensor<int, [size n]>
    print total(dynamic)
    let int wide ← 300
    let u8 narrow ← wide
    print narrow
}
"#;
    let unit =
        analyze_source(&session, "corrupt.fab", source, AnalyzeRequest::default()).expect("valid analyzed fixture");
    let artifact = from_analyzed(&unit, "corrupt-test", "corrupt.fab");
    load_fhir(&postcard::to_allocvec(&artifact).unwrap()).expect("unmodified unit must load");
    artifact
}

fn refuse(artifact: &HirArtifact) -> HirArtifactError {
    load_fhir(&postcard::to_allocvec(artifact).unwrap())
        .err()
        .expect("corruption must be refused")
}

fn patch_first(value: &mut Value, key: &str, replacement: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if let Some(field) = map.get_mut(key) {
                *field = replacement.clone();
                return true;
            }
            map.values_mut()
                .any(|child| patch_first(child, key, replacement))
        }
        Value::Array(items) => items
            .iter_mut()
            .any(|child| patch_first(child, key, replacement)),
        _ => false,
    }
}

fn mutate(artifact: &HirArtifact, f: impl FnOnce(&mut Value)) -> HirArtifact {
    let mut value = serde_json::to_value(artifact).unwrap();
    f(&mut value);
    serde_json::from_value(value).unwrap()
}

#[test]
fn self_referential_applied_and_two_entry_cycle_are_refused() {
    let original = artifact();
    let value = serde_json::to_value(&original).unwrap();
    let types = value["types"]["types"].as_array().unwrap();
    let index = types
        .iter()
        .position(|ty| ty.get("Struct").is_some())
        .expect("nominal type");
    let corrupt = mutate(&original, |value| {
        value["types"]["types"][index] = json!({"Applied": [index, []]});
    });
    assert!(matches!(
        refuse(&corrupt),
        HirArtifactError::MalformedSnapshot { kind: "types", .. }
    ));
    let corrupt = mutate(&original, |value| {
        let types = value["types"]["types"].as_array_mut().unwrap();
        let next = types.len();
        types[index] = json!({"Applied": [next, []]});
        types.push(json!({"Applied": [index, []]}));
    });
    assert!(matches!(
        refuse(&corrupt),
        HirArtifactError::MalformedSnapshot { kind: "types", .. }
    ));
}

#[test]
fn undeclared_conversion_route_and_store_symbol_are_refused() {
    let original = artifact();
    let corrupt = mutate(&original, |value| {
        assert!(patch_first(
            &mut value["hir"],
            "route",
            &json!({"Function": {"def_id": 123456789, "failable": false}})
        ));
    });
    assert!(matches!(
        refuse(&corrupt),
        HirArtifactError::DanglingIndex { kind: "DefId", .. }
    ));
    let corrupt = mutate(&original, |value| {
        assert!(patch_first(&mut value["hir"], "slot", &json!(u32::MAX)));
    });
    assert!(matches!(
        refuse(&corrupt),
        HirArtifactError::DanglingIndex { kind: "Symbol", .. }
    ));
}

#[test]
fn duplicate_shape_call_cannot_erase_guard_relations() {
    let original = artifact();
    assert!(
        original
            .shape_obligations
            .calls
            .iter()
            .any(|row| !row.relations.is_empty()),
        "fixture needs retained guards"
    );
    let corrupt = mutate(&original, |value| {
        let calls = value["shape_obligations"]["calls"].as_array_mut().unwrap();
        let mut duplicate = calls
            .iter()
            .find(|row| !row["relations"].as_array().unwrap().is_empty())
            .unwrap()
            .clone();
        duplicate["relations"] = json!([]);
        calls.push(duplicate);
    });
    assert!(matches!(
        refuse(&corrupt),
        HirArtifactError::MalformedSnapshot { kind: "shape obligations", .. }
    ));
}

#[test]
fn duplicate_interner_row_and_symbol_referring_to_it_are_refused() {
    let mut corrupt = artifact();
    let duplicate = corrupt.interner.len() as u32;
    corrupt.interner.push(corrupt.interner[0].clone());
    corrupt.hir.entry_args_name = Some(radix_lexer::Symbol(duplicate));
    assert!(matches!(
        refuse(&corrupt),
        HirArtifactError::MalformedSnapshot { kind: "interner", .. }
    ));
}

fn provider(artifact: &HirArtifact) -> ProviderUnitWire {
    ProviderUnitWire {
        hir: artifact.hir.clone(),
        types: artifact.types.clone(),
        resolved_generic_calls: artifact.resolved_generic_calls.clone(),
        interner: artifact.interner.clone(),
        resolver: artifact.resolver.clone(),
        tensor_edge_policies: artifact.tensor_edge_policies.clone(),
        shape_obligations: artifact.shape_obligations.clone(),
        nested: BTreeMap::new(),
    }
}

#[test]
fn nested_provider_symbols_and_interners_are_validated() {
    let mut artifact = artifact();
    let good = provider(&artifact);
    artifact
        .provider_units
        .insert("outer".into(), ProviderCarriageEntryWire::Unit(Box::new(good.clone())));
    load_fhir(&postcard::to_allocvec(&artifact).unwrap()).expect("valid provider");
    for duplicate in [false, true] {
        let mut bad = good.clone();
        if duplicate {
            bad.interner.push(bad.interner[0].clone());
        } else {
            bad.hir.entry_args_name = Some(radix_lexer::Symbol(u32::MAX));
        }
        let mut outer = good.clone();
        outer
            .nested
            .insert("inner".into(), ProviderCarriageEntryWire::Unit(Box::new(bad)));
        artifact
            .provider_units
            .insert("outer".into(), ProviderCarriageEntryWire::Unit(Box::new(outer)));
        if duplicate {
            assert!(matches!(
                refuse(&artifact),
                HirArtifactError::MalformedSnapshot { kind: "interner", .. }
            ));
        } else {
            assert!(matches!(
                refuse(&artifact),
                HirArtifactError::DanglingIndex { kind: "Symbol", .. }
            ));
        }
    }
}

#[test]
fn trailing_postcard_bytes_are_refused() {
    let original = artifact();
    let mut bytes = postcard::to_allocvec(&original).unwrap();
    bytes.extend_from_slice(&[0, 0xff]);
    assert!(matches!(
        load_fhir(&bytes),
        Err(HirArtifactError::MalformedSnapshot { kind: "artifact", .. })
    ));
}

#[test]
fn named_package_cannot_drop_implicit_store_contract() {
    use radix_hir_fhir::{
        FhirPackage, FhirPackageModule, MsgpackError, PACKAGE_SCHEMA_VERSION, PackageIdentityWire, SCHEMA_VERSION,
        decode_package_msgpack, encode_package_msgpack,
    };
    let original = artifact();
    let package = FhirPackage {
        package_schema_version: PACKAGE_SCHEMA_VERSION,
        identity: PackageIdentityWire { name: "checked".into(), version: "1.0.0".into(), edition: "2026".into() },
        entry_path: "main.fab".into(),
        entry_frontmatter: None,
        modules: vec![FhirPackageModule {
            path: "main.fab".into(),
            module_segments: vec!["main".into()],
            is_entry: true,
            export_names: Vec::new(),
            local_links: Vec::new(),
            library_imports: Vec::new(),
            source_hash: original.source_identity.content_hash.clone(),
            unit_schema_version: SCHEMA_VERSION,
            unit: postcard::to_allocvec(&original).unwrap(),
        }],
        dependencies: Vec::new(),
    };
    let bytes = encode_package_msgpack(&package).unwrap();
    radix_module::hir::package::load_package(&bytes).expect("valid named package");
    // Rename only the fixstr map key to an unknown key of equal width. Serde
    // ignores the unknown key, but must refuse the now-missing required fact.
    // No document framing or field value is changed by this byte mutation.
    for field in ["implicit", "route", "await_store"] {
        let mut key = vec![0xa0 | u8::try_from(field.len()).unwrap()];
        key.extend_from_slice(field.as_bytes());
        let positions: Vec<_> = bytes
            .windows(key.len())
            .enumerate()
            .filter_map(|(i, window)| (window == key).then_some(i))
            .collect();
        assert!(!positions.is_empty(), "fixture carries {field}");
        let mut corrupt = bytes.clone();
        for position in positions {
            corrupt[position + 1] = b'!';
        }
        assert!(
            matches!(decode_package_msgpack(&corrupt), Err(MsgpackError::Decode(detail)) if detail.contains(field))
        );
        assert!(radix_module::hir::package::load_package(&corrupt).is_err());
    }
}
