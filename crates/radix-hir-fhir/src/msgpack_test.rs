use super::*;
use crate::test_support::stub_artifact;
use radix_lexer::Symbol;

/// Collects every float leaf in a value tree (§1.3 wire pin).
fn floats_in(value: &Value, out: &mut Vec<&'static str>) {
    match value {
        Value::F32(_) => out.push("f32"),
        Value::F64(_) => out.push("f64"),
        Value::Array(items) => {
            for item in items {
                floats_in(item, out);
            }
        }
        Value::Map(entries) => {
            for (key, val) in entries {
                floats_in(key, out);
                floats_in(val, out);
            }
        }
        _ => {}
    }
}

/// Finds the value under a chain of map keys / array indices.
fn get<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for step in path {
        match (current, step) {
            (Value::Map(entries), key) => {
                current = &entries
                    .iter()
                    .find(|(k, _)| matches!(k, Value::Str(s) if s == key))?
                    .1;
            }
            _ => return None,
        }
    }
    Some(current)
}

/// The unit's named top-level keys, in declaration order.
const UNIT_KEYS: [&str; 24] = [
    "schema_version",
    "shape_hash",
    "source_identity",
    "hir",
    "presentation",
    "cli_program",
    "types",
    "resolved_generic_calls",
    "radix_lanes",
    "interner",
    "resolver",
    "libraries",
    "function_facts",
    "resolved_uses",
    "analysis_stamp",
    "gpu_builtins",
    "capability_provenance",
    "provider_units",
    "shape_obligations",
    "contract_roles",
    "kernel_requirements",
    "tensor_edge_policies",
    "display_tokens",
    "conversation_constructor",
];

fn top_keys(value: &Value) -> Vec<&str> {
    let Value::Map(entries) = value else {
        panic!("unit value must be a map");
    };
    entries
        .iter()
        .map(|(k, _)| match k {
            Value::Str(s) => s.as_str(),
            other => panic!("non-str map key {other:?}"),
        })
        .collect()
}

fn parse(bytes: &[u8]) -> Value {
    Value::from_bytes(bytes).expect("encoded unit value must parse")
}

// ── Round-trip proofs mirroring the postcard proofs in artifact_test.rs ──

#[test]
fn stub_artifact_round_trips_through_msgpack() {
    let original = stub_artifact();
    let bytes = encode_msgpack(&original).expect("msgpack-encode HirArtifact");
    let decoded = decode_msgpack(&bytes).expect("msgpack-decode HirArtifact");
    assert_eq!(original.schema_version, SCHEMA_VERSION);
    assert_eq!(original, decoded);
}

/// N7E-1: the policy-float cell survives the `MessagePack` wire at the current
/// schema (the encode/decode pair validates the artifact on both sides).
#[test]
fn policy_float_cells_round_trip_through_msgpack() {
    use radix_types::{FloatPolicy, NumericWidth, Type, TypeTable};

    let mut table = TypeTable::new();
    let cell = table.intern(Type::PolicyFloat(
        FloatPolicy::Saturating,
        NumericWidth::F16,
    ));
    let mut original = stub_artifact();
    original.types = table.snapshot();

    let bytes = encode_msgpack(&original).expect("msgpack-encode HirArtifact");
    let decoded = decode_msgpack(&bytes).expect("msgpack-decode HirArtifact");
    assert_eq!(decoded.schema_version, SCHEMA_VERSION);
    assert_eq!(original, decoded);
    let restored = TypeTable::from_snapshot(decoded.types).expect("restore type table");
    assert_eq!(
        restored.get(cell),
        &Type::PolicyFloat(FloatPolicy::Saturating, NumericWidth::F16)
    );
}

#[test]
fn selective_import_round_trips_through_msgpack() {
    let original = crate::test_support::selective_import_artifact();
    let bytes = encode_msgpack(&original).expect("msgpack-encode selective HirArtifact");
    let decoded = decode_msgpack(&bytes).expect("msgpack-decode selective HirArtifact");
    assert_eq!(original, decoded);
    let radix_hir::HirItemKind::Import(import) = &decoded.hir.items[1].kind else {
        panic!("expected import item");
    };
    assert!(import.selective, "selective marker must survive the wire");
    assert_eq!(
        import.items.len(),
        2,
        "ordered member list must survive the wire"
    );
    assert_eq!(import.items[0].name, Symbol(3));
    assert_eq!(import.items[0].alias, Some(Symbol(4)));
    assert_eq!(import.items[1].alias, None);
}

#[test]
fn selective_import_msgpack_decode_passes_referential_integrity() {
    let artifact = crate::test_support::selective_import_artifact();
    let bytes = encode_msgpack(&artifact).expect("encode");
    let decoded = decode_msgpack(&bytes).expect("selective artifact must validate");
    let radix_hir::HirItemKind::Import(import) = &decoded.hir.items[1].kind else {
        panic!("expected import item");
    };
    assert!(import.selective);
}

#[test]
fn transfer_round_trips_through_msgpack() {
    let original = crate::test_support::transfer_artifact();
    let bytes = encode_msgpack(&original).expect("msgpack-encode transfer HirArtifact");
    let decoded = decode_msgpack(&bytes).expect("msgpack-decode transfer HirArtifact");
    assert_eq!(original, decoded);
    let radix_hir::HirItemKind::Function(function) = &decoded.hir.items[0].kind else {
        panic!("expected function item");
    };
    let body = function.body.as_ref().expect("body");
    let radix_hir::HirStatementKind::Expr(expr) = &body.statements[0].kind else {
        panic!("expected expr statement");
    };
    let radix_hir::HirExpressionKind::CopyInto { target, value } = &expr.kind else {
        panic!("expected copy-into expression");
    };
    assert!(
        matches!(target.kind, radix_hir::HirExpressionKind::Path(DefId(0))),
        "target must survive the wire"
    );
    assert!(
        matches!(value.kind, radix_hir::HirExpressionKind::Literal(_)),
        "value must survive the wire"
    );
}

#[test]
fn transfer_msgpack_decode_passes_referential_integrity() {
    let artifact = crate::test_support::transfer_artifact();
    let bytes = encode_msgpack(&artifact).expect("encode");
    let decoded = decode_msgpack(&bytes).expect("transfer artifact must validate");
    let radix_hir::HirItemKind::Function(function) = &decoded.hir.items[0].kind else {
        panic!("expected function item");
    };
    let body = function.body.as_ref().expect("body");
    assert!(
        matches!(
            &body.statements[0].kind,
            radix_hir::HirStatementKind::Expr(radix_hir::HirExpression {
                kind: radix_hir::HirExpressionKind::CopyInto { .. },
                ..
            })
        ),
        "transfer must survive the full msgpack decode/validate path"
    );
}

// ── Wire-shape pins (spec.md §1/§2 binding rules) ────────────────────────

