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

/// The 13 spec §2 named top-level keys, in declaration order.
const UNIT_KEYS: [&str; 13] = [
    "schema_version",
    "source_identity",
    "hir",
    "presentation",
    "cli_program",
    "types",
    "interner",
    "resolver",
    "libraries",
    "function_facts",
    "resolved_uses",
    "analysis_stamp",
    "gpu_builtins",
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

#[test]
fn selective_import_round_trips_through_msgpack() {
    let original = crate::artifact::tests::selective_import_artifact();
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
    let artifact = crate::artifact::tests::selective_import_artifact();
    let bytes = encode_msgpack(&artifact).expect("encode");
    let decoded = decode_msgpack(&bytes).expect("selective artifact must validate");
    let radix_hir::HirItemKind::Import(import) = &decoded.hir.items[1].kind else {
        panic!("expected import item");
    };
    assert!(import.selective);
}

#[test]
fn transfer_round_trips_through_msgpack() {
    let original = crate::artifact::tests::transfer_artifact();
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
    let radix_hir::HirExpressionKind::Transfer { sink, payloads } = &expr.kind else {
        panic!("expected transfer expression");
    };
    assert!(
        matches!(sink.kind, radix_hir::HirExpressionKind::Path(DefId(0))),
        "sink must survive the wire"
    );
    assert_eq!(
        payloads.len(),
        2,
        "ordered payload list must survive the wire"
    );
}

#[test]
fn transfer_msgpack_decode_passes_referential_integrity() {
    let artifact = crate::artifact::tests::transfer_artifact();
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
                kind: radix_hir::HirExpressionKind::Transfer { .. },
                ..
            })
        ),
        "transfer must survive the full msgpack decode/validate path"
    );
}

// ── Wire-shape pins (spec.md §1/§2 binding rules) ────────────────────────

#[test]
fn unit_wire_uses_the_thirteen_named_keys_in_order() {
    // §1.1/§1.2: named fields, not compact arrays; schema_version first.
    let bytes = encode_msgpack(&stub_artifact()).expect("encode");
    let value = parse(&bytes);
    assert_eq!(top_keys(&value), UNIT_KEYS);

    // §2 row 1: version is a uint fact.
    assert_eq!(get(&value, &["schema_version"]), Some(&Value::Uint(5)));

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
    assert_eq!(get(item_kind, &["tag"]), Some(&Value::Str("Function".into())));

    // Struct variant (Transfer): payload fields inlined next to "tag".
    let transfer = crate::artifact::tests::transfer_artifact();
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
    assert_eq!(keys, vec!["tag", "sink", "payloads"]);
    assert_eq!(get(expr, &["tag"]), Some(&Value::Str("Transfer".into())));
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
        crate::artifact::tests::selective_import_artifact(),
        crate::artifact::tests::transfer_artifact(),
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
            impl<'de> serde::de::Visitor<'de> for V {
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
    for wrong in [4u64, 6] {
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
