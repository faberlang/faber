//! Typed JSON conversions retain imported record identity and field metadata.

use super::*;
use radix_mir_runner::BufferHost;

#[test]
fn package_mir_round_trips_imported_json_record() {
    let dir = crate::test_support::test_temp_dir("imported-json-record");
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create sources");
    fs::write(
        dir.join("faber.toml"),
        r#"[package]
name = "imported-json-record"
version = "1.0.0"
[paths]
source = "src"
entry = "main.fab"
[reader]
locale = "en"
"#,
    )
    .expect("write manifest");
    fs::write(
        src.join("model.fab"),
        "@ public\n@ json {}\nclass Data { var int count\nvar list<int> cells }\n",
    )
    .expect("write model");
    fs::write(
        src.join("main.fab"),
        r#"import from "./model" model
main {
    const model.Data original ← model.Data { count = 3, cells = [7, 11] }
    const json encoded ← original ↦ json
    const model.Data decoded ← encoded ↦ model.Data
    print decoded.count
    print decoded.cells[1]
}
"#,
    )
    .expect("write entry");
    let mut host = BufferHost::default();
    run_package_mir(
        &Config::default(),
        &src.join("main.fab"),
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("imported record must round trip through JSON");
    assert_eq!(host.stdout_lines, ["3", "11"]);
}
