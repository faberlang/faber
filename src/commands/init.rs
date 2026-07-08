//! `faber init` — scaffold a new package directory.

use crate::cli::InitArgs;

/// Creates the minimal on-disk package shape expected by the package commands.
pub(super) fn cmd_init(args: InitArgs) {
    let root = args.path;
    let src = root.join("src");
    let manifest = root.join("faber.toml");
    let entry = src.join("main.fab");
    let package_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && *name != ".")
        .unwrap_or("faber-package");

    if manifest.exists() || entry.exists() {
        eprintln!(
            "error: package files already exist in {}; refusing to overwrite",
            root.display()
        );
        std::process::exit(1);
    }

    if let Err(err) = std::fs::create_dir_all(&src) {
        eprintln!("error: failed to create '{}': {}", src.display(), err);
        std::process::exit(1);
    }

    let manifest_source = format!(
        "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[paths]\nsource = \"src\"\nentry = \"main.fab\"\n\n[build]\ntarget = \"rust\"\nkind = \"bin\"\n",
        package_name
    );
    if let Err(err) = std::fs::write(&manifest, manifest_source) {
        eprintln!("error: failed to write '{}': {}", manifest.display(), err);
        std::process::exit(1);
    }

    if let Err(err) = std::fs::write(&entry, "incipit {\n    nota \"Salve, munde!\"\n}\n") {
        eprintln!("error: failed to write '{}': {}", entry.display(), err);
        std::process::exit(1);
    }

    println!("{}", manifest.display());
}
