//! Companion tests for `backend_smoke`: smoke-helper regression probes.
//! Backend-leaf emission smoke belongs in the backend leaf crates; these tests
//! only prove primary-owned helper behavior.

use super::{
    assert_emitted_rust_typechecks, assert_mir_llvm_smoke, cargo_available, llvm_as_available, mangle_emitted_text,
    rustc_available,
};
use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session, compile};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn emit_rust_from_faber(source: &str) -> String {
    let session = Session::new(Config::default().with_target(Target::HirRust).with_latin_locale());
    let result = compile(&session, "smoke.fab", source, ModuleCompileRequest::default());
    assert!(result.success(), "{:?}", result.diagnostics);
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("expected Rust output");
    };
    output.code
}

fn emit_llvm_from_faber(source: &str) -> String {
    let session = Session::new(Config::default().with_target(Target::MirLlvm).with_latin_locale());
    let result = compile(&session, "smoke.fab", source, ModuleCompileRequest::default());
    let Some(crate::Output::LlvmText(output)) = result.output else {
        panic!("expected LLVM text output: {:?}", result.diagnostics);
    };
    output.code
}

#[ignore = "slow: spawns cargo check of generated Rust; run with --ignored"]
#[test]
fn smoke_hir_rust_mangled_real_emit_fails_when_rustc_present() {
    if !rustc_available() {
        eprintln!("rustc not found on PATH; skipping rust mangled-emit self-test");
        return;
    }
    if !cargo_available() || crate::test_support::faber_runtime_crate_path().is_none() {
        eprintln!(
            "cargo or faber runtime crate not available; skipping rust mangled-emit self-test \
             (assert_emitted_rust_typechecks would skip the typecheck and pass vacuously)"
        );
        return;
    }
    let emitted = emit_rust_from_faber("incipit { nota \"salve\" }");
    let mangled = mangle_emitted_text(&emitted);
    let failed = catch_unwind(AssertUnwindSafe(|| {
        assert_emitted_rust_typechecks(&mangled, "self-test-rust-mangled");
    }));
    assert!(failed.is_err(), "mangled real Rust emit must fail smoke when rustc is present");
}

#[test]
fn smoke_mir_llvm_mangled_real_emit_fails_when_llvm_as_present() {
    if !llvm_as_available() {
        eprintln!("llvm-as not found on PATH; skipping llvm mangled-emit self-test");
        return;
    }
    let ll = emit_llvm_from_faber(
        r"
functio adde(numerus a, numerus b) → numerus {
    redde a + b
}
",
    );
    let mangled = mangle_emitted_text(&ll);
    let failed = catch_unwind(AssertUnwindSafe(|| {
        assert_mir_llvm_smoke(&mangled, "self-test-llvm-mangled");
    }));
    assert!(
        failed.is_err(),
        "mangled real LLVM emit must fail smoke when llvm-as is present"
    );
}
