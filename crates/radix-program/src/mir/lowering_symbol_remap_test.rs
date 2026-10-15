//! B11: a sibling unit's lowering interns runtime diagnostic text (the
//! lockstep length-mismatch message, format templates, …) into the unit's
//! interner *after* analysis. Package linking must remap MIR text symbols
//! from that post-lowering table; remapping from the pre-lowering analysis
//! clone indexed past its end and panicked in `Interner::resolve`
//! (gradus `model/moe.proba`).

use super::*;

const HELPER: &str = r"@ public
fn dot(list<int> a, list<int> b) → int {
    mut int sum ← 0
    for from a, b let x, y {
        sum ← sum + x * y
    }
    return sum
}
";

const MAIN: &str = r#"import from "./helper" helper

main {
    print helper.dot([1, 2], [3, 4])
}
"#;

fn write_package(dir: &Path) -> PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "lowering-symbol-remap"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"
"#,
    )
    .expect("write faber.toml");
    fs::write(src.join("helper.fab"), HELPER).expect("write helper.fab");
    fs::write(src.join("main.fab"), MAIN).expect("write main.fab");
    src.join("main.fab")
}

#[test]
fn sibling_lowering_added_text_symbols_remap_into_merged_interner() {
    let dir = crate::test_support::test_temp_dir("lowering-symbol-remap");
    let entry = write_package(dir.as_ref());
    let linked = with_interpreted_lowered_package_mir(
        &Config::default(),
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let message = lowered
                .interner
                .lookup("itera lockstep sources must have equal length")
                .map(MirConstant::String);
            (message, format!("{:?}", lowered.validated.program()))
        },
    )
    .unwrap_or_else(|errors| panic!("sibling lockstep helper must link, got {errors:?}"));
    let (message, program) = linked;
    let message = message.expect("merged interner carries the lockstep message");
    assert!(
        program.contains(&format!("{message:?}")),
        "merged program must reference the remapped lockstep message symbol"
    );
}
