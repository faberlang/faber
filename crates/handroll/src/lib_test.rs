/// Panic if this crate ever grows a `[dependencies]` section or external deps.
#[test]
fn manifest_is_std_only() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("handroll Cargo.toml next to src/");
    let in_deps = manifest.split("[dependencies]").nth(1).is_none_or(|rest| {
        // Stop at the next section; an empty or absent section is std-only.
        let section = rest.split('[').next().unwrap_or("");
        section.trim().is_empty()
    });
    assert!(
        in_deps,
        "handroll must stay dependency-free (removal-delivery-spec U5a)"
    );
}
