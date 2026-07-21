//! Stable diagnostic traces for behavior-checked Wasm exempla under the stub host.

#[cfg(test)]
#[path = "wasm_behavior_fixtures_test.rs"]
mod tests;

pub struct WasmBehaviorFixture {
    pub exemplum: &'static str,
    pub expected_diag: &'static [&'static str],
}

pub const WASM_BEHAVIOR_FIXTURES: &[WasmBehaviorFixture] = &[
    // Text handle index includes module comment/header strings interned at parse time.
    WasmBehaviorFixture {
        exemplum: "incipit/salve-munde.fab",
        expected_diag: &["nota_text:9"],
    },
    WasmBehaviorFixture {
        exemplum: "incipit/incipit.fab",
        expected_diag: &["nota_text:10"],
    },
    WasmBehaviorFixture {
        exemplum: "nota/nota.fab",
        // The stub host does not implement `faber_text::format_*` yet, so
        // formatted strings flow through as the default text handle `0`.
        expected_diag: &[
            "nota_text:11",
            "nota_text:15",
            "nota_text:0",
            "nota_text:0",
            "nota_text:0",
            "nota_text:0",
        ],
    },
    WasmBehaviorFixture {
        exemplum: "functio/functio.fab",
        expected_diag: &[
            "nota_text:10",
            "nota_text:22",
            "nota_text:17",
            "nota_i64:42",
        ],
    },
    WasmBehaviorFixture {
        exemplum: "unarius/unarius.fab",
        expected_diag: &[
            "nota_i64:-5",
            "nota_i32:0",
            "nota_i32:1",
            "nota_i32:0",
            "nota_i32:0",
            "nota_i32:1",
            "nota_i32:1",
            "nota_i32:0",
            "nota_i32:0",
            "nota_i32:1",
        ],
    },
    WasmBehaviorFixture {
        exemplum: "vide/vide.fab",
        expected_diag: &["vide_text:7"],
    },
];

pub fn expected_wasm_behavior(exemplum: &str) -> Option<&'static [&'static str]> {
    WASM_BEHAVIOR_FIXTURES
        .iter()
        .find_map(|fixture| (fixture.exemplum == exemplum).then_some(fixture.expected_diag))
}

pub fn behavior_matches(expected: &[&str], actual: &[String]) -> bool {
    expected.len() == actual.len()
        && expected
            .iter()
            .zip(actual)
            .all(|(expected, actual)| expected == actual)
}
