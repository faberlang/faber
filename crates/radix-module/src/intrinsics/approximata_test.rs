//! Tests for the `approximata` intrinsic (fractus only, absolute-compare).
//! Proves: fractus-only registration; typecheck acceptance; emitted Rust
//! uses `.abs()` + `<=` not a method call.

use super::{IntrinsicReceiver, lookup_method, lookup_method_for_type};
#[cfg(feature = "hir-rust")]
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::intrinsics::mir_collection_op;
use crate::semantic::{Primitive, TypeTable};
use radix_types::MirCollectionOp;

#[cfg(feature = "hir-rust")]
fn compile_rust(name: &str, source: &str) -> String {
    let compiler = crate::Compiler::new(crate::Config::default().with_latin_locale());
    let result = compiler.compile_str(name, source);
    let Some(crate::Output::Rust(rust)) = result.output else {
        panic!("expected Rust output, got diagnostics: {:?}", result.diagnostics);
    };
    rust.code
}

#[test]
fn approximata_is_fractus_only_in_registry() {
    assert!(lookup_method(IntrinsicReceiver::Fractus, "approximata", 2).is_some());

    let types = TypeTable::new();
    let fractus = types.get(types.primitive(Primitive::Fractus));
    let approximata = lookup_method_for_type(fractus, "approximata", 2).expect("fractus approximata");
    assert_eq!(mir_collection_op(approximata), Some(MirCollectionOp::Approximate));

    let numerus = types.get(types.primitive(Primitive::Numerus));
    assert!(lookup_method_for_type(numerus, "approximata", 2).is_none());
}

#[cfg(feature = "hir-rust")]
#[test]
fn fractus_approximata_typechecks_and_emits_abs_compare() {
    let session = Session::new(Config::default().with_latin_locale());
    let source = r"incipit {
    fixum fractus x ← 1.5
    fixum bivalens near ← x.approximata(1.51, 0.05)
    nota near
}";
    let result = analyze_source(&session, "fractus-approximata.fab", source, AnalyzeRequest::default());
    assert!(result.is_ok(), "typecheck failed: {:?}", result.err());

    let code = compile_rust("fractus-approximata.fab", source);
    assert!(code.contains(".abs()"));
    assert!(code.contains("<="));
    assert!(!code.contains(".approximata("));
}
