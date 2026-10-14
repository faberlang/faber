use super::{library_module_dash_name, library_specifier_to_relative};

#[test]
fn library_specifiers_dash_join_provider_and_module_path() {
    assert_eq!(
        library_module_dash_name("tela:dom").as_deref(),
        Some("tela-dom")
    );
    assert_eq!(
        library_module_dash_name("triga:graph/object").as_deref(),
        Some("triga-graph-object")
    );
    assert_eq!(
        library_specifier_to_relative("triga:graph/object").as_deref(),
        Some("./triga-graph-object.js")
    );
}

#[test]
fn non_library_specifiers_have_no_emitted_stem() {
    for spec in [
        "./local",
        "norma:math",
        ":dom",
        "tela:",
        "tela:a:b",
        "kernel:*",
    ] {
        assert_eq!(library_module_dash_name(spec), None, "{spec}");
        assert_eq!(library_specifier_to_relative(spec), None, "{spec}");
    }
}
