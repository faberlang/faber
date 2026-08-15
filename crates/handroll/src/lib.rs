//! Hand-rolled primitives replacing small external crates in the default build
//! (removal-delivery-spec U5).
//!
//! std-only, zero dependencies — enforced by `manifest_is_std_only`. Consumers:
//! `entropy`/`uuid_v4` (U5b, replaces rand/getrandom in the runner),
//! `sha256` (U5c, replaces sha2), error `impl`s live at their call sites (U5d).

/// Panic if this crate ever grows a `[dependencies]` section or external deps.
#[test]
fn manifest_is_std_only() {
    let manifest = std::fs::read_to_string("Cargo.toml")
        .expect("handroll Cargo.toml sits next to src/ in test cwd");
    let in_deps = manifest
        .split("[dependencies]")
        .nth(1)
        .map(|rest| {
            // Stop at the next section; an empty or absent section is std-only.
            let section = rest.split('[').next().unwrap_or("");
            section.trim().is_empty()
        })
        .unwrap_or(true);
    assert!(
        in_deps,
        "handroll must stay dependency-free (removal-delivery-spec U5a)"
    );
}
