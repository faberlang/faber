#![cfg(feature = "hir-fhir")]

//! Explicit size arguments in a generic call cross the FHIR seam unchanged.
//!
//! A `size` argument (`make<int, size 4>`, `shaped<int, size [2, 2]>`) is
//! carried as a `Magnitudo` generic witness, and a whole shape is one tuple
//! index. This proves the artifact keeps them: the loaded unit renders the
//! marked call forms byte-identically to the direct unit, re-emits Rust
//! without a size or a shape leaking in as a type argument, and a mixed list
//! binds by marker after the round trip.

use radix_module::Output;
use radix_module::codegen::{Target, generate_from_analyzed};
use radix_module::driver::{AnalyzeRequest, AnalyzedModule, Config, Session, analyze_source};
use radix_module::hir::artifact::load_fhir;
use radix_module::locale::{KeywordSurface, LocalePack};

fn pack_for(locale: &str) -> LocalePack {
    let pack_path = radix_module::locale::workspace_locale_pack_path(locale);
    LocalePack::from_toml_path(&pack_path).unwrap_or_else(|err| panic!("installed {locale} pack should load: {err}"))
}

const SOURCE: &str = r#"fn make<T, size N>(list<T> xs) → tensor<T, [N]> ⇥ string {
    return xs ↦ tensor<T, [N]>
}

fn shaped<T, size Shape>(list<T> data) → tensor<T, Shape> ⇥ string {
    return data ↦ tensor<T, Shape>
}

fn total(tensor<int, [4]> cells) → int {
    return 0
}

main {
    let list<int> data ← [5, 6, 7, 8]
    let tensor<int, [4]> row ← make<int, size 4>(data)
    let tensor<int, [4]> inferred ← make<size 4>(data)
    let tensor<int, [2, 2]> grid ← shaped<int, size [2, 2]>(data)
    print total(shaped<int, size [4]>(data))
}
"#;

fn analyze_source_once() -> AnalyzedModule {
    let config = Config::default().with_locale_pack(pack_for("en"));
    let session = Session::new(config);
    match analyze_source(&session, "explicit-size.fab", SOURCE, AnalyzeRequest::default()) {
        Ok(unit) => unit,
        Err(diagnostics) => panic!("analysis failed: {diagnostics:?}"),
    }
}

fn export_fhir(unit: &AnalyzedModule) -> Vec<u8> {
    let artifact = radix_module::hir::serialize::from_analyzed(unit, "test_hash", "explicit-size.fab");
    postcard::to_allocvec(&artifact).expect("postcard serialization should succeed")
}

fn emit(target: Target, unit: &AnalyzedModule) -> String {
    let pack = pack_for("en");
    let surface = KeywordSurface::new(&pack);
    match generate_from_analyzed(target, unit, &surface) {
        Ok(Output::Faber(output)) => output.code,
        Ok(Output::Rust(output)) => output.code,
        Ok(_) => panic!("unexpected output kind for {target:?}"),
        Err(err) => panic!("{target:?} emit failed: {err:?}"),
    }
}

fn assert_roundtrip_is_identical(target: Target, unit: &AnalyzedModule, loaded: &AnalyzedModule) {
    let direct = emit(target, unit);
    let via_fhir = emit(target, loaded);
    assert_eq!(
        direct, via_fhir,
        "{target:?}: the loaded unit must emit what the direct unit emits"
    );
}

#[test]
fn marked_size_arguments_render_identically_through_fhir() {
    let unit = analyze_source_once();
    let loaded = load_fhir(&export_fhir(&unit)).expect("FHIR artifact should load");

    // The `HirRust` leg needs the private rust leaf, so it compiles out of the
    // public cut and the `HirFaber` leg proves the seam alone there.
    assert_roundtrip_is_identical(Target::HirFaber, &unit, &loaded);
    #[cfg(feature = "hir-rust")]
    assert_roundtrip_is_identical(Target::HirRust, &unit, &loaded);
}

#[test]
fn faber_emit_keeps_the_marker_and_the_whole_shape() {
    let unit = analyze_source_once();
    let loaded = load_fhir(&export_fhir(&unit)).expect("FHIR artifact should load");
    let faber = emit(Target::HirFaber, &loaded);

    for form in [
        "make<int, size 4>(data)",
        "make<size 4>(data)",
        "shaped<int, size [2, 2]>(data)",
        "shaped<int, size [4]>(data)",
    ] {
        assert!(faber.contains(form), "the Faber emit must re-spell `{form}`:\n{faber}");
    }
}

// Rust-emission probe: needs the private rust leaf, so it compiles out of
// the public cut (the `HirFaber` probes above keep the seam covered there).
#[cfg(feature = "hir-rust")]
#[test]
fn rust_emit_never_spells_a_size_or_shape_as_a_type() {
    let unit = analyze_source_once();
    let loaded = load_fhir(&export_fhir(&unit)).expect("FHIR artifact should load");
    let rust = emit(Target::HirRust, &loaded);

    assert!(
        !rust.contains("magnitudo"),
        "a size marker must not reach the Rust turbofish:\n{rust}"
    );
    assert!(
        !rust.contains("size 4"),
        "a size argument must not reach the Rust turbofish:\n{rust}"
    );
    // The mixed and whole-shape calls keep only their type argument in the
    // turbofish; the shape-only call has nothing left to spell. A
    // shape-returning generic takes its shape at run time (a value argument),
    // never as a type.
    assert!(
        rust.contains("make::<i64>(l_data.clone())"),
        "mixed call keeps the type only:\n{rust}"
    );
    assert!(
        rust.contains("shaped::<i64>(l_data.clone(), &[Some(2), Some(2)])"),
        "shape call keeps the type only in the turbofish and passes its shape at run time:\n{rust}"
    );
    assert!(
        rust.contains("make(l_data.clone())"),
        "a shape-only call has no turbofish:\n{rust}"
    );
    assert!(!rust.contains("::<i64, "), "no size or shape beside the type:\n{rust}");
}