#[test]
fn unit_wire_uses_the_named_keys_in_order() {
    // §1.1/§1.2: named fields, not compact arrays; schema_version first.
    let bytes = encode_msgpack(&stub_artifact()).expect("encode");
    let value = parse(&bytes);
    assert_eq!(top_keys(&value), UNIT_KEYS);

    // §2 row 1: version is a uint fact.
    assert_eq!(
        get(&value, &["schema_version"]),
        Some(&Value::Uint(u64::from(SCHEMA_VERSION)))
    );

    // §2 row 2: source identity map with its named keys.
    assert!(matches!(
        get(&value, &["source_identity"]),
        Some(Value::Map(entries)) if entries.len() == 2
    ));

    // §2 row 5: absent CLI contract is nil, not a missing key (§1.5).
    assert_eq!(get(&value, &["cli_program"]), Some(&Value::Nil));

    // §2 row 7: interner is an array of str in order.
    assert!(matches!(
        get(&value, &["interner"]),
        Some(Value::Array(items)) if items.len() == 2
    ));

    // §2 row 12: analysis stamp is a uint fact.
    assert_eq!(get(&value, &["analysis_stamp"]), Some(&Value::Uint(42)));
}

#[test]
fn tensor_edge_policy_facts_survive_named_msgpack_round_trip() {
    use crate::artifact::{
        OrdoOrderComparisonWire, ResolvedTensorEdgePolicyWire, TensorEdgePolicyCallWire,
        TensorEdgePolicyFunctionRequirementsWire, TensorEdgePolicyProviderImplWire,
        TensorEdgePolicyRequirementWire, TensorShiftContextWire,
    };
    use radix_hir::HirId;
    use radix_types::{BuiltinTensorEdgePolicy, DefId, IndexExpr, Primitive, Type, TypeTable};

    let mut artifact = stub_artifact();
    let mut types = TypeTable::new();
    let policy_type = types.intern(Type::Struct(DefId(0)));
    let element_type = types.primitive(Primitive::Fractus);
    let builtin_policy_type = types.tensor_edge_policy(BuiltinTensorEdgePolicy::Reflect);
    let source_shape = types.intern_index(IndexExpr::Literal(8));
    artifact.types = types.snapshot();
    artifact.tensor_edge_policies = crate::artifact::TensorEdgePolicyTableWire {
        calls: vec![TensorEdgePolicyCallWire {
            call: HirId(0),
            policy: ResolvedTensorEdgePolicyWire::Custom {
                policy_def: DefId(0),
                remap: DefId(0),
                transform: None,
            },
        }],
        shifted_expressions: vec![HirId(0)],
        shifted_view_expressions: vec![(
            HirId(0),
            TensorShiftContextWire {
                source_shape,
                shift_expression: HirId(0),
            },
        )],
        shifted_bindings: vec![DefId(0)],
        shifted_view_bindings: vec![(
            DefId(0),
            TensorShiftContextWire {
                source_shape,
                shift_expression: HirId(0),
            },
        )],
        limes_contexts: vec![(
            HirId(0),
            TensorShiftContextWire {
                source_shape,
                shift_expression: HirId(0),
            },
        )],
        function_requirements: vec![TensorEdgePolicyFunctionRequirementsWire {
            function: DefId(0),
            requirements: vec![TensorEdgePolicyRequirementWire {
                policy_type,
                element_type,
                rank: 2,
            }],
        }],
        provider_impls: vec![TensorEdgePolicyProviderImplWire {
            policy_type: builtin_policy_type,
            element_type,
            rank: 2,
            policy: ResolvedTensorEdgePolicyWire::Builtin(BuiltinTensorEdgePolicy::Reflect),
        }],
        ordo_order: vec![OrdoOrderComparisonWire {
            lhs: HirId(0),
            rows: vec![(DefId(0), 10), (DefId(0), -3), (DefId(0), i64::MIN)],
        }],
    };

    let bytes = encode_msgpack(&artifact).expect("encode populated policy table");
    let decoded = decode_msgpack(&bytes).expect("decode populated policy table");
    assert_eq!(decoded.tensor_edge_policies, artifact.tensor_edge_policies);
    assert_eq!(decoded.types, artifact.types);
}

#[test]
fn enums_encode_as_tag_maps() {
    // §1.4: { "tag": <variant name>, ...payload }.
    let bytes = encode_msgpack(&stub_artifact()).expect("encode");
    let value = parse(&bytes);

    // Newtype variant: tag + payload under "value".
    let item_kind = get(&value, &["hir", "items"])
        .and_then(|v| match v {
            Value::Array(items) => items.first(),
            _ => None,
        })
        .and_then(|item| get(item, &["kind"]))
        .expect("hir.items[0].kind");
    let keys = top_keys(item_kind);
    assert_eq!(keys, vec!["tag", "value"]);
    assert_eq!(
        get(item_kind, &["tag"]),
        Some(&Value::Str("Function".into()))
    );

    // Struct variant (CopyInto): payload fields inlined next to "tag".
    let transfer = crate::test_support::transfer_artifact();
    let value = parse(&encode_msgpack(&transfer).expect("encode"));
    let expr = get(&value, &["hir", "items"])
        .and_then(|v| match v {
            Value::Array(items) => items.first(),
            _ => None,
        })
        .and_then(|item| get(item, &["kind", "value", "body", "statements"]))
        .and_then(|v| match v {
            Value::Array(items) => items.first(),
            _ => None,
        })
        .and_then(|st| get(st, &["kind", "value", "kind"]))
        .expect("transfer expression kind");
    let keys = top_keys(expr);
    assert_eq!(keys, vec!["tag", "target", "value"]);
    assert_eq!(get(expr, &["tag"]), Some(&Value::Str("CopyInto".into())));
}

#[test]
fn tuple_families_carry_named_def_id_maps() {
    // §2 rows 10/13: (DefId, …) tuple families as { def_id, facts } /
    // { def_id, builtin } named maps, never compact arrays.
    let mut artifact = stub_artifact();
    artifact.function_facts = FunctionFactTableWire {
        entries: vec![(
            DefId(0),
            FunctionFactWire {
                has_direct_failure: false,
                may_fail: true,
                is_async: false,
                is_generator: false,
                requires_await: false,
                call_edges: vec![DefId(0)],
                captures: Vec::new(),
                param_modes: Vec::new(),
                err_ty: None,
            },
        )],
        entry: None,
    };
    artifact.gpu_builtins = vec![(DefId(0), GpuBuiltinWire::WorkgroupId)];
    let bytes = encode_msgpack(&artifact).expect("encode");
    let decoded = decode_msgpack(&bytes).expect("round-trip");
    assert_eq!(decoded, artifact);

    let value = parse(&bytes);
    let fact_entry = get(&value, &["function_facts", "entries"])
        .and_then(|v| match v {
            Value::Array(items) => items.first(),
            _ => None,
        })
        .expect("function_facts.entries[0]");
    assert_eq!(top_keys(fact_entry), vec!["def_id", "facts"]);
    assert_eq!(get(fact_entry, &["def_id"]), Some(&Value::Uint(0)));

    let gpu_entry = get(&value, &["gpu_builtins"])
        .and_then(|v| match v {
            Value::Array(items) => items.first(),
            _ => None,
        })
        .expect("gpu_builtins[0]");
    assert_eq!(top_keys(gpu_entry), vec!["def_id", "builtin"]);
    // Unit-variant builtin: a bare { "tag": … } map (§1.4).
    assert_eq!(
        get(gpu_entry, &["builtin", "tag"]),
        Some(&Value::Str("WorkgroupId".into()))
    );
}

