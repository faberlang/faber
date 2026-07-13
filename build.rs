#[path = "src/core_support/assembler.rs"]
mod assembler;

use std::env;
use std::fs;
use std::path::PathBuf;

const MANIFEST: &str = "core-support-manifest.txt";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let workspace = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?)
        .parent()
        .ok_or("Faber crate must have a workspace parent")?
        .to_path_buf();
    let manifest = workspace.join("faber").join(MANIFEST);
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
