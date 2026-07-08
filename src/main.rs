//! User-facing Faber project and package tool (`faber` binary).
//!
//! Clap shapes live in [`cli`]; handlers live in [`commands`]. Package-aware
//! compilation routes through [`package`]; single-file compiler inspection
//! delegates to `radix::tool`.

mod cli;
mod commands;
mod io_buf;
mod library;
mod package;
mod script;

#[cfg(test)]
#[path = "cli_test.rs"]
mod cli_test;

#[cfg(test)]
#[path = "script_test.rs"]
mod script_test;

fn main() {
    commands::run();
}
