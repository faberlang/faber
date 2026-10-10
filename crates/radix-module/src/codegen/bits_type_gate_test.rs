//! Need: the raw-bits types `b8 b16 b32 b64` are implemented only by
//! the Faber source emitter, the Rust, Go, TypeScript, Swift and sexp emitters
//! and the MIR runner. Every
//! other emitter rejects a module that names one with the named `bits_type_unsupported_on_target`
//! diagnostic before emission, instead of printing a placeholder identifier
//! or a plain integer carrier.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const SOURCE: &str = "functio f(b8 a) → b8 { redde a }\nincipit {\n}\n";

fn diagnostics(target: Target) -> Vec<(String, String)> {
    let session = Session::new(Config::default().with_target(target).with_dev_locale_root());
    let result = crate::driver::compile(&session, "<test>", SOURCE, ModuleCompileRequest::default());
    result
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "bits_type_unsupported_on_target")
        })
        .map(|diagnostic| {
            let value = |name: &str| {
                diagnostic
                    .args
                    .iter()
                    .find(|arg| arg.name == name)
                    .map(|arg| arg.value.clone())
                    .unwrap_or_default()
            };
            (value("type"), value("target"))
        })
        .collect()
}

fn assert_rejected(target: Target, spelled: &str) {
    let found = diagnostics(target);
    assert_eq!(
        found,
        vec![("b8".to_owned(), spelled.to_owned())],
        "{target:?} did not reject b8 with the named diagnostic"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_emitter_keeps_bits() {
    assert_eq!(diagnostics(Target::HirRust), Vec::<(String, String)>::new());
}

#[cfg(feature = "hir-go")]
#[test]
fn go_emitter_keeps_bits() {
    assert_eq!(diagnostics(Target::HirGo), Vec::<(String, String)>::new());
}

#[cfg(feature = "hir-ts")]
#[test]
fn typescript_keeps_bits() {
    assert_eq!(diagnostics(Target::HirTypeScript), Vec::<(String, String)>::new());
}

#[cfg(feature = "hir-swift")]
#[test]
fn swift_emitter_keeps_bits() {
    assert_eq!(diagnostics(Target::HirSwift), Vec::<(String, String)>::new());
}

#[cfg(feature = "mir-llvm")]
#[test]
fn llvm_emitter_keeps_bits() {
    assert_eq!(diagnostics(Target::MirLlvm), Vec::<(String, String)>::new());
}

#[cfg(feature = "mir-wasm")]
#[test]
fn wasm_keeps_bits() {
    assert_eq!(diagnostics(Target::MirWasm), Vec::<(String, String)>::new());
}

/// The wasm-text emission of a Latin `incipit` body, with its diagnostics.
#[cfg(feature = "mir-wasm")]
fn wasm_text(body: &str) -> String {
    let source = format!("incipit {{\n{body}\n}}\n");
    let session = Session::new(
        Config::default()
            .with_target(Target::MirWasm)
            .with_dev_locale_root(),
    );
    let result = crate::driver::compile(&session, "<test>", &source, ModuleCompileRequest::default());
    match result.output {
        Some(crate::Output::WasmText(output)) => output.code,
        _ => panic!("no wasm text: {:?}", result.diagnostics),
    }
}

#[cfg(feature = "mir-wasm")]
#[test]
fn wasm_bits_not_flips_exactly_the_width() {
    let b8 = wasm_text("fixum b8 a ← 0xf3\nnota ¬ a");
    assert!(b8.contains("(i32.xor (local.get $l0) (i32.const 255))"), "{b8}");
    let b16 = wasm_text("fixum b16 a ← 0x00f3\nnota ¬ a");
    assert!(b16.contains("(i32.xor (local.get $l0) (i32.const 65535))"), "{b16}");
    let b64 = wasm_text("fixum b64 a ← 0xf3\nnota ¬ a");
    assert!(b64.contains("(i64.xor (local.get $l0) (i64.const -1))"), "{b64}");
}

#[cfg(feature = "mir-wasm")]
#[test]
fn wasm_bits_shift_past_width_is_zero_and_negative_count_traps() {
    let text = wasm_text("fixum b8 a ← 0xf3\nnota a ⇐ 8\nnota a ⇒ 9");
    // Width and mask travel with the call; the helper zeroes a count of N or
    // more (unsigned compare, the count is not wrapped) and traps a negative
    // one with the runner's message (kind 2).
    assert!(text.contains("(call $faber_bits_shl (i64.extend_i32_u (local.get $l0)) (i64.const 8) (i64.const 8) (i64.const 255) (i32.const 1))"), "{text}");
    assert!(text.contains("(call $faber_bits_shr"), "{text}");
    assert!(text.contains("(i64.ge_u (local.get $c) (local.get $n))"), "{text}");
    assert!(text.contains("(i64.lt_s (local.get $c) (i64.const 0))"), "{text}");
    assert!(text.contains("(call $faber_x_trap (i32.const 2))"), "{text}");
    assert!(text.contains("negative shift count"), "{text}");
    assert!(text.contains("(i64.shr_u (local.get $v) (local.get $c))"), "{text}");
}

#[cfg(feature = "mir-wasm")]
#[test]
fn wasm_bits_display_is_hex_at_full_width() {
    let text = wasm_text(
        "fixum b8 a ← 0x01\nfixum b16 b ← 0x0001\nfixum b32 c ← 0x01\nfixum b64 d ← 0x01\nnota a\nnota b\nnota c\nnota d",
    );
    for digits in [2, 4, 8, 16] {
        assert!(text.contains(&format!(" (i32.const {digits}))")), "{digits}: {text}");
    }
    assert!(text.contains("(func $faber_bits_hex"), "{text}");
    assert!(
        text.contains("(data (i32.const 1) \"0x\")") || text.contains("\"0x\""),
        "{text}"
    );
}

#[cfg(feature = "mir-wasm")]
#[test]
fn wasm_bits_equality_and_order_read_the_pattern_unsigned() {
    let text = wasm_text(
        "fixum b8 a ← 0x80\nfixum b8 b ← 0x7f\nnota a ≻ b\nnota a ≺ b\nnota a ≥ b\nnota a ≤ b\nnota a ≡ b\nnota a ≠ b",
    );
    for opcode in ["i32.gt_u", "i32.lt_u", "i32.ge_u", "i32.le_u", "i32.eq", "i32.ne"] {
        assert!(
            text.contains(&format!("({opcode} (local.get $l0) (local.get $l1))")),
            "{opcode}: {text}"
        );
    }
    assert!(!text.contains("i32.gt_s"), "{text}");
}

#[cfg(feature = "mir-wasm")]
#[test]
fn wasm_displays_a_list_of_bits_in_module_at_full_width() {
    let text = wasm_text("fixum lista<b8> xs ← [0x00, 0x0f]\nfixum lista<b32> ys ← [0x01]\nnota xs\nnota ys");
    // One helper per displayed element width, joined `[a, b]` from the
    // bracket and separator rows, reading the existing array rows.
    for digits in [2, 8] {
        assert!(
            text.contains(&format!("(func $faber_bits_list_hex_{digits} (param $a i32) (result i32)")),
            "{digits}: {text}"
        );
        assert!(
            text.contains(&format!("(call $faber_bits_list_hex_{digits} (local.get $l")),
            "{digits}: {text}"
        );
    }
    assert!(!text.contains("(func $faber_bits_list_hex_4"), "{text}");
    assert!(!text.contains("(func $faber_bits_list_hex_16"), "{text}");
    assert!(text.contains(r#"(import "faber_rt_v1" "__faber_rt_v1_array_length" (func $faber_bits_list_len (param i32) (result i64)))"#), "{text}");
    assert!(text.contains(r#"(import "faber_rt_v1" "__faber_rt_v1_array_get" (func $faber_bits_list_get (param i32 i64) (result i64)))"#), "{text}");
    assert!(
        text.contains("(call $faber_bits_hex (call $faber_bits_list_get (local.get $a) (local.get $i)) (i32.const 2))"),
        "{text}"
    );
    assert!(
        text.contains("(br_if $done (i64.ge_s (local.get $i) (local.get $n)))"),
        "{text}"
    );
    for literal in ["\"[\"", "\", \"", "\"]\""] {
        assert!(text.contains(literal), "{literal}: {text}");
    }
}

#[cfg(feature = "mir-wasm")]
#[test]
fn wasm_refuses_display_of_a_nested_list_of_bits_by_name() {
    let source = "incipit {\nfixum lista<b8> xs ← [0x00]\nfixum lista<lista<b8>> ys ← [xs, xs]\nnota ys\n}\n";
    let session = Session::new(
        Config::default()
            .with_target(Target::MirWasm)
            .with_dev_locale_root(),
    );
    let result = crate::driver::compile(&session, "<test>", source, ModuleCompileRequest::default());
    assert!(result.output.is_none(), "{:?}", result.diagnostics);
    let message = format!("{:?}", result.diagnostics);
    assert!(message.contains("nested collection of raw bits"), "{message}");
}

#[cfg(feature = "mir-sexp")]
#[test]
fn sexp_keeps_bits() {
    assert_eq!(diagnostics(Target::MirSexp), Vec::<(String, String)>::new());
}

#[cfg(feature = "hir-faber")]
#[test]
fn faber_source_emitter_keeps_bits() {
    assert_eq!(diagnostics(Target::HirFaber), Vec::<(String, String)>::new());
}
