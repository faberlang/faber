//! Core compiler library for Faber.
//!
//! `radix` owns the language implementation: lexing, parsing, semantic
//! analysis, lowering, target code generation, diagnostics, and the
//! developer-facing inspection surfaces that expose those phases. The
//! user-facing `faber` crate builds package and project workflows on top of
//! this library; package policy, manifest loading, and generated Cargo layouts
//! deliberately live there instead of here.
//!
//! The public API keeps the embedding contract small. Callers choose a
//! [`Config`], instantiate a [`Compiler`], and receive a [`CompileResult`] that
//! carries both output and diagnostics. Command-line tools use the same driver
//! pipeline as library callers so phase behavior does not drift between API and
//! terminal use.
//!
//! PIPELINE
//! ========
//!
//! ```text
//! Source (.fab)
//!   → Lexer (tokens)
//!   → Parser (AST)
//!   → Collect + Resolve + Lower
//!   → Typecheck + Analysis
//!   → Codegen (Rust, Faber, TypeScript, or Go)
//! ```
//!
//! INVARIANTS
//! ==========
//! - Malformed source returns diagnostics rather than panicking.
//! - Missing semantic facts must be fixed before codegen; backend guessing is
//!   not part of the contract.
//! - Rust is the primary executable backend; canonical Faber re-emission is a
//!   compiler backend used by the in-tree `forma` formatter and the developer CLI.
//! - CLI and library entry points both go through `driver` so diagnostics and
//!   phase ordering remain consistent.
//!
//! ```no_run
//! use radix::{Compiler, Config, Target};
//! use std::path::Path;
//!
//! let config = Config::default().with_target(Target::Rust);
//! let compiler = Compiler::new(config);
//! let result = compiler.compile(Path::new("example.fab"));
//!
//! if result.success() {
//!     // Use result.output
//! } else {
//!     for diag in &result.diagnostics {
//!         eprintln!("{:?}", diag);
//!     }
//! }
//! ```

// Clippy pedantic allowlist lives in Cargo.toml [lints.clippy] so it covers
// lib, integration tests, examples, and bins (crate attrs only cover lib).

pub mod abi;
pub mod air;
pub mod ascii_literal;
pub mod aspect;
#[cfg(test)]
pub mod backend_smoke;
pub mod builtins;
pub mod cli;
pub mod codegen;
pub mod diagnostics;
pub mod driver;
pub mod file_interface;
pub mod forma;
#[cfg(test)]
pub mod forma_render;
pub mod hir;
pub mod import_resolve;
pub mod intrinsics;
pub mod kernel;
pub mod lexer;
pub mod live_tuus_cursor;
pub mod llvm_host;
pub mod mir;
pub mod octeti_literal;
pub mod parser;
pub mod proba;
pub mod reader_locale;
pub mod runtime_contract;
pub mod semantic;
pub mod syntax;
#[cfg(test)]
pub mod test_support;
pub mod textus_literal;
pub mod tool;

use std::path::Path;

pub use codegen::Target;
pub use diagnostics::{is_known_diagnostic_code, Diagnostic, DiagnosticConvert};
pub use driver::{apply_warn_policy, Config, Session, WarnPolicy};

/// Output and diagnostics from one compiler invocation.
///
/// Successful compilations may still carry warnings. Callers should use
/// [`CompileResult::success`] when they need the same success policy as the CLI:
/// target output must exist and no diagnostic may be classified as an error.
pub struct CompileResult {
    /// Target-specific generated output, absent when compilation failed before
    /// codegen or codegen rejected the analyzed program.
    pub output: Option<Output>,

    /// Ordered diagnostics accumulated across compiler phases.
    pub diagnostics: Vec<Diagnostic>,
}

impl CompileResult {
    /// Return true when codegen produced output and no error diagnostics remain.
    pub fn success(&self) -> bool {
        self.output.is_some()
            && !self
                .diagnostics
                .iter()
                .any(radix_diagnostics::Diagnostic::is_error)
    }
}

/// Generated source for the requested backend.
///
/// The enum keeps callers honest about backend-specific output while avoiding
/// stringly typed target dispatch at the public API boundary.
pub enum Output {
    /// Rust backend output, currently the primary executable target.
    Rust(RustOutput),

