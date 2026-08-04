//! `faber emit -t faber` policy wrapper.

use crate::cli::FaberCliTarget;
use radix::tool::EmitCommand;

pub fn cmd_emit_faber(command: EmitCommand) {
    if command.package {
        eprintln!("error: package Faber emit is not supported; use single-file input");
        std::process::exit(1);
    }

    // Faber owns reader-locale → pack resolution (install layout + package
    // manifests live here, not in radix). Resolve once and hand the pack to the
    // shared emit path so `-t faber --locale=<X>` emits localized Faber.
    let locale_pack = match crate::package::locale_pack_for_emit(
        &command.input,
        command.locale.as_deref(),
    ) {
        Ok(pack) => pack,
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    };

    radix::tool::cmd_emit_with_locale_pack(command, locale_pack.as_ref());
}

pub fn is_faber_emit(target: FaberCliTarget) -> bool {
    target.is_faber()
}
