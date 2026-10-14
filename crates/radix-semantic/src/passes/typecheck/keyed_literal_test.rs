//! Keyed-literal source acceptance at a `Verte` check. A stated `json` /
//! `map<K, V>` type, the hole `_ { … }`, and the conversion `{ … } ↦ value`
//! all leave the JSON object data type explicit; the `Verte` check names
//! `json` as the expected type for a JSON object source instead of passing no
//! expectation. A bare keyed literal with no typed cell is a
//! named error in a later unit.

use crate::PassConfig;
use crate::locale::LocalePack;
use crate::test_support::analyze_source_with_config;
use radix_target::Target;

fn config() -> PassConfig {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    config
}

fn check(source: &str) -> crate::test_support::AnalyzedFixture {
    analyze_source_with_config(source, config())
}

fn accepts(source: &str) {
    let result = check(source);
    assert!(
        result.is_ok(),
        "expected green for {source:?}, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn keyed_literal_accepts_json_typed_cell() {
    accepts("fn main() {\n    const json j ← { \"a\": 1 }\n}\n");
}

#[test]
fn keyed_literal_accepts_map_typed_cell() {
    accepts("fn main() {\n    const map<string, int> m ← { \"a\": 1 }\n}\n");
}

#[test]
fn keyed_literal_accepts_hole_constructor() {
    accepts("fn main() {\n    const json k ← _ { \"a\": 1 }\n}\n");
}

#[test]
fn keyed_literal_accepts_value_conversion() {
    accepts("fn main() {\n    const value v ← { \"a\": 1 } ↦ value\n}\n");
}

#[test]
fn keyed_literal_accepts_stated_json_constructor() {
    accepts("fn main() {\n    const json s ← json { \"a\": 1 }\n}\n");
}
