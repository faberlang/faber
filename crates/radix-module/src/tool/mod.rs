//! Command implementation layer for Faber `CLI` surfaces.
//!
//! This module is the executable boundary around the `radix` compiler library.
//! Clap shapes live in [`cli`]; handlers live in [`commands`] (`inspect`, `check`,
//! `emit`, `compile`, `targets`, `json`, `source`, `package`).
//! It owns stdin/file source loading, terminal diagnostics,
//! `JSON`-ish inspection output, target formatting/linting hooks, and the policy
//! split between the developer `radix` binary and the user-facing `faber`
//! package tool.
//!
//! `radix` remains a single-file compiler and phase-inspection tool. Package
//! compilation is intentionally rejected here and delegated to `crates/faber`,
//! where manifests, import graphs, stdlib binding, and generated `Cargo` layouts
//! are available. That separation keeps compiler phase debugging lightweight
//! while preventing the developer tool from growing a second package policy.
//!
//! ERROR STRATEGY
//! ==============
//! The command functions are process-facing: they print diagnostics and call
//! `std::process::exit` on fatal errors. Reusable helpers such as
//! [`mir_output_for_source`], [`compile_cli_source`], and formatter/linter
//! wrappers return values so tests and wrappers can exercise the same policy
//! without spawning a binary.
//!
//! INVARIANTS
//! ==========
//! - Stdin is valid for single-file commands and invalid for package mode.
//! - `radix` package requests fail fast with a message pointing to `faber`.
//! - Inspection commands expose deterministic, machine-readable output for
//!   tests and tools rather than pretty terminal prose.
//! - Formatting and linting are best-effort post-processing steps; failures are
//!   warnings that leave generated compiler output available.

pub mod cli;
pub mod commands;

pub use cli::*;
pub use commands::*;
