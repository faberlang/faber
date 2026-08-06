//! `faber emit -t faber` policy wrapper.

use crate::cli::FaberCliTarget;
use radix::tool::EmitCommand;

pub fn cmd_emit_faber(command: EmitCommand) {
    if command.package {
        eprintln!("error: package Faber emit is not supported; use single-file input");
        std::process::exit(1);
    }

    cmd_emit_with_locale(command);
}

/// Emit a single-file target with Faber-owned reader-locale resolution.
///
/// TypeScript remains on Radix's direct-file route, but it still needs the
/// package-aware locale resolver here when a library source file uses a
/// non-Latin reader surface. Keeping this seam in Faber avoids asking Radix
/// to rediscover Faber's install and package layout.
pub fn cmd_emit_with_locale(command: EmitCommand) {
    if command.package {
        eprintln!("error: package emit must use the package compiler route");
        std::process::exit(1);
    }

    // Faber owns locale → pack resolution (install layout + package manifests
    // live here, not in radix). Code pack drives emit surface; diagnostic pack
    // drives message rendering (`--diagnostic-locale` when set).
    let code_pack =
        match crate::package::locale_pack_for_emit(&command.input, command.locale.as_deref()) {
            Ok(pack) => pack,
            Err(message) => {
                eprintln!("error: {message}");
                std::process::exit(1);
            }
        };
    let diagnostic_pack = match command.diagnostic_locale.as_deref() {
        Some(locale) => match crate::package::locale_pack_for_emit(&command.input, Some(locale)) {
            Ok(pack) => pack,
            Err(message) => {
                eprintln!("error: {message}");
                std::process::exit(1);
            }
        },
        None => code_pack.clone(),
    };

    radix::tool::cmd_emit_with_locale_packs(command, code_pack.as_ref(), diagnostic_pack.as_ref());
}

pub fn is_faber_emit(target: FaberCliTarget) -> bool {
    target.is_faber()
}
