use super::{
    format_ceiling_line, format_count_floor_line, format_tier_line, generated_rust_needs_tokio,
    make_temp_root, write_rust_cargo_project,
};
use std::fs;

#[test]
fn format_tier_line_includes_live_total_and_floor() {
    let line = format_tier_line("MIR lowered", 194, 210, 184);
    assert_eq!(line, "  MIR lowered: 194/210 (floor 184)");
}

#[test]
fn format_ceiling_line_includes_live_and_ceiling() {
    let line = format_ceiling_line("unsupported diagnostic", 5, 5);
    assert_eq!(line, "  unsupported diagnostic: 5 (ceiling 5)");
}

#[test]
fn format_count_floor_line_includes_live_and_floor() {
    let line = format_count_floor_line("unsupported diagnostic", 54, 15);
    assert_eq!(line, "  unsupported diagnostic: 54 (floor 15)");
}

#[test]
fn generated_rust_needs_tokio_detects_async_runtime() {
    assert!(!generated_rust_needs_tokio("fn main() {}"));
    assert!(generated_rust_needs_tokio(
        "fn main() { __faber_block_on(async {}); }"
    ));
    assert!(generated_rust_needs_tokio(
        "fn main() { tokio::runtime::Builder::new_current_thread(); }"
    ));
}

#[test]
fn make_temp_root_removes_owned_tree_on_drop() {
    let path = {
        let temp_root = make_temp_root();
        fs::write(temp_root.join("fixture.txt"), "fixture").expect("write temp fixture");
        temp_root.to_path_buf()
    };

    assert!(!path.exists(), "E2E temp root leaked: {}", path.display());
}

#[test]
fn write_rust_cargo_project_links_tokio_when_generated_code_uses_block_on() {
    let temp_root = make_temp_root();
    let code = "fn main() { __faber_block_on(async { }); }";
    let manifest_path = write_rust_cargo_project(&temp_root, "tokio_fixture", code);
    let manifest = fs::read_to_string(manifest_path).expect("read manifest");
    assert!(
        manifest.contains("package = \"faber-runtime\"") && manifest.contains("path ="),
        "manifest should depend on sibling faber-runtime: {manifest}"
    );
    assert!(manifest.contains("tokio = { version = \"1\""));
}