#[test]
fn no_width_or_dtype_fact_rides_a_float_opcode() {
    // §1.3: the artifact fixtures carry no genuine float values, so the
    // entire wire tree must be free of f32/f64 opcodes — widths, ids,
    // dtype/shape facts are uint/int values under named keys.
    let fixtures = [
        stub_artifact(),
        crate::test_support::selective_import_artifact(),
        crate::test_support::transfer_artifact(),
    ];
    for artifact in &fixtures {
        let value = parse(&encode_msgpack(artifact).expect("encode"));
        let mut floats = Vec::new();
        floats_in(&value, &mut floats);
        assert!(floats.is_empty(), "float opcodes leaked: {floats:?}");
    }
}

#[test]
fn genuine_floats_and_bytes_keep_their_native_types() {
    // §1.5/§1.6: genuine IEEE floats stay float opcodes; byte payloads (the
    // serde `serialize_bytes` path) are bin; Some carries the bare payload,
    // None is nil. Pinned on a scratch serde type because the artifact
    // fixtures carry no floats or byte payloads.
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
    struct Scratch {
        ratio: f64,
        tiny: f32,
    }

    let scratch = Scratch {
        ratio: 0.25,
        tiny: 1.5,
    };
    let mut bytes = Vec::new();
    scratch
        .serialize(Serializer { out: &mut bytes })
        .expect("encode scratch");
    let value = parse(&bytes);
    assert_eq!(get(&value, &["ratio"]), Some(&Value::F64(0.25)));
    assert_eq!(get(&value, &["tiny"]), Some(&Value::F32(1.5)));
    let decoded = Scratch::deserialize(value).expect("decode scratch");
    assert_eq!(decoded, scratch);

    // Byte payloads ride the bin opcode on the serde bytes path.
    struct BytesWrap<'a>(&'a [u8]);
    impl Serialize for BytesWrap<'_> {
        fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            s.serialize_bytes(self.0)
        }
    }
    #[derive(PartialEq, Debug)]
    struct BinDto(Vec<u8>);
    impl<'de> serde::Deserialize<'de> for BinDto {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct V;
            impl serde::de::Visitor<'_> for V {
                type Value = BinDto;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("bin bytes")
                }
                fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E> {
                    Ok(BinDto(v))
                }
            }
            d.deserialize_byte_buf(V)
        }
    }
    let mut bin_bytes = Vec::new();
    BytesWrap(&[0, 1, 2])
        .serialize(Serializer {
            out: &mut bin_bytes,
        })
        .expect("encode bytes");
    let value = parse(&bin_bytes);
    assert_eq!(value, Value::Bin(vec![0, 1, 2]));
    let decoded = BinDto::deserialize(value).expect("decode bin");
    assert_eq!(decoded, BinDto(vec![0, 1, 2]));
}

#[test]
fn msgpack_decode_fails_closed_on_version_mismatch() {
    // §1.2/F3: mismatched version rejected before payload interpretation,
    // forward and backward.
    for wrong in [4u64, 5] {
        let bytes = encode_msgpack(&stub_artifact()).expect("encode");
        let mut value = parse(&bytes);
        if let Value::Map(entries) = &mut value {
            for (key, val) in entries {
                if matches!(key, Value::Str(s) if s == "schema_version") {
                    *val = Value::Uint(wrong);
                }
            }
        }
        let mut patched = Vec::new();
        write_value(&mut patched, &value).expect("re-encode patched value");
        match decode_msgpack(&patched) {
            Err(MsgpackError::UnsupportedSchemaVersion { found, expected }) => {
                assert_eq!(u64::from(found), wrong);
                assert_eq!(expected, SCHEMA_VERSION);
            }
            other => panic!("expected version rejection for {wrong}, got {other:?}"),
        }
    }
}

#[test]
fn msgpack_decode_rejects_truncated_and_malformed_input() {
    let bytes = encode_msgpack(&stub_artifact()).expect("encode");
    assert!(matches!(
        decode_msgpack(&bytes[..bytes.len() - 1]),
        Err(MsgpackError::Truncated | MsgpackError::Decode(_))
    ));
    // Not a map: fails before the version gate can even find a named key.
    assert!(matches!(
        decode_msgpack(&[0x90]),
        Err(MsgpackError::Decode(_))
    ));
}

// ── FPF-3: package codec (nested named unit values, OQ-3) ────────────────

use crate::package::{
    FhirPackage, FhirPackageModule, LibraryImportWire, LocalLinkWire, PACKAGE_SCHEMA_VERSION,
    PackageDependencyWire, PackageIdentityWire,
};

/// One step in a wire-tree walk: a map key or an array index.
#[derive(Clone, Copy)]
enum Step {
    Key(&'static str),
    Idx(usize),
}

fn at<'a>(value: &'a Value, steps: &[Step]) -> Option<&'a Value> {
    let mut current = value;
    for step in steps {
        current = match (current, step) {
            (Value::Map(entries), Step::Key(key)) => {
                &entries
                    .iter()
                    .find(|(k, _)| matches!(k, Value::Str(s) if s == key))?
                    .1
            }
            (Value::Array(items), Step::Idx(i)) => items.get(*i)?,
            _ => return None,
        };
    }
    Some(current)
}

fn at_mut<'a>(value: &'a mut Value, steps: &[Step]) -> Option<&'a mut Value> {
    let mut current = value;
    for step in steps {
        current = match (current, step) {
            (Value::Map(entries), Step::Key(key)) => {
                &mut entries
                    .iter_mut()
                    .find(|(k, _)| matches!(k, Value::Str(s) if s == key))?
                    .1
            }
            (Value::Array(items), Step::Idx(i)) => items.get_mut(*i)?,
            _ => return None,
        };
    }
    Some(current)
}

/// Re-encodes a patched wire tree back to bytes (gate-patching seam).
fn reserialize(value: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    write_value(&mut bytes, value).expect("re-encode patched value");
    bytes
}

/// Patches one named uint field on the wire tree and re-encodes.
fn patch_uint(bytes: &[u8], steps: &[Step], replacement: u64) -> Vec<u8> {
    let mut value = parse(bytes);
    *at_mut(&mut value, steps).expect("patch target must exist") = Value::Uint(replacement);
    reserialize(&value)
}

/// The 6 envelope named keys, in declaration order (version prefix first).
const PACKAGE_KEYS: [&str; 6] = [
    "package_schema_version",
    "identity",
    "entry_path",
    "entry_frontmatter",
    "modules",
    "dependencies",
];

/// The 9 module-record named keys, in declaration order.
const MODULE_KEYS: [&str; 9] = [
    "path",
    "module_segments",
    "is_entry",
    "export_names",
    "local_links",
    "library_imports",
    "source_hash",
    "unit_schema_version",
    "unit",
];

