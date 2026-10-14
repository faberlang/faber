//! Tests for the Radix-owned ABI contract payload and renderers.

use crate::abi::{AbiPayload, COMPILER_NAME, COMPILER_VERSION, host_abi};
use crate::abi::{render_json, render_rust};

#[test]
fn payload_reports_compiler_metadata_and_version() {
    let payload = AbiPayload::current();
    assert_eq!(payload.compiler.name, COMPILER_NAME);
    assert_eq!(payload.compiler.version, COMPILER_VERSION);
    assert_eq!(payload.abi_version, host_abi::ABI_VERSION);
    assert_eq!(payload.abi_version, 1);
    assert_eq!(payload.surface, host_abi::ABI_SURFACE_NAME);
}

#[test]
fn payload_symbol_rows_match_const_declarations() {
    // The macro produces both the individual consts and the paired row table
    // from one declaration list; assert they stay 1:1 by spot-checking that
    // every const value is present in the rows and the count matches.
    let payload = AbiPayload::current();
    assert_eq!(
        payload.symbols.len(),
        host_abi::SYMBOL_ROWS.len() + host_abi::GRADIENT_SYMBOL_ROWS.len(),
    );

    let row_for = |name: &str| {
        host_abi::SYMBOL_ROWS
            .iter()
            .chain(host_abi::GRADIENT_SYMBOL_ROWS)
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, value)| *value)
    };
    assert_eq!(row_for("SYMBOL_INIT"), Some(host_abi::SYMBOL_INIT));
    assert_eq!(row_for("SYMBOL_TEXT_CONCAT"), Some(host_abi::SYMBOL_TEXT_CONCAT));
    assert_eq!(row_for("SYMBOL_PROGRAM_ENTRY"), Some(host_abi::SYMBOL_PROGRAM_ENTRY));
    assert_eq!(row_for("SYMBOL_PROGRAM_ENTRY"), Some("__faber_program_entry_v1"));
    assert_eq!(row_for("SYMBOL_GRADIENT_CREATE"), Some(host_abi::SYMBOL_GRADIENT_CREATE));
    assert_eq!(
        row_for("SYMBOL_GRADIENT_ACCUMULATE"),
        Some(host_abi::SYMBOL_GRADIENT_ACCUMULATE)
    );
    assert_eq!(row_for("SYMBOL_GRADIENT_READ"), Some(host_abi::SYMBOL_GRADIENT_READ));
    assert_eq!(row_for("SYMBOL_GRADIENT_ZERO"), Some(host_abi::SYMBOL_GRADIENT_ZERO));
    assert!(row_for("SYMBOL_DOES_NOT_EXIST").is_none());

    // Every payload row resolves to the same value as the named const.
    for row in &payload.symbols {
        let const_value = row_for(row.name).expect("payload row has no matching const");
        assert_eq!(row.value, const_value, "mismatch for {}", row.name);
    }
}

#[test]
fn payload_enum_rows_match_const_declarations() {
    let payload = AbiPayload::current();
    let value_kind = payload
        .enums
        .iter()
        .find(|family| family.family == "value_kind")
        .expect("value_kind family present");
    assert_eq!(value_kind.type_alias, "FaberRtValueKindV1");
    assert_eq!(value_kind.repr, "u32");
    assert_eq!(value_kind.rows.len(), host_abi::VALUE_KIND_ROWS.len());

    let row_for = |name: &str| {
        host_abi::VALUE_KIND_ROWS
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, value)| *value)
    };
    assert_eq!(row_for("VALUE_KIND_I64"), Some(host_abi::VALUE_KIND_I64));
    assert_eq!(row_for("VALUE_KIND_VALOR"), Some(host_abi::VALUE_KIND_VALOR));
    assert_eq!(row_for("VALUE_KIND_ASCII"), Some(host_abi::VALUE_KIND_ASCII));
}

#[test]
fn payload_reports_genus_field_policy_family() {
    // The genus field-layout contract is part of the reported surface: the
    // LLVM emitter writes its codes and `render_rust` emits its type alias
    // and constants, so `radix abi` must enumerate it.
    let payload = AbiPayload::current();
    let family = payload
        .enums
        .iter()
        .find(|family| family.family == "genus_field_policy")
        .expect("genus_field_policy family is reported by the ABI surface");
    assert_eq!(family.type_alias, "FaberRtGenusFieldPolicyV1");
    assert_eq!(family.repr, "u8");
    assert_eq!(family.rows.len(), host_abi::GENUS_FIELD_POLICY_ROWS.len());

    let row_for = |name: &str| {
        host_abi::GENUS_FIELD_POLICY_ROWS
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, value)| *value)
    };
    assert_eq!(
        row_for("GENUS_FIELD_POLICY_MANDATORY"),
        Some(u32::from(host_abi::GENUS_FIELD_POLICY_MANDATORY))
    );
    assert_eq!(
        row_for("GENUS_FIELD_POLICY_DEFAULTABLE"),
        Some(u32::from(host_abi::GENUS_FIELD_POLICY_DEFAULTABLE))
    );
}

