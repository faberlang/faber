//! User-facing Faber project and package tool (`faber` binary).
//!
//! Clap shapes live in [`cli`]; handlers live in [`commands`]. Package-aware
//! compilation routes through the `faber_cli` lib's `package` module (S09-2:
//! the shared modules — `package`, `library`, `input_shape`, `io_buf`,
//! `postprocess` — are declared only in the lib and compiled once); single-file
//! compiler inspection delegates to typed `radix::module::services`.

mod cli;
mod commands;
mod script;

#[cfg(test)]
#[path = "cli_test.rs"]
mod cli_test;

#[cfg(test)]
#[path = "script_test.rs"]
mod script_test;

#[cfg(test)]
#[path = "construction_source_run_test.rs"]
mod construction_source_run_test;

#[cfg(test)]
#[path = "multi_print_one_line_test.rs"]
mod multi_print_one_line_test;

fn main() {
    commands::run();
}
