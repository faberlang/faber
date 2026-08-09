use super::*;

/// Bindings record with a package-level default shim and optional
/// per-stem overrides.
fn bindings(default_shim: &Path, per_stem: &[(&str, &Path)]) -> TsLibraryBindings {
    TsLibraryBindings {
        shim_path: default_shim.to_path_buf(),
        shims: per_stem
            .iter()
            .map(|(stem, path)| (stem.to_string(), path.to_path_buf()))
            .collect(),
        functions: BTreeMap::new(),
    }
}

/// Write a runtime shim file under `root/runtime/` and return its path.
fn write_shim(root: &Path, name: &str) -> PathBuf {
    let runtime_dir = root.join("runtime");
    fs::create_dir_all(&runtime_dir).expect("create runtime dir");
    let path = runtime_dir.join(name);
    fs::write(&path, "export const shimNoop = () => {};\n").expect("write shim");
    path
}

fn exports(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(api, symbol)| (api.to_string(), symbol.to_string()))
        .collect()
}

#[test]
fn dom_facade_output_is_unchanged() {
    // Existing dom behavior is additive: the dom facade still imports the
    // shared DOM type aliases and re-exports them under Faber API names.
    let dir = tempfile::tempdir().expect("create ts root");
    let dom_shim = write_shim(dir.path(), "dom.ts");
    let bindings = bindings(&dom_shim, &[]);
    let exports = exports(&[("attr_set", "webDomAttrSet")]);

    emit_ts_binding_facade(
        Path::new("/repo/pkg"),
        "faber-web",
        "dom",
        "faber-web-dom.ts",
        &bindings,
        &exports,
        dir.path(),
    )
    .expect("emit dom facade");

    // The dom runtime shim is copied into the product tree.
    assert!(dir.path().join("faber-web-shim-dom.ts").is_file());

    let code = fs::read_to_string(dir.path().join("faber-web-dom.ts")).expect("read facade");
    // Value re-exports and namespace object.
    assert!(code.contains("  webDomAttrSet as attr_set,"));
    assert!(code.contains("export {"));
    assert!(code.contains("export const dom = {"));
    assert!(code.contains("  attr_set"));
    // Genus type-alias import + re-export from the dom shim.
    assert!(code.contains("import type {"));
    assert!(code.contains("  WebDomScope,"));
    assert!(code.contains("export type Scope = WebDomScope;"));
    assert!(code.contains("export type Element = WebDomElement;"));
    assert!(code.contains("from \"./faber-web-shim-dom.js\""));
}

#[test]
fn canvas2d_facade_uses_own_shim_and_reexports_genus() {
    // A second binding module (web:canvas2d) with its own per-stem shim:
    // the generated facade imports from the canvas2d shim (not the dom
    // default) and re-exports Canvas2dContext via the stem type-alias row.
    let dir = tempfile::tempdir().expect("create ts root");
    let dom_shim = write_shim(dir.path(), "dom.ts");
    let canvas2d_shim = write_shim(dir.path(), "canvas2d.ts");
    let bindings = bindings(&dom_shim, &[("canvas2d", &canvas2d_shim)]);
    let exports = exports(&[
        ("canvas2d_context", "webCanvas2dContext"),
        ("canvas2d_save", "webCanvas2dSave"),
    ]);

    emit_ts_binding_facade(
        Path::new("/repo/pkg"),
        "faber-web",
        "canvas2d",
        "faber-web-canvas2d.ts",
        &bindings,
        &exports,
        dir.path(),
    )
    .expect("emit canvas2d facade");

    // The canvas2d runtime shim is copied into the product tree under its
    // own name (independent of the package-level dom shim).
    assert!(dir.path().join("faber-web-shim-canvas2d.ts").is_file());
    assert!(!dir.path().join("faber-web-shim-dom.ts").is_file());

    let code = fs::read_to_string(dir.path().join("faber-web-canvas2d.ts")).expect("read facade");
    // Value imports come from the canvas2d shim, not the dom default.
    assert!(code.contains("  webCanvas2dContext as canvas2d_context,"));
    assert!(code.contains("from \"./faber-web-shim-canvas2d.js\""));
    assert!(!code.contains("faber-web-shim-dom"));
    // Genus type-alias import + re-export through the facade.
    assert!(code.contains("import type {"));
    assert!(code.contains("  WebCanvas2dContext,"));
    assert!(code.contains("export type Canvas2dContext = WebCanvas2dContext;"));
    // Namespace object uses the stem name.
    assert!(code.contains("export const canvas2d = {"));
}

