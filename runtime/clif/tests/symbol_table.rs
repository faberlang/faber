//! The JIT's explicit symbol table equals the runtime's exports: every
//! `extern "C"` function in the sources is in `runtime_symbols()` under its own
//! name, and nothing else is.

use std::collections::BTreeSet;
use std::path::Path;

fn source_exports() -> BTreeSet<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut exports = BTreeSet::new();
    for entry in std::fs::read_dir(&dir).expect("runtime/clif/src is readable") {
        let path = entry.expect("directory entry").path();
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("runtime source is readable");
        for line in text.lines() {
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
    }
    exports
}

#[test]
fn the_symbol_table_is_the_set_of_runtime_exports() {
    let table = faber_rt_clif::runtime_symbols();
    let names: BTreeSet<String> = table.iter().map(|(name, _)| (*name).to_owned()).collect();
    assert_eq!(names.len(), table.len(), "a symbol is listed twice");
    assert_eq!(names, source_exports());
}

#[test]
fn every_symbol_has_an_address() {
    for (name, address) in faber_rt_clif::runtime_symbols() {
        assert!(!address.is_null(), "{name} has a null address");
    }
}