    /// TypeScript backend output.
    TypeScript(TypeScriptOutput),

    /// Go backend output.
    Go(GoOutput),

    /// Swift backend output.
    Swift(SwiftOutput),

    /// Canonical Faber source output.
    Faber(FaberOutput),

    /// Experimental WebAssembly text output emitted from validated MIR.
    WasmText(WasmTextOutput),

    /// Experimental WebAssembly binary output emitted from validated MIR.
    Wasm(WasmOutput),

    /// Experimental LLVM text output emitted from validated MIR.
    LlvmText(LlvmTextOutput),

    /// Experimental Metal Shading Language source emitted from validated MIR.
    MetalText(MetalTextOutput),

    /// Experimental WGSL compute-shader source emitted from validated MIR.
    WgslText(WgslTextOutput),

    /// Experimental Racket s-expression output emitted from validated MIR.
    Sexp(SexpOutput),
}

/// Rust backend output.
pub struct RustOutput {
    /// Complete generated Rust source.
    pub code: String,
}

/// TypeScript backend output.
pub struct TypeScriptOutput {
    /// Complete generated TypeScript source.
    pub code: String,
}

/// Go backend output.
pub struct GoOutput {
    /// Complete generated Go source.
    pub code: String,
}

/// Swift backend output.
pub struct SwiftOutput {
    /// Complete generated Swift source.
    pub code: String,
}

/// Canonical Faber source output.
pub struct FaberOutput {
    /// Complete re-emitted Faber source.
    pub code: String,
}

/// Experimental WebAssembly text output.
pub struct WasmTextOutput {
    /// Complete generated WAT source.
    pub code: String,
}

/// Experimental WebAssembly binary output.
pub struct WasmOutput {
    /// Complete generated Wasm module bytes.
    pub bytes: Vec<u8>,
}

/// Experimental LLVM text output.
pub struct LlvmTextOutput {
    /// Complete generated LLVM text source.
    pub code: String,
}

/// Experimental Metal Shading Language source output.
pub struct MetalTextOutput {
    /// Complete generated Metal Shading Language source.
    pub code: String,
    /// Optional GPU reflection sidecar generated from kernel ABI/layout facts.
    pub reflection: Option<crate::mir::MirGpuReflection>,
}

/// Experimental WGSL compute-shader source output.
pub struct WgslTextOutput {
    /// Complete generated WGSL source.
    pub code: String,
    /// Optional GPU reflection sidecar generated from kernel ABI/layout facts.
    pub reflection: Option<crate::mir::MirGpuReflection>,
}

/// Experimental Racket s-expression output.
pub struct SexpOutput {
    /// Complete generated Racket source.
    pub code: String,
}

/// Stable library entry point for compiling Faber source.
///
/// A compiler owns one [`Session`] so repeated calls share the same target and
/// diagnostic policy. It does not own package discovery or import graph loading;
/// callers that need package semantics should assemble source through the
/// higher-level `faber` package layer.
pub struct Compiler {
    session: Session,
}

impl Compiler {
    /// Create a compiler configured for one target and diagnostic policy.
    #[must_use]
    // public constructor: moves config into session
    pub fn new(config: Config) -> Self {
        Self { session: Session::new(config) }
    }

    /// Compile a file from disk.
    ///
    /// File I/O errors are reported through [`CompileResult::diagnostics`] so
    /// callers can handle all compile failures through the same diagnostic
    /// channel.
    #[must_use]
    pub fn compile(&self, path: &Path) -> CompileResult {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                return CompileResult { output: None, diagnostics: vec![Diagnostic::io_error(path, &e)] };
            }
        };
        let name = path.display().to_string();
        self.compile_str(&name, &source)
    }

    /// Compile source code from a string.
    ///
    /// This is the core in-memory path used by tests, tools, and file-based
    /// compilation after source loading. `name` is diagnostic identity only; it
    /// does not imply filesystem-backed package behavior.
    #[must_use]
    pub fn compile_str(&self, name: &str, source: &str) -> CompileResult {
        driver::compile(&self.session, name, source)
    }
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
