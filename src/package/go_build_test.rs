use super::{inject_after_imports, render_norma_consolum_shim};

#[test]
fn inject_after_single_line_imports_preserves_following_imports() {
    let entry = "package main\n\nimport \"fmt\"\nimport \"os\"\n\nfunc main() {}\n";
    let injected = inject_after_imports(entry, "var namespace_alpha = alpha");

    let imports_end = injected
        .find("import \"os\"\n")
        .expect("expected second import");
    let namespace_start = injected
        .find("var namespace_alpha = alpha\n")
        .expect("expected namespace block");
    assert!(imports_end < namespace_start);
    assert!(injected.ends_with("func main() {}\n"));
}

#[test]
fn inject_after_package_without_imports_inserts_after_blank_lines() {
    let entry = "package main\n\nfunc main() {}\n";
    let injected = inject_after_imports(entry, "var namespace_alpha = alpha");

    assert_eq!(
        injected,
        "package main\n\nvar namespace_alpha = alpha\n\nfunc main() {}\n"
    );
}

#[test]
fn render_norma_consolum_shim_covers_full_public_surface() {
    let shim = render_norma_consolum_shim("consolum");

    for snippet in [
        "Hauri func(int64) []byte",
        "Hauriet func(int64) []byte",
        "Lege func() string",
        "Leget func() string",
        "Funde func([]byte)",
        "Fundet func([]byte)",
        "Dicet func(string)",
        "Scribet func(string)",
        "Monet func(string)",
        "Vide func(string)",
        "Videbit func(string)",
        "Audit func() bool",
        "Loquitur func() bool",
        "Admonet func() bool",
        "func consolum_isTerminal(file *os.File) bool",
    ] {
        assert!(
            shim.contains(snippet),
            "expected shim snippet `{snippet}` in:\n{shim}"
        );
    }
}