fn pkg_module(
    path: &str,
    is_entry: bool,
    artifact: &crate::artifact::HirArtifact,
) -> FhirPackageModule {
    let unit = crate::encode(artifact).expect("encode unit");
    FhirPackageModule {
        path: path.to_owned(),
        module_segments: path
            .trim_start_matches("src/")
            .trim_end_matches(".fab")
            .split('/')
            .map(str::to_owned)
            .collect(),
        is_entry,
        export_names: vec!["public_fn".to_owned()],
        local_links: vec![],
        library_imports: vec![],
        source_hash: artifact.source_identity.content_hash.clone(),
        unit_schema_version: SCHEMA_VERSION,
        unit,
    }
}

/// A multi-module package: three modules sorted by path, the entry module
/// linking to both siblings (multi-module link table), a library import on
/// `aux`, frontmatter, and a sorted two-entry dependency table. Three
/// distinct artifact fixtures ride as embedded units.
fn multi_module_package() -> FhirPackage {
    let mut aux = pkg_module(
        "src/aux.fab",
        false,
        &crate::test_support::selective_import_artifact(),
    );
    aux.library_imports = vec![LibraryImportWire {
        binding: "norma".to_owned(),
        package: "norma".to_owned(),
        module: vec!["math".to_owned()],
    }];
    let mut math = pkg_module(
        "src/lib/math.fab",
        false,
        &crate::test_support::transfer_artifact(),
    );
    math.export_names = vec!["add".to_owned(), "sub".to_owned()];
    let mut main = pkg_module("src/main.fab", true, &stub_artifact());
    main.local_links = vec![
        LocalLinkWire {
            binding: "aux".to_owned(),
            target: "src/aux.fab".to_owned(),
        },
        LocalLinkWire {
            binding: "math".to_owned(),
            target: "src/lib/math.fab".to_owned(),
        },
    ];
    FhirPackage {
        package_schema_version: PACKAGE_SCHEMA_VERSION,
        identity: PackageIdentityWire {
            name: "exemplum".to_owned(),
            version: "0.1.0".to_owned(),
            edition: "2026".to_owned(),
        },
        entry_path: "src/main.fab".to_owned(),
        entry_frontmatter: Some("[entry]\nname = \"exemplum\"\n".to_owned()),
        modules: vec![aux, math, main],
        dependencies: vec![
            PackageDependencyWire {
                name: "norma".to_owned(),
                version: "1.0.0".to_owned(),
                lock_identity: "path:/tmp/norma".to_owned(),
                checksum: Some("cafe01".to_owned()),
            },
            PackageDependencyWire {
                name: "triga".to_owned(),
                version: "0.9.0".to_owned(),
                lock_identity: "path:/tmp/triga".to_owned(),
                checksum: None,
            },
        ],
    }
}

#[test]
fn multi_module_package_round_trips_through_msgpack() {
    let original = multi_module_package();
    let bytes = encode_package_msgpack(&original).expect("msgpack-encode FhirPackage");
    let decoded = decode_package_msgpack(&bytes).expect("msgpack-decode FhirPackage");
    assert_eq!(decoded.package_schema_version, PACKAGE_SCHEMA_VERSION);
    assert_eq!(
        decoded, original,
        "structural equality vs pre-encode package"
    );
    // Link tables and the dependency table survive the wire.
    assert_eq!(decoded.modules[2].local_links.len(), 2);
    assert_eq!(decoded.modules[0].library_imports.len(), 1);
    assert_eq!(decoded.dependencies.len(), 2);
}

#[test]
fn package_msgpack_encode_is_deterministic() {
    let pkg = multi_module_package();
    assert_eq!(
        encode_package_msgpack(&pkg).expect("encode"),
        encode_package_msgpack(&pkg).expect("encode again")
    );
}

#[test]
fn package_msgpack_wire_uses_named_keys_and_nested_unit_values() {
    // OQ-3: embedded units are nested named values — the whole package is
    // one MessagePack document; no opaque nested bytes on the wire.
    let bytes = encode_package_msgpack(&multi_module_package()).expect("encode");
    let value = parse(&bytes);
    assert_eq!(top_keys(&value), PACKAGE_KEYS);
    // FPF-5 ratchet: the live envelope prefix is the v2 MessagePack value.
    assert_eq!(
        get(&value, &["package_schema_version"]),
        Some(&Value::Uint(u64::from(PACKAGE_SCHEMA_VERSION)))
    );

    // modules[0] is src/aux.fab; its record carries the named module keys.
    let aux = at(&value, &[Step::Key("modules"), Step::Idx(0)]).expect("modules[0]");
    assert_eq!(top_keys(aux), MODULE_KEYS);
    assert_eq!(
        get(aux, &["unit_schema_version"]),
        Some(&Value::Uint(u64::from(SCHEMA_VERSION)))
    );

    // The unit is a nested named map with the unit keys — not bin.
    let unit = get(aux, &["unit"]).expect("nested unit");
    assert!(
        matches!(unit, Value::Map(_)),
        "unit must be a nested named map"
    );
    assert_eq!(top_keys(unit), UNIT_KEYS);
    // Link table entries are named maps too (binding → target).
    let main = at(&value, &[Step::Key("modules"), Step::Idx(2)]).expect("modules[2]");
    let link = get(main, &["local_links"])
        .and_then(|v| match v {
            Value::Array(items) => items.first(),
            _ => None,
        })
        .expect("local_links[0]");
    assert_eq!(top_keys(link), vec!["binding", "target"]);
}

#[test]
fn package_msgpack_rejects_unsupported_package_schema_version() {
    // Envelope gate: read before payload interpretation, fail closed. v1 is
    // the retired postcard envelope version (rejected as a plain version
    // mismatch here because the payload is a genuine map value; bare postcard
    // bytes get the named legacy rejection instead).
    let bytes = encode_package_msgpack(&multi_module_package()).expect("encode");
    for wrong in [0u64, 1, 99] {
        let patched = patch_uint(&bytes, &[Step::Key("package_schema_version")], wrong);
        match decode_package_msgpack(&patched) {
            Err(MsgpackError::UnsupportedPackageSchemaVersion { found, expected }) => {
                assert_eq!(u64::from(found), wrong);
                assert_eq!(expected, PACKAGE_SCHEMA_VERSION);
            }
            other => panic!("expected package version rejection for {wrong}, got {other:?}"),
        }
    }
}

#[test]
fn package_msgpack_rejects_unsupported_manifest_unit_version() {
    let bytes = encode_package_msgpack(&multi_module_package()).expect("encode");
    let patched = patch_uint(
        &bytes,
        &[
            Step::Key("modules"),
            Step::Idx(0),
            Step::Key("unit_schema_version"),
        ],
        99,
    );
    match decode_package_msgpack(&patched) {
        Err(MsgpackError::InvalidPackage(FhirPackageError::UnsupportedUnitSchemaVersion {
            module,
            found,
            expected,
        })) => {
            assert_eq!(module, "src/aux.fab");
            assert_eq!(found, 99);
            assert_eq!(expected, SCHEMA_VERSION);
        }
        other => panic!("expected manifest unit version rejection, got {other:?}"),
    }
}