#[test]
fn fingerprint_is_deterministic_and_stable() {
    let one = AbiPayload::current()
        .fingerprint()
        .expect("fingerprint one");
    let two = AbiPayload::current()
        .fingerprint()
        .expect("fingerprint two");
    assert_eq!(one, two, "fingerprint must be stable across builds of the same payload");
    assert_eq!(one.len(), 16, "fingerprint is a 64-bit FxHash in hex");
    assert!(one.chars().all(|ch| ch.is_ascii_hexdigit()), "fingerprint must be hex: {one}");
}

#[test]
fn fingerprint_changes_when_payload_changes() {
    let baseline = AbiPayload::current()
        .fingerprint()
        .expect("baseline fingerprint");
    let mut mutated = AbiPayload::current();
    mutated.abi_version = 999;
    let mutated_fingerprint = mutated.fingerprint().expect("mutated fingerprint");
    assert_ne!(baseline, mutated_fingerprint, "fingerprint must reflect payload content");
}

#[test]
fn json_render_is_deterministic_and_includes_fingerprint() {
    let one = render_json(&AbiPayload::current()).expect("json one");
    let two = render_json(&AbiPayload::current()).expect("json two");
    assert_eq!(one, two, "json render must be stable");

    // Pretty JSON ends with a closing brace and is multi-line.
    assert!(one.starts_with('{'));
    assert!(one.contains(&format!("\"name\": \"{COMPILER_NAME}\"")));
    assert!(one.contains("\"abi_version\": 1"));
    assert!(one.contains("\"fingerprint\":"));
    assert!(one.contains("\"symbols\":"));
    assert!(one.contains("\"diagnostics\":"));

    let fingerprint = AbiPayload::current().fingerprint().expect("fingerprint");
    assert!(
        one.contains(&format!("\"{fingerprint}\"")),
        "json must embed the computed fingerprint"
    );
}

#[test]
fn rust_render_emits_compilable_constants() {
    let rust = render_rust(&AbiPayload::current()).expect("rust render");
    // Expected declarations are asserted through bindings so the artifact
    // content (not diagnostic message text) is what is checked.
    let abi_version_decl = "pub const ABI_VERSION: u32 = 1;";
    assert!(rust.contains("//! Generated host ABI constants"));
    assert!(rust.contains(abi_version_decl));
    assert!(rust.contains("pub const SYMBOL_INIT: &str = \"__faber_rt_v1_init\";"));
    assert!(rust.contains("pub const SYMBOL_PROGRAM_ENTRY: &str = \"__faber_program_entry_v1\";"));
    assert!(rust.contains("pub type FaberRtValueKindV1 = u32;"));
    assert!(rust.contains("pub const VALUE_KIND_I64: FaberRtValueKindV1 = 4;"));
    // LLVM type spellings are LLVM-backend facts and live in radix-mir-llvm (Faber 2 A7).
    assert!(!rust.contains("LLVM_SLICE_TYPE"));
    assert!(rust.contains("pub const STATUS_OK: i32 = 0;"));
    assert!(rust.contains("pub const STATUS_UNSUPPORTED: i32 = 4;"));
    assert!(rust.contains("pub const DIAGNOSTIC_SYMBOLS_V1: &[(&str, &str, &str)] = &["));
    assert!(rust.contains("pub fn diagnostic_symbol_v1(kind: &str, carrier: &str) -> Option<&'static str>"));
    assert!(rust.contains("__faber_rt_v1_diagnostic_nota_ptr"));
    assert!(rust.contains(&format!("Compiler: {COMPILER_NAME}")));
}

#[test]
fn diagnostic_symbol_resolver_matches_runtime_table() {
    assert_eq!(
        host_abi::diagnostic_symbol_v1("nota", "i64"),
        Some("__faber_rt_v1_diagnostic_nota_i64"),
    );
    assert_eq!(
        host_abi::diagnostic_symbol_v1("vide", "text"),
        Some("__faber_rt_v1_diagnostic_vide_text"),
    );
    assert!(host_abi::diagnostic_symbol_v1("unknown", "carrier").is_none());
}
