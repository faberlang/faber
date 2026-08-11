#[path = "src/core_support/assembler.rs"]
mod assembler;
#[path = "src/core_support/path_deps.rs"]
mod path_deps;

use std::env;
use std::fs;
use std::path::PathBuf;

const MANIFEST: &str = "core-support-manifest.txt";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    // Stage 1 recomposition: the faber crate now lives at
    // <container>/radix/crates/faber/. The core-support manifest travels with
    // the crate, while the logical roots it names stay container-relative
    // (destination-map §6.3) — so resolve them against the container root,
    // three levels up from the crate directory.
    let workspace = crate_dir.join("../../..");
    let manifest = crate_dir.join(MANIFEST);
    println!("cargo:rerun-if-changed={}", manifest.display());

    let roots = assembler::read_roots(&manifest)?;
    let assembly = assembler::assemble(&workspace, &roots)?;
    for root in &roots {
        println!("cargo:rerun-if-changed={}", workspace.join(root).display());
    }
    for file in &assembly.files {
        println!(
            "cargo:rerun-if-changed={}",
            workspace.join(&file.path).display()
        );
    }

    let out = PathBuf::from(env::var("OUT_DIR")?);
    fs::write(out.join("core-support.tar.zst"), &assembly.archive)?;
    fs::write(out.join("core-support.sha256"), &assembly.archive_sha256)?;
    fs::write(
        out.join("core-support.files.sha256"),
        assembler::file_manifest(&assembly.files),
    )?;
    println!(
        "cargo:rustc-env=FABER_CORE_SUPPORT_SHA256={}",
        assembly.archive_sha256
    );
    Ok(())
}