#[test]
fn package_msgpack_rejects_a_foreign_unit_wire_shape() {
    let bytes = encode_package_msgpack(&multi_module_package()).expect("encode");
    let patched = patch_uint(
        &bytes,
        &[
            Step::Key("modules"),
            Step::Idx(1),
            Step::Key("unit"),
            Step::Key("shape_hash"),
        ],
        wire_shape_hash() ^ 1,
    );
    match decode_package_msgpack(&patched) {
        Err(MsgpackError::InvalidPackage(FhirPackageError::UnsupportedUnitWireShape {
            module,
            found,
            expected,
        })) => {
            assert_eq!(module, "src/lib/math.fab");
            assert_eq!(found, wire_shape_hash() ^ 1);
            assert_eq!(expected, wire_shape_hash());
        }
        other => panic!("expected unit wire-shape rejection, got {other:?}"),
    }
}

#[test]
fn msgpack_unit_decode_rejects_a_foreign_wire_shape() {
    let mut artifact = stub_artifact();
    artifact.shape_hash = wire_shape_hash() ^ 1;
    let bytes = encode_msgpack(&artifact).expect("encode");
    assert!(matches!(
        decode_msgpack(&bytes),
        Err(MsgpackError::UnsupportedWireShape { .. })
    ));
}

#[test]
fn package_msgpack_rejects_unsupported_nested_unit_version() {
    // The nested unit's own schema_version gate fails closed with module
    // attribution even when the manifest copy still says 5.
    let bytes = encode_package_msgpack(&multi_module_package()).expect("encode");
    let patched = patch_uint(
        &bytes,
        &[
            Step::Key("modules"),
            Step::Idx(1),
            Step::Key("unit"),
            Step::Key("schema_version"),
        ],
        4,
    );
    match decode_package_msgpack(&patched) {
        Err(MsgpackError::InvalidPackage(FhirPackageError::UnsupportedUnitSchemaVersion {
            module,
            found,
            expected,
        })) => {
            assert_eq!(module, "src/lib/math.fab");
            assert_eq!(found, 4);
            assert_eq!(expected, SCHEMA_VERSION);
        }
        other => panic!("expected nested unit version rejection, got {other:?}"),
    }
}

#[test]
fn package_msgpack_rejects_truncated_and_malformed_input() {
    let bytes = encode_package_msgpack(&multi_module_package()).expect("encode");
    assert!(matches!(
        decode_package_msgpack(&bytes[..bytes.len() - 1]),
        Err(MsgpackError::Truncated | MsgpackError::Decode(_))
    ));
    assert!(matches!(
        decode_package_msgpack(&[0x90]),
        Err(MsgpackError::Decode(_))
    ));
}

#[test]
fn package_msgpack_rejects_dangling_local_link() {
    let mut pkg = multi_module_package();
    pkg.modules[2].local_links.push(LocalLinkWire {
        binding: "ghost".to_owned(),
        target: "src/absent.fab".to_owned(),
    });
    // Keep the link table canonically sorted so the failure is the dangling
    // target, not the sortedness gate.
    pkg.modules[2]
        .local_links
        .sort_by(|a, b| a.binding.cmp(&b.binding));
    let bytes = encode_package_msgpack(&pkg).expect("encode");
    assert!(matches!(
        decode_package_msgpack(&bytes),
        Err(MsgpackError::InvalidPackage(
            FhirPackageError::DanglingModuleRef { from, to }
        )) if from == "src/main.fab" && to == "src/absent.fab"
    ));
}

#[test]
fn package_msgpack_rejects_missing_entry() {
    let mut pkg = multi_module_package();
    pkg.entry_path = "src/absent.fab".to_owned();
    let bytes = encode_package_msgpack(&pkg).expect("encode");
    assert!(matches!(
        decode_package_msgpack(&bytes),
        Err(MsgpackError::InvalidPackage(FhirPackageError::MissingEntry {
            entry
        })) if entry == "src/absent.fab"
    ));
}

#[test]
fn package_msgpack_rejects_entry_flag_inconsistent_with_entry_path() {
    let mut pkg = multi_module_package();
    pkg.modules[2].is_entry = false;
    pkg.modules[0].is_entry = true;
    let bytes = encode_package_msgpack(&pkg).expect("encode");
    assert!(matches!(
        decode_package_msgpack(&bytes),
        Err(MsgpackError::InvalidPackage(
            FhirPackageError::MissingEntry { .. }
        ))
    ));
}

#[test]
fn package_msgpack_rejects_duplicate_module_paths() {
    let mut pkg = multi_module_package();
    // Duplicate of the last module: adjacent equal paths hit the duplicate
    // branch (not merely the sortedness branch).
    let clone = pkg.modules[2].clone();
    pkg.modules.push(clone);
    let bytes = encode_package_msgpack(&pkg).expect("encode");
    assert!(matches!(
        decode_package_msgpack(&bytes),
        Err(MsgpackError::InvalidPackage(
            FhirPackageError::DuplicateModulePath { path }
        )) if path == "src/main.fab"
    ));
}

#[test]
fn package_msgpack_rejects_duplicate_dependencies() {
    let mut pkg = multi_module_package();
    let dup = pkg.dependencies[0].clone();
    pkg.dependencies.push(dup);
    // Keep the table canonically sorted so the failure is the duplicate
    // name, not the sortedness gate.
    pkg.dependencies.sort_by(|a, b| a.name.cmp(&b.name));
    let bytes = encode_package_msgpack(&pkg).expect("encode");
    assert!(matches!(
        decode_package_msgpack(&bytes),
        Err(MsgpackError::InvalidPackage(
            FhirPackageError::MissingDependencyArtifact { name, .. }
        )) if name == "norma"
    ));
}

#[test]
fn package_msgpack_rejects_mismatched_content_hash() {
    let mut pkg = multi_module_package();
    pkg.modules[0].source_hash = "deadbeef".to_owned();
    let bytes = encode_package_msgpack(&pkg).expect("encode");
    assert!(matches!(
        decode_package_msgpack(&bytes),
        Err(MsgpackError::InvalidPackage(
            FhirPackageError::MismatchedContentHash { module, .. }
        )) if module == "src/aux.fab"
    ));
}

#[test]
fn package_msgpack_rejects_unit_failing_referential_integrity() {
    // Emptying the interner on the nested unit must fail the unit's
    // referential-integrity gate with module attribution — never a panic.
    let bytes = encode_package_msgpack(&multi_module_package()).expect("encode");
    let mut value = parse(&bytes);
    *at_mut(
        &mut value,
        &[
            Step::Key("modules"),
            Step::Idx(0),
            Step::Key("unit"),
            Step::Key("interner"),
        ],
    )
    .expect("interner must exist") = Value::Array(Vec::new());
    match decode_package_msgpack(&reserialize(&value)) {
        Err(MsgpackError::InvalidPackage(FhirPackageError::MalformedUnit { module, .. })) => {
            assert_eq!(module, "src/aux.fab");
        }
        other => panic!("expected MalformedUnit, got {other:?}"),
    }
}

#[test]
fn package_msgpack_encode_fails_closed_on_undecodable_unit() {
    let mut pkg = multi_module_package();
    pkg.modules[0].unit = vec![0xde, 0xad, 0xbe, 0xef];
    assert!(
        encode_package_msgpack(&pkg).is_err(),
        "a module whose postcard unit cannot decode must not be embeddable"
    );
}

