use super::{
    format_ceiling_line, format_count_floor_line, format_tier_line, generated_rust_needs_tokio,
    write_rust_cargo_project,
};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

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
fn write_rust_cargo_project_links_tokio_when_generated_code_uses_block_on() {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("radix-e2e-manifest-tokio-{nanos}"));
    let code = "fn main() { __faber_block_on(async { }); }";
    let manifest_path = write_rust_cargo_project(&dir, "tokio_fixture", code);
    let manifest = fs::read_to_string(manifest_path).expect("read manifest");
    assert!(
        manifest.contains("package = \"faber-runtime\"") && manifest.contains("path ="),
        "manifest should depend on sibling faber-runtime: {manifest}"
    );
    assert!(manifest.contains("tokio = { version = \"1\""));
}
