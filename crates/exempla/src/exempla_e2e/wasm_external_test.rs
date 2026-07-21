use super::{parse_wat_import_sites, probe_wasm_instantiation_stubless, WasmInstantiationBucket};

#[test]
fn parses_wat_import_sites() {
    let wat = r#"
(module
    (import "faber_diag" "nota_i32" (func $__faber_diag_nota_i32 (param i32)))
    (import "faber_runtime" "append" (func $__faber_runtime_append (param i32 i32)))
)
"#;
    let imports = parse_wat_import_sites(wat);
    assert_eq!(imports.len(), 2);
    assert_eq!(imports[0].module, "faber_diag");
    assert_eq!(imports[0].name, "nota_i32");
}

#[test]
fn classifies_missing_import_for_stubless_host() {
    let wat = r#"
(module
    (import "faber_diag" "nota_i32" (func $__faber_diag_nota_i32 (param i32)))
)
"#;
    let imports = parse_wat_import_sites(wat);
    let probe = probe_wasm_instantiation_stubless(std::path::Path::new("/dev/null"), &imports);
    assert_eq!(probe.bucket, WasmInstantiationBucket::MissingImport);
    assert_eq!(probe.imports.len(), 1);
}