#[test]
fn package_msgpack_is_the_live_wire_and_postcard_is_rejected_legacy() {
    // FPF-5: `.fhirpkg` carries the v2 MessagePack envelope. The v1
    // postcard envelope still decodes through the legacy codec (equivalence
    // and migration reads), but the MessagePack decoder — the only load
    // path — rejects it with the named legacy error; no fallback decode.
    let mut pkg = multi_module_package();
    let msgpack_bytes = encode_package_msgpack(&pkg).expect("msgpack encode");
    pkg.package_schema_version = crate::package::LEGACY_PACKAGE_SCHEMA_VERSION;
    let legacy_bytes = crate::package::encode_package(&pkg).expect("postcard encode");
    assert_ne!(msgpack_bytes, legacy_bytes);

    let via_msgpack = decode_package_msgpack(&msgpack_bytes).expect("msgpack decode");
    let via_legacy = crate::package::decode_package(&legacy_bytes).expect("legacy postcard decode");
    assert_eq!(via_msgpack.identity, via_legacy.identity);
    assert_eq!(via_msgpack.entry_path, via_legacy.entry_path);
    assert_eq!(via_msgpack.modules.len(), via_legacy.modules.len());

    match decode_package_msgpack(&legacy_bytes) {
        Err(MsgpackError::InvalidPackage(FhirPackageError::LegacyPostcardPackage {
            found,
            expected,
        })) => {
            assert_eq!(found, crate::package::LEGACY_PACKAGE_SCHEMA_VERSION);
            assert_eq!(expected, PACKAGE_SCHEMA_VERSION);
        }
        other => panic!("expected the named legacy rejection, got {other:?}"),
    }
}

#[test]
fn package_msgpack_rejects_bare_postcard_version_prefix() {
    // OQ-1: a postcard envelope's u32 version varint (a bare uint prefix)
    // can never be a v2+ map value — the version ratchet is the
    // discriminator, no magic bytes. A v1-varint-prefixed payload is
    // rejected as legacy even when the trailing bytes are garbage.
    let legacy = decode_package_msgpack(&[0x01, 0xff, 0xfe, 0xfd]);
    assert!(
        matches!(
            legacy,
            Err(MsgpackError::InvalidPackage(
                FhirPackageError::LegacyPostcardPackage {
                    found: 1,
                    expected: 2
                }
            ))
        ),
        "postcard-shaped bytes must fail closed as legacy, got {legacy:?}"
    );
    // A single complete uint with no trailing bytes is not a postcard
    // prefix: it takes the ordinary malformed-path (not a map value).
    assert!(matches!(
        decode_package_msgpack(&[0x01]),
        Err(MsgpackError::Decode(_))
    ));
}

#[test]
fn legacy_gate_decides_from_the_first_marker_without_a_document_walk() {
    use super::legacy_postcard_version;
    // v2+ top-level map markers are never legacy, at any width.
    for marker in [0x80, 0x8f, 0xde, 0xdf] {
        assert_eq!(
            legacy_postcard_version(&[marker, 0x00, 0x01, 0xaa]),
            None,
            "map marker {marker:#x} must not be read as a legacy version"
        );
    }
    // Bare uint prefixes with trailing payload are legacy: fixint and the
    // explicit uint8/uint16/uint32 width classes.
    assert_eq!(legacy_postcard_version(&[0x01, 0xff, 0xfe, 0xfd]), Some(1));
    assert_eq!(legacy_postcard_version(&[0xcc, 0x05, 0xaa]), Some(5));
    assert_eq!(legacy_postcard_version(&[0xcd, 0x00, 0x01, 0xaa]), Some(1));
    assert_eq!(
        legacy_postcard_version(&[0xce, 0x00, 0x00, 0x00, 0x02, 0xaa]),
        Some(2)
    );
    // Truncated uint payloads are malformed, not legacy.
    for bytes in [&[][..], &[0xcc][..], &[0xcd, 0x00][..], &[0xcf, 0x00][..]] {
        assert_eq!(legacy_postcard_version(bytes), None);
    }
    // A complete uint alone carries no envelope prefix.
    for bytes in [&[0x01][..], &[0xcc, 0x05][..], &[0xcd, 0x00, 0x01][..]] {
        assert_eq!(legacy_postcard_version(bytes), None);
    }
    // Negative fixints and a uint64 beyond u32 are not a legacy version.
    assert_eq!(legacy_postcard_version(&[0xe0, 0x01]), None);
    assert_eq!(
        legacy_postcard_version(&[0xcf, 0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xaa]),
        None
    );
}

// ---------------------------------------------------------------------------
// Codec-level marker pins (FPF-2-PIN)
//
// Pins the length-marker selection and write_uint width-class behavior from
// FPF-2 (467866aaf) directly at the primitive level: exact marker bytes on
// both sides of each width boundary, encode → decode round-trip where a full
// value exists. These would fail if the minimal-width selection or the
// uint/int marker classes regressed.
// ---------------------------------------------------------------------------

/// Encodes a Value and asserts the exact leading bytes, then decodes back.
fn assert_wire(value: &Value, prefix: &[u8], label: &str) {
    let mut out = Vec::new();
    write_value(&mut out, value).expect("encode");
    assert!(
        out.starts_with(prefix),
        "{label}: expected prefix {prefix:02x?}, got {out:02x?}"
    );
    // Round-trip: the full document decodes back to the same value.
    let back = Value::from_bytes(&out).expect("decode");
    assert_eq!(&back, value, "{label}: round-trip mismatch");
}

