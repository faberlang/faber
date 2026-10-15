//! `faber init` — scaffold a new package directory.

use crate::cli::InitArgs;

/// Creates the minimal on-disk package shape expected by the package commands.
pub(super) fn cmd_init(args: InitArgs) {
    let root = args.path;
    let package_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && *name != ".")
        .unwrap_or("faber-package");

    if let Err(message) = scaffold(&root, package_name) {
        eprintln!("error: {message}");
        std::process::exit(1);
    }
    println!("{}", root.join("faber.toml").display());
}

/// Write the scaffold files for a new package. Split from `cmd_init` so tests
/// can drive it without process exits.
pub(crate) fn scaffold(root: &std::path::Path, package_name: &str) -> Result<(), String> {
    let src = root.join("src");
    let manifest = root.join("faber.toml");
    let entry = src.join("main.fab");

    if manifest.exists() || entry.exists() {
        return Err(format!(
            "package files already exist in {}; refusing to overwrite",
            root.display()
        ));
    }

    if let Err(err) = std::fs::create_dir_all(&src) {
        return Err(format!("failed to create '{}': {}", src.display(), err));
    }

    let manifest_source = format!(
        "[package]\nname = \"{package_name}\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[paths]\nsource = \"src\"\nentry = \"main.fab\"\n\n[build]\nkind = \"bin\"\n"
    );
    if let Err(err) = std::fs::write(&manifest, manifest_source) {
        return Err(format!("failed to write '{}': {}", manifest.display(), err));
    }

    // Untagged sources parse on the `en` reader surface by default, so the
    // scaffold must be en-surface or the fresh package fails its own check.
    if let Err(err) = std::fs::write(&entry, "main {\n    print \"Hello, world!\"\n}\n") {
        return Err(format!("failed to write '{}': {}", entry.display(), err));
    }

    Ok(())
}
