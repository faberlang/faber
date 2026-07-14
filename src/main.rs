//! User-facing Faber project and package tool (`faber` binary).
//!
//! Clap shapes live in [`cli`]; handlers live in [`commands`]. Package-aware
//! compilation routes through [`package`]; single-file compiler inspection
//! delegates to `radix::tool`.

mod cli;
mod commands;
mod core_support;
mod input_shape;
mod io_buf;
mod library;
mod package;
mod script;

#[cfg(test)]
#[path = "cli_test.rs"]
mod cli_test;

#[cfg(test)]
#[path = "input_shape_test.rs"]
mod input_shape_test;

#[cfg(test)]
#[path = "script_test.rs"]
mod script_test;

const PACKAGE_DIAGNOSTIC_CODE: &str = "PKG001";

fn package_diagnostic_error(message: impl Into<String>) -> radix::diagnostics::Diagnostic {
    radix::diagnostics::Diagnostic::error(message).with_code(PACKAGE_DIAGNOSTIC_CODE)
}

fn main() {
    commands::run();
}