#[test]
fn write_uint_width_classes_pin_the_467866aaf_boundaries() {
    // Each value class selects exactly one marker; the boundary values are
    // the ones FPF-2's minimal-width selection moved between.
    assert_wire(&Value::Uint(0x7f), &[0x7f], "positive fixint top 0x7f");
    assert_wire(&Value::Uint(0x80), &[0xcc, 0x80], "uint8 bottom 128");
    assert_wire(&Value::Uint(0xff), &[0xcc, 0xff], "uint8 top 255");
    assert_wire(
        &Value::Uint(0x100),
        &[0xcd, 0x01, 0x00],
        "uint16 bottom 256",
    );
    assert_wire(
        &Value::Uint(0xffff),
        &[0xcd, 0xff, 0xff],
        "uint16 top 65535",
    );
    assert_wire(
        &Value::Uint(0x1_0000),
        &[0xce, 0x00, 0x01, 0x00, 0x00],
        "uint32 bottom 65536",
    );
    assert_wire(
        &Value::Uint(u64::from(u32::MAX)),
        &[0xce, 0xff, 0xff, 0xff, 0xff],
        "uint32 top 4294967295",
    );
    assert_wire(
        &Value::Uint(0x1_0000_0000),
        &[0xcf, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00],
        "uint64 bottom 4294967296",
    );
    assert_wire(
        &Value::Uint(u64::MAX),
        &[0xcf, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
        "uint64 top u64::MAX",
    );
    // Zero is a positive fixint, never a wider uint marker.
    assert_wire(&Value::Uint(0), &[0x00], "zero is fixint");
}

#[test]
fn str_length_markers_pin_fixstr32_str8_256_str16_65536_boundaries() {
    assert_wire(&Value::Str("a".repeat(31)), &[0xbf], "fixstr top 31");
    assert_wire(&Value::Str("a".repeat(32)), &[0xd9, 0x20], "str8 bottom 32");
    assert_wire(&Value::Str("a".repeat(0xff)), &[0xd9, 0xff], "str8 top 255");
    assert_wire(
        &Value::Str("a".repeat(0x100)),
        &[0xda, 0x01, 0x00],
        "str16 bottom 256",
    );
    assert_wire(
        &Value::Str("a".repeat(0xffff)),
        &[0xda, 0xff, 0xff],
        "str16 top 65535",
    );
    assert_wire(
        &Value::Str("a".repeat(0x1_0000)),
        &[0xdb, 0x00, 0x01, 0x00, 0x00],
        "str32 bottom 65536",
    );
}

#[test]
fn bin_length_markers_pin_bin8_256_bin16_65536_boundaries() {
    assert_wire(&Value::Bin(vec![0u8; 0xff]), &[0xc4, 0xff], "bin8 top 255");
    assert_wire(
        &Value::Bin(vec![0u8; 0x100]),
        &[0xc5, 0x01, 0x00],
        "bin16 bottom 256",
    );
    assert_wire(
        &Value::Bin(vec![0u8; 0xffff]),
        &[0xc5, 0xff, 0xff],
        "bin16 top 65535",
    );
    assert_wire(
        &Value::Bin(vec![0u8; 0x1_0000]),
        &[0xc6, 0x00, 0x01, 0x00, 0x00],
        "bin32 bottom 65536",
    );
}

#[test]
fn array_length_markers_pin_fixarray16_array16_65536_boundaries() {
    let nils = |n: usize| Value::Array(vec![Value::Nil; n]);
    assert_wire(&nils(15), &[0x9f], "fixarray top 15");
    assert_wire(&nils(16), &[0xdc, 0x00, 0x10], "array16 bottom 16");
    assert_wire(&nils(0xffff), &[0xdc, 0xff, 0xff], "array16 top 65535");
    assert_wire(
        &nils(0x1_0000),
        &[0xdd, 0x00, 0x01, 0x00, 0x00],
        "array32 bottom 65536",
    );
}

#[test]
fn map_length_markers_pin_fixmap16_map16_65536_boundaries() {
    let nilmap = |n: usize| Value::Map((0..n).map(|_| (Value::Nil, Value::Nil)).collect());
    assert_wire(&nilmap(15), &[0x8f], "fixmap top 15");
    assert_wire(&nilmap(16), &[0xde, 0x00, 0x10], "map16 bottom 16");
    assert_wire(&nilmap(0xffff), &[0xde, 0xff, 0xff], "map16 top 65535");
    assert_wire(
        &nilmap(0x1_0000),
        &[0xdf, 0x00, 0x01, 0x00, 0x00],
        "map32 bottom 65536",
    );
}

#[test]
fn int_width_markers_pin_negative_fixint_and_signed_classes() {
    // write_uint's sibling: signed opcodes are a wire fact, never the uint
    // classes, and the -32..-1 negative-fixint window overlaps uint space.
    assert_wire(&Value::Int(-32), &[0xe0], "nfix bottom -32");
    assert_wire(&Value::Int(-1), &[0xff], "nfix top -1");
    assert_wire(&Value::Int(-33), &[0xd0, 0xdf], "int8 bottom -33");
    assert_wire(&Value::Int(-129), &[0xd1, 0xff, 0x7f], "int16 bottom -129");
    assert_wire(
        &Value::Int(-32769),
        &[0xd2, 0xff, 0xff, 0x7f, 0xff],
        "int32 bottom -32769",
    );
    assert_wire(
        &Value::Int(i64::MIN),
        &[0xd3, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        "int64 bottom i64::MIN",
    );
}

#[test]
fn msgpack_rejects_dangling_resolved_use_local() {
    // U2.3 / 8dcb8a8f: `ResolvedUseWire.local` must clear the DefId gate — a
    // unit-local id absent from the decoded arena fails closed instead of
    // silently counting an import as a resolved use.
    let mut artifact = stub_artifact();
    artifact
        .resolved_uses
        .push(crate::artifact::ResolvedUseWire {
            key: crate::artifact::ResolvedUseKeyWire::Portable("pkg::item".to_string()),
            kind: crate::artifact::ResolvedUseKindWire::Path,
            enclosing: None,
            local: Some(DefId(999_999)),
        });
    let bytes = encode_msgpack(&artifact).expect("encode artifact with dangling local");
    match decode_msgpack(&bytes) {
        Err(MsgpackError::Invalid(HirArtifactError::DanglingIndex { kind: "DefId", .. })) => {}
        other => panic!("expected dangling DefId rejection, got {other:?}"),
    }
}

#[test]
fn msgpack_resolved_use_local_round_trips() {
    // A Portable path row with a live unit-local DefId and an ImportBinding
    // row without one both survive the codec and the integrity gate.
    let mut artifact = stub_artifact();
    artifact
        .resolved_uses
        .push(crate::artifact::ResolvedUseWire {
            key: crate::artifact::ResolvedUseKeyWire::Portable("pkg::used".to_string()),
            kind: crate::artifact::ResolvedUseKindWire::Path,
            enclosing: None,
            local: Some(DefId(0)),
        });
    artifact
        .resolved_uses
        .push(crate::artifact::ResolvedUseWire {
            key: crate::artifact::ResolvedUseKeyWire::Portable("pkg::imported".to_string()),
            kind: crate::artifact::ResolvedUseKindWire::ImportBinding,
            enclosing: None,
            local: None,
        });
    let bytes = encode_msgpack(&artifact).expect("encode artifact with resolved-use locals");
    let decoded = decode_msgpack(&bytes).expect("valid resolved-use locals must round-trip");
    assert_eq!(decoded.resolved_uses, artifact.resolved_uses);
}

/// Serializes a value through the codec's serde `Serializer` and reads it
/// back through the `Value` tree, the way every wire type travels.
fn serde_round_trip<T>(value: &T) -> (Vec<u8>, T)
where
    T: Serialize + for<'de> Deserialize<'de>,
{
    let mut out = Vec::new();
    value
        .serialize(Serializer { out: &mut out })
        .expect("serialize");
    let back = T::deserialize(Value::from_bytes(&out).expect("parse")).expect("deserialize");
    (out, back)
}

#[test]
fn msgpack_i128_extension_pins_its_wire_form() {
    // `fixext 16` (0xd8), extension type 1, then 16 big-endian two's
    // complement bytes, whatever the magnitude.
    let mut zero = vec![0xd8, 0x01];
    zero.extend_from_slice(&[0u8; 16]);
    assert_wire(&Value::Int128(0), &zero, "i128 zero is still the extension");
    let mut minus_one = vec![0xd8, 0x01];
    minus_one.extend_from_slice(&[0xffu8; 16]);
    assert_wire(&Value::Int128(-1), &minus_one, "i128 -1 is all ones");
}

#[test]
fn msgpack_i128_round_trips_the_full_range_and_the_i64_boundary() {
    let values = [
        i128::MIN,
        i128::MIN + 1,
        i128::from(i64::MIN) - 1,
        i128::from(i64::MIN),
        -1,
        0,
        1,
        i128::from(i64::MAX),
        i128::from(i64::MAX) + 1,
        i128::from(u64::MAX),
        i128::from(u64::MAX) + 1,
        i128::MAX - 1,
        i128::MAX,
    ];
    for value in values {
        assert_wire(&Value::Int128(value), &[0xd8, 0x01], "i128 value");
        let (bytes, back) = serde_round_trip(&value);
        assert_eq!(bytes.len(), 18, "{value}: one fixext 16 value");
        assert_eq!(back, value, "{value}: serde round trip");
    }
}

#[test]
fn msgpack_i128_wire_facts_round_trip() {
    // The artifact's own i128 carriers: a constant range bound and the extent
    // condition's two bounds.
    let range = crate::artifact::KernelRangeExprWire::Add(
        Box::new(crate::artifact::KernelRangeExprWire::Constant(i128::MIN)),
        Box::new(crate::artifact::KernelRangeExprWire::Constant(i128::MAX)),
    );
    let (_, back) = serde_round_trip(&range);
    assert_eq!(back, range);

    let condition = crate::artifact::KernelExtentConditionWire {
        span: radix_lexer::Span::default(),
        range: crate::artifact::KernelValueRangeWire {
            minimum: crate::artifact::KernelRangeExprWire::Constant(i128::from(i64::MIN) - 1),
            maximum: crate::artifact::KernelRangeExprWire::Constant(i128::from(i64::MAX) + 1),
        },
        minimum: i128::MIN,
        maximum: i128::MAX,
    };
    let (_, back) = serde_round_trip(&condition);
    assert_eq!(back, condition);
}

#[test]
fn msgpack_i128_decode_fails_closed_on_other_shapes() {
    // An `i128` field reads only the extension: a plain int is not one.
    let mut plain = Vec::new();
    write_int(&mut plain, 7);
    let err = i128::deserialize(Value::from_bytes(&plain).expect("parse"))
        .expect_err("a plain int is not an i128");
    assert!(matches!(err, MsgpackError::Decode(_)), "{err:?}");

    // Any other extension type is refused at parse time.
    let mut other = vec![0xd8, 0x02];
    other.extend_from_slice(&[0u8; 16]);
    assert!(matches!(
        Value::from_bytes(&other),
        Err(MsgpackError::Decode(_))
    ));

    // A short payload is truncation, not a narrower value.
    let mut short = vec![0xd8, 0x01];
    short.extend_from_slice(&[0u8; 15]);
    assert!(matches!(
        Value::from_bytes(&short),
        Err(MsgpackError::Truncated)
    ));
}

fn remove_named_field(value: &mut Value, field: &str) -> bool {
    match value {
        Value::Map(entries) => {
            if let Some(index) = entries
                .iter()
                .position(|(key, _)| matches!(key, Value::Str(key) if key == field))
            {
                entries.remove(index);
                return true;
            }
            entries
                .iter_mut()
                .any(|(_, child)| remove_named_field(child, field))
        }
        Value::Array(items) => items
            .iter_mut()
            .any(|child| remove_named_field(child, field)),
        _ => false,
    }
}

#[test]
fn missing_store_contract_fields_are_not_silently_defaulted() {
    use radix_hir::{
        HirBlock, HirExpression, HirExpressionKind, HirId, HirItemKind, HirLiteral, HirLocal,
        HirStatement, HirStatementKind, HirStoreKind, HirStoreSite,
    };
    use radix_lexer::Span;
    let mut artifact = stub_artifact();
    let store = HirStoreSite {
        kind: HirStoreKind::AwaitBinding,
        slot: Some(Symbol(0)),
        inferred_from: None,
        inferred_text: None,
    };
    let expression = HirExpression {
        id: HirId(2),
        kind: HirExpressionKind::Conversio {
            source: Box::new(HirExpression {
                id: HirId(3),
                kind: HirExpressionKind::Literal(HirLiteral::Int(300)),
                ty: None,
                span: Span::default(),
            }),
            target: radix_hir::HirConversioTarget::Type(radix_types::TypeId(0)),
            params: Vec::new(),
            recovery: None,
            route: None,
            implicit: Some(store.clone()),
        },
        ty: None,
        span: Span::default(),
    };
    let HirItemKind::Function(function) = &mut artifact.hir.items[0].kind else {
        panic!("function")
    };
    function.body = Some(HirBlock::simple(
        vec![HirStatement {
            id: HirId(1),
            kind: HirStatementKind::Local(HirLocal {
                def_id: DefId(1),
                name: Symbol(0),
                ty: None,
                init: Some(expression),
                mutable: false,
                is_await: false,
                runtime_provided: false,
                await_store: Some(store),
                size_decl: false,
                size_static: false,
            }),
            span: Span::default(),
        }],
        None,
        Span::default(),
    ));
    let bytes = encode_msgpack(&artifact).expect("encode");
    assert_eq!(
        decode_msgpack(&bytes).expect("explicit fields load"),
        artifact
    );
    for field in ["implicit", "route", "await_store"] {
        let mut value = parse(&bytes);
        assert!(remove_named_field(&mut value, field));
        let error = decode_msgpack(&reserialize(&value)).expect_err("required field");
        assert!(
            matches!(&error, MsgpackError::Decode(detail) if detail.contains(field)),
            "{error:?}"
        );
    }
}

#[test]
fn tuple_lengths_and_document_consumption_are_exact() {
    let bytes = encode_msgpack(&stub_artifact()).unwrap();
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(matches!(
        decode_msgpack(&trailing),
        Err(MsgpackError::Decode(_))
    ));

    // Ordinary tuples, tuple structs, and enum tuple variants all use exact arity.
    for len in [1, 3] {
        let value = Value::Array(vec![Value::Uint(0); len]);
        assert!(matches!(
            <(u32, u32)>::deserialize(value),
            Err(MsgpackError::Decode(_))
        ));
    }
    #[derive(Serialize, Deserialize)]
    struct Pair(u32, u32);
    assert!(matches!(
        Pair::deserialize(Value::Array(vec![Value::Uint(0); 3])),
        Err(MsgpackError::Decode(_))
    ));

    let mut value = parse(&bytes);
    // An Applied type has a two-element tuple payload. A third element must
    // not disappear when serde's visitor has consumed its expected fields.
    let types = at_mut(&mut value, &[Step::Key("types"), Step::Key("types")]).unwrap();
    let Value::Array(types) = types else {
        panic!("types arena")
    };
    types.push(Value::Map(vec![
        (Value::Str("tag".into()), Value::Str("Applied".into())),
        (
            Value::Str("value".into()),
            Value::Array(vec![Value::Uint(0), Value::Array(Vec::new()), Value::Nil]),
        ),
    ]));
    assert!(matches!(
        decode_msgpack(&reserialize(&value)),
        Err(MsgpackError::Decode(_))
    ));
}
