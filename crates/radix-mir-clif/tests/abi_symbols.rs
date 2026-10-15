//! D3 proof: the generator's runtime symbol table and the `runtime/clif`
//! exports agree (goal `radix-mir-clif`).
//!
//! This crate cannot depend on `runtime/*`, so the test reads the runtime's
//! source: every table entry must be an exported `extern "C"` function there,
//! and every `__faber_clif_v1_*` export there must be in the table.

use std::collections::BTreeSet;
use std::path::Path;

use radix_mir_clif::abi::RUNTIME_SYMBOLS;

fn runtime_exports() -> BTreeSet<String> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../runtime/clif/src/lib.rs");
    let text = std::fs::read_to_string(&source).expect("runtime/clif/src/lib.rs is readable");
    let mut exports = BTreeSet::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line
            .strip_prefix("pub extern \"C\" fn ")
            .or_else(|| line.strip_prefix("pub unsafe extern \"C\" fn "))
        else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
            .collect();
        exports.insert(name);
    }
    exports
}

#[test]
fn every_table_symbol_has_a_runtime_export() {
    let exports = runtime_exports();
    for symbol in RUNTIME_SYMBOLS {
        assert!(
            exports.contains(symbol.name),
            "runtime/clif does not export {}",
            symbol.name
        );
    }
}

#[test]
fn every_runtime_export_is_in_the_table() {
    let table: BTreeSet<&str> = RUNTIME_SYMBOLS.iter().map(|symbol| symbol.name).collect();
    for export in runtime_exports() {
        assert!(
            table.contains(export.as_str()),
            "runtime/clif exports {export}, which the generator's table does not declare"
        );
    }
}
