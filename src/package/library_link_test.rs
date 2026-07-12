use super::{
    binding_for_function, promote_binding_function_visibility, promote_library_surface_visibility,
    render_library_cargo_toml,
};
use crate::package::manifest::ManifestTarget;
use std::collections::BTreeMap;
use std::path::Path;

#[test]
fn visibility_promotion_ignores_string_literals() {
    let source = r#"pub(crate) fn exported() {}
let note = "pub(crate) fn hidden() {}";
    pub(crate) struct Visible;
// pub(crate) enum CommentOnly {}
"#;

    let promoted = promote_library_surface_visibility(source);

    assert!(promoted.contains("pub fn exported() {}"));
    assert!(promoted.contains("    pub struct Visible;"));
    assert!(promoted.contains("\"pub(crate) fn hidden() {}\""));
    assert!(promoted.contains("// pub(crate) enum CommentOnly {}"));
}

#[test]
fn binding_lookup_requires_the_exact_module_suffix() {
    let bindings = BTreeMap::from([(
        "provider:other.value".to_owned(),
        super::FunctionBinding {
            symbol: "other".to_owned(),
        },
    )]);

    assert!(binding_for_function(&bindings, "", "value").is_none());
    assert!(binding_for_function(&bindings, "nested", "value").is_none());
}

#[test]
fn async_binding_wrappers_are_public_to_consumers() {
    let source = "#[allow(dead_code)]\nasync fn invoke() {\n    ready().await;\n}\n";
    let promoted = promote_binding_function_visibility(source);

    assert!(promoted.contains("async fn invoke"));
    assert!(promoted.contains("pub async fn invoke"));
    assert!(promoted.contains("    ready().await;"));
}

#[test]
fn linked_library_cargo_manifest_escapes_values_and_inline_paths() {
    let target = ManifestTarget {
        dependencies: BTreeMap::from([
            ("dep\"key".to_owned(), "1.2.3\"\n# injected".to_owned()),
            (
                "inline\"key".to_owned(),
                r#"{ version = "1.0", path = "native\"shim" }"#.to_owned(),
            ),
        ]),
        ..ManifestTarget::default()
    };
    let rendered = render_library_cargo_toml(
        "library",
        "0.1.0\"\n# injected",
        Path::new("/tmp/app"),
        Path::new("/tmp/package"),
        &target,
    );
    let manifest = toml::from_str::<toml::Value>(&rendered).expect("valid Cargo TOML");
    let package = manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .expect("package table");
    assert_eq!(
        package.get("version").and_then(toml::Value::as_str),
        Some("0.1.0\"\n# injected")
    );
    let dependencies = manifest
        .get("dependencies")
        .and_then(toml::Value::as_table)
        .expect("dependency table");
    assert_eq!(
        dependencies.get("dep\"key").and_then(toml::Value::as_str),
        Some("1.2.3\"\n# injected")
    );
    let inline = dependencies
        .get("inline\"key")
        .and_then(toml::Value::as_table)
        .expect("inline dependency table");
    assert_eq!(
        inline.get("path").and_then(toml::Value::as_str),
        Some("/tmp/package/native\"shim")
    );
}
