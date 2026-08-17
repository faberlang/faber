//! `radix abi` command handler — compiler-artifact ABI introspection.
//!
//! Renders the Radix-owned ABI payload to stdout by default, or to an explicit
//! `--output` path for CI or release packaging. The payload and renderers live
//! in [`crate::abi`]; this module is only the process-facing glue.

use std::io::Write;

use crate::abi;
use crate::tool::cli::{AbiArgs, AbiFormat};

/// Entry point for `radix abi`.
///
/// Prints diagnostics to stderr and exits non-zero on render or write failure.
pub fn cmd_abi(args: AbiArgs) {
    let payload = abi::AbiPayload::current();
    let rendered = match args.format {
        AbiFormat::Json => abi::render_json(&payload),
        AbiFormat::Rust => abi::render_rust(&payload),
    };

    let text = match rendered {
        Ok(text) => text,
        Err(error) => {
            eprintln!("radix abi: failed to render ABI payload: {error}");
            std::process::exit(1);
        }
    };

    match args.output {
        None => {
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            if let Err(error) = writeln!(handle, "{text}") {
                eprintln!("radix abi: failed to write to stdout: {error}");
                std::process::exit(1);
            }
        }
        Some(path) => {
            if let Err(error) = std::fs::write(&path, format!("{text}\n")) {
                eprintln!("radix abi: failed to write {}: {error}", path.display());
                std::process::exit(1);
            }
        }
    }
}