#[test]
fn stem_without_type_aliases_emits_no_alias_block() {
    // The per-stem table is opt-in: a stem with no declared aliases gets
    // no `import type`/`export type` block (regression guard against
    // hardcoding a single stem).
    let dir = tempfile::tempdir().expect("create ts root");
    let web_shim = write_shim(dir.path(), "web.ts");
    let bindings = bindings(&web_shim, &[]);
    let exports = exports(&[("beep", "webBeep")]);

    emit_ts_binding_facade(
        Path::new("/repo/pkg"),
        "acme-web",
        "web",
        "acme-web-web.ts",
        &bindings,
        &exports,
        dir.path(),
    )
    .expect("emit facade without aliases");

    let code = fs::read_to_string(dir.path().join("acme-web-web.ts")).expect("read facade");
    assert!(!code.contains("import type {"));
    assert!(!code.contains("export type "));
    assert!(code.contains("export const web = {"));
}

#[test]
fn load_bindings_resolves_per_stem_shims() {
    // A package hosting two binding modules (web:dom + web:canvas2d)
    // wires the default `[shim]` for unlisted stems and `[shims.<stem>]`
    // overrides for the second module.
    let dir = tempfile::tempdir().expect("create package root");
    let pkg = dir.path();
    fs::create_dir_all(pkg.join("bindings")).expect("create bindings dir");
    fs::create_dir_all(pkg.join("runtime")).expect("create runtime dir");
    fs::write(
        pkg.join("faber.toml"),
        r#"[package]
name = "faber-web"
version = "0.1.0"

[build]
kind = "lib"
targets = ["ts"]

[target.ts]
bindings = "bindings/ts.toml"
"#,
    )
    .expect("write faber.toml");
    fs::write(
        pkg.join("bindings/ts.toml"),
        r#"[shim]
path = "runtime/dom.ts"

[shims.canvas2d]
path = "runtime/canvas2d.ts"

[functions."web:dom.attr_set"]
symbol = "webDomAttrSet"

[functions."web:canvas2d.canvas2d_context"]
symbol = "webCanvas2dContext"
"#,
    )
    .expect("write binding manifest");
    fs::write(
        pkg.join("runtime/dom.ts"),
        "export const webDomAttrSet = () => {};\n",
    )
    .expect("write dom shim");
    fs::write(
        pkg.join("runtime/canvas2d.ts"),
        "export const webCanvas2dContext = () => {};\n",
    )
    .expect("write canvas2d shim");

    let bindings = load_ts_library_bindings(pkg)
        .expect("load bindings")
        .expect("bindings present");

    // Default shim applies to the dom stem…
    assert_eq!(
        bindings.shim_for_stem("dom"),
        pkg.join("runtime/dom.ts").as_path()
    );
    // …while canvas2d resolves its own shim.
    assert_eq!(
        bindings.shim_for_stem("canvas2d"),
        pkg.join("runtime/canvas2d.ts").as_path()
    );
    // Per-stem exports still resolve per module.
    assert_eq!(
        bindings.module_exports("dom"),
        Some(exports(&[("attr_set", "webDomAttrSet")]))
    );
    assert_eq!(
        bindings.module_exports("canvas2d"),
        Some(exports(&[("canvas2d_context", "webCanvas2dContext")]))
    );
}

#[test]
fn load_bindings_absent_without_shim_and_shims() {
    // A binding manifest with functions but neither `[shim]` nor
    // `[shims.<stem>]` loads no bindings (unchanged historical behavior).
    let dir = tempfile::tempdir().expect("create package root");
    let pkg = dir.path();
    fs::create_dir_all(pkg.join("bindings")).expect("create bindings dir");
    fs::write(
        pkg.join("faber.toml"),
        r#"[package]
name = "faber-web"
version = "0.1.0"

[build]
kind = "lib"
targets = ["ts"]

[target.ts]
bindings = "bindings/ts.toml"
"#,
    )
    .expect("write faber.toml");
    fs::write(
        pkg.join("bindings/ts.toml"),
        r#"[functions."web:dom.attr_set"]
symbol = "webDomAttrSet"
"#,
    )
    .expect("write binding manifest");

    assert!(
        load_ts_library_bindings(pkg)
            .expect("load bindings")
            .is_none()
    );
}
