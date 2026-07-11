use super::{emit_go_module, inject_after_imports, render_norma_consolum_shim, GoBuildLayout};
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(Box::<dyn Error>::from)?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("faber-{label}-{nonce}"));
    fs::create_dir_all(&path)?;
    Ok(path)
}

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

#[test]
fn emit_go_module_replaces_stale_owned_sources() -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_dir("go-build-stale-files")?;
    let layout = GoBuildLayout {
        module_root: root.join("target").join("faber").join("go"),
        binary_path: root
            .join("target")
            .join("faber")
            .join("go")
            .join("bin")
            .join("demo"),
        package_name: "demo".to_owned(),
    };

    emit_go_module(
        &layout,
        "package main\n\nfunc main() {}\n",
        &[(
            "alpha.go".to_owned(),
            "package main\n\nfunc alpha() {}\n".to_owned(),
        )],
    )
    .expect("first emit");
    fs::write(
        layout.binary_path.parent().expect("bin dir").join("demo"),
        b"keep",
    )?;

    emit_go_module(&layout, "package main\n\nfunc main() {}\n", &[]).expect("second emit");

    assert!(!layout.module_root.join("alpha.go").exists());
    assert!(layout.module_root.join("main.go").exists());
    assert!(layout.module_root.join("go.mod").exists());
    assert!(
        layout.binary_path.exists(),
        "binary output must be preserved"
    );
    Ok(())
}
