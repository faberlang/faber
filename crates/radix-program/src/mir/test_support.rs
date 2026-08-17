//! FMIR image test summaries (RP5).
//!
//! Small decode-and-summarize helpers for FMIR images, used by the
//! radix-program mir tests and re-exported by faber for its package test
//! suite. Non-`cfg(test)` because faber (a dependent crate) consumes the
//! summaries.

use super::*;

#[derive(Debug, PartialEq, Eq)]
pub struct FmirImageTestSummary {
    pub version: u32,
    pub entry: String,
    pub toolchain_version: String,
    pub exit_code: Option<i32>,
    pub runtime_requirements: Vec<String>,
    pub source_count: usize,
    pub interner_len: usize,
    pub function_count: usize,
}

pub fn fmir_text_image_test_summary(
    text: &str,
    path: &Path,
) -> Result<FmirImageTestSummary, Vec<Diagnostic>> {
    let image: FmirTextImageFile = toml::from_str(text).map_err(|error| {
        vec![mir_diag(
            path,
            format!("could not parse fmir-text image: {error}"),
        )]
    })?;
    let program: MirProgram = serde_json::from_str(&image.program.json).map_err(|error| {
        vec![mir_diag(
            path,
            format!("could not decode fmir-text MIR program: {error}"),
        )]
    })?;
    Ok(FmirImageTestSummary {
        version: image.version,
        entry: image.entry,
        toolchain_version: image.toolchain.faber_cli_version,
        exit_code: image.exit_code,
        runtime_requirements: image.runtime.requirement,
        source_count: image.sources.source.len(),
        interner_len: image.interner.len(),
        function_count: program.functions.len(),
    })
}

pub fn fmir_image_test_summary(
    bytes: &[u8],
    path: &Path,
) -> Result<FmirImageTestSummary, Vec<Diagnostic>> {
    let image: FmirBinaryImageFile = postcard::from_bytes(bytes).map_err(|error| {
        vec![mir_diag(
            path,
            format!("could not decode fmir image: {error}"),
        )]
    })?;
    Ok(FmirImageTestSummary {
        version: image.version,
        entry: image.entry,
        toolchain_version: image.toolchain.faber_cli_version,
        exit_code: image.exit_code,
        runtime_requirements: image.runtime.requirement,
        source_count: image.sources.source.len(),
        interner_len: image.interner.len(),
        function_count: image.program.functions.len(),
    })
}
