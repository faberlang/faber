//! Path formula and generated-source extraction. Process I/O lives in `radix-cli`.

use std::path::{Path, PathBuf};

/// Derive the output path for source-emission builds.
#[must_use]
pub fn build_output_path(
    out_dir: &Path,
    input_path: &Path,
    target: crate::codegen::Target,
    is_package: bool,
) -> PathBuf {
    let base_name = if is_package {
        "main".to_owned()
    } else {
        input_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .unwrap_or("out")
            .to_owned()
    };
    out_dir.join(format!("{}.{}", base_name, super::targets::target_extension(target)))
}

/// Extract generated source from a target-specific compiler output.
#[must_use]
pub fn output_code(output: crate::Output) -> String {
    match output {
        crate::Output::Rust(out) => out.code,
        crate::Output::TypeScript(out) => out.code,
        crate::Output::Go(out) => out.code,
        crate::Output::Swift(out) => out.code,
        crate::Output::Python(out) => out.code,
        crate::Output::Haskell(out) => out.code,
        #[cfg(feature = "hir-pascal")]
        crate::Output::Pascal(out) => out.code,
        #[cfg(feature = "hir-cobol")]
        crate::Output::Cobol(out) => out.code,
        #[cfg(feature = "hir-fortran")]
        crate::Output::Fortran(out) => out.code,
        #[cfg(feature = "hir-odin")]
        crate::Output::Odin(out) => out.code,
        #[cfg(feature = "hir-c")]
        crate::Output::C(out) => out.code,
        crate::Output::Faber(out) => out.code,
        #[cfg(feature = "mir-wasm")]
        crate::Output::WasmText(out) => out.code,
        #[cfg(feature = "mir-llvm")]
        crate::Output::LlvmText(out) => out.code,
        #[cfg(feature = "mir-clif")]
        crate::Output::ClifObject(_) | crate::Output::ClifJit(_) => String::new(),
        #[cfg(feature = "mir-metal")]
        crate::Output::MetalText(out) => out.code,
        #[cfg(feature = "mir-wgsl")]
        crate::Output::WgslText(out) => out.code,
        #[cfg(feature = "mir-sexp")]
        crate::Output::Sexp(out) => out.code,
        #[cfg(feature = "mir-basic")]
        crate::Output::Basic(out) => out.code,
    }
}
