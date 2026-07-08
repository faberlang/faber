//! `faber emit -t faber` policy wrapper.

use crate::cli::FaberCliTarget;
use radix::tool::EmitCommand;

pub fn cmd_emit_faber(command: EmitCommand) {
    if let Some(locale) = command.reader_locale.as_ref() {
        eprintln!("error: --reader-locale {locale} for Faber output is deferred to format reader-locale support");
        std::process::exit(1);
    }

    if command.package {
        eprintln!("error: package Faber emit is not supported; use single-file input");
        std::process::exit(1);
    }

    radix::tool::cmd_emit(command);
}

pub fn is_faber_emit(target: FaberCliTarget) -> bool {
    target.is_faber()
}
