//! Core compiler library for Faber.
//!
//! `radix-module` owns the module-compiler pipeline: the driver (phase
//! ordering and target dispatch), MIR lowering, HIR-direct codegen dispatch,
//! services, and the façade modules that re-export the extracted leaves.
//! Semantic analysis (collection, resolution, HIR lowering, typecheck,
//! post-typecheck passes) lives in the `radix-semantic` leaf; the lexer,
//! parser, syntax, types, HIR, diagnostics, intrinsics, target-identity, and
//! backend emitters live in their own leaves, re-exported here for the
//! historical `crate::*` paths. The user-facing `faber` crate builds package
//! and project workflows on top of this library; the developer `radix` binary
//! lives in `radix-cli`. Package policy, manifest loading, and generated
//! Cargo layouts deliberately live there instead of here.
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
//!   compiler backend used by `faber format` and the developer CLI.
//! - CLI and library entry points both go through `driver` so diagnostics and
//!   phase ordering remain consistent.
//!
//! ```no_run
//! use radix_module::{Compiler, Config, Target};
//! use std::path::Path;
//!
//! let config = Config::default().with_target(Target::HirRust);
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
#[cfg(feature = "device")]
pub mod air;
pub use radix_lexer::ascii_literal;
pub mod aspect;
#[cfg(any(test, feature = "test-support"))]
pub mod backend_smoke;
pub use radix_semantic::builtins;
pub mod cli;
pub mod cli_descriptor;
pub mod codegen;
#[cfg(feature = "mir-metal")]
pub mod device_export;
pub mod diagnostics;
pub mod driver;
pub mod file_interface;
pub mod hir;
pub(crate) mod import_resolve;
pub use import_resolve::contains_path_traversal;
pub mod intrinsics;
pub mod kernel;
pub mod lexer;
pub mod live_tuus_cursor;
#[cfg(feature = "mir-llvm")]
pub mod llvm_host;
pub mod locale;
#[cfg(feature = "mir-core")]
pub mod mir;
pub mod tensor_goal_gate;
pub use radix_lexer::octeti_literal;
pub mod parser;
pub mod program;
pub mod runtime_contract;
pub mod semantic;
pub mod services;
pub mod syntax;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
pub use radix_lexer::textus_literal;

use std::path::Path;

pub use codegen::Target;
pub use diagnostics::{Diagnostic, DiagnosticConvert, is_known_diagnostic_code};
pub use driver::{Config, ModuleCompileRequest, Session, WarnPolicy, apply_warn_policy};

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
    /// Experimental EVM deployment and runtime bytecode.
    #[cfg(feature = "mir-evm")]
    Evm(EvmOutput),
    /// Rust backend output, currently the primary executable target.
    Rust(RustOutput),

    /// TypeScript backend output.
    TypeScript(TypeScriptOutput),

    /// Go backend output.
    Go(GoOutput),

    /// Swift backend output.
    Swift(SwiftOutput),

    /// Python backend output.
    Python(PythonOutput),

    /// Haskell backend output.
    Haskell(HaskellOutput),

    /// Novelty Free Pascal backend output (default-off `hir-pascal` feature).
    #[cfg(feature = "hir-pascal")]
    Pascal(PascalOutput),

    /// Novelty `GnuCOBOL` free-format output emitted from checked HIR.
    #[cfg(feature = "hir-cobol")]
    Cobol(CobolOutput),

    /// Novelty Fortran backend output (default-off `hir-fortran` feature).
    #[cfg(feature = "hir-fortran")]
    Fortran(FortranOutput),

    /// Novelty Odin backend output (default-off `hir-odin` feature).
    #[cfg(feature = "hir-odin")]
    Odin(OdinOutput),

    /// Novelty C backend output (default-off `hir-c` feature).
    #[cfg(feature = "hir-c")]
    C(COutput),

    /// Canonical Faber source output.
    Faber(FaberOutput),

    /// Experimental WebAssembly text output emitted from validated MIR.
    /// Binary conversion is an external-tool step (`wat2wasm` /
    /// `wasm-tools parse`).
    #[cfg(feature = "mir-wasm")]
    WasmText(WasmTextOutput),

    /// Experimental LLVM text output emitted from validated MIR.
    #[cfg(feature = "mir-llvm")]
    LlvmText(LlvmTextOutput),

    /// Native object file emitted from validated MIR by the Cranelift backend.
    #[cfg(feature = "mir-clif")]
    ClifObject(ClifObjectOutput),

    /// A program compiled into this process's memory by the Cranelift JIT
    /// engine (`clif-jit`); produced instead of [`Output::ClifObject`] when the
    /// configuration selects [`ClifEngine::Jit`].
    #[cfg(feature = "mir-clif")]
    ClifJit(ClifJitOutput),

    /// Experimental Metal Shading Language source emitted from validated MIR.
    #[cfg(feature = "mir-metal")]
    MetalText(MetalTextOutput),

    /// Experimental WGSL compute-shader source emitted from validated MIR.
    #[cfg(feature = "mir-wgsl")]
    WgslText(WgslTextOutput),

    /// Experimental Racket s-expression output emitted from validated MIR.
    #[cfg(feature = "mir-sexp")]
    Sexp(SexpOutput),

    /// Novelty classic line-numbered BASIC listing emitted from validated MIR.
    #[cfg(feature = "mir-basic")]
    Basic(BasicOutput),
}

/// Experimental Shanghai bytecode, with the temporary raw-word evm_entry interface.
#[cfg(feature = "mir-evm")]
pub struct EvmOutput {
    /// Deployment bytecode that returns the runtime.
    pub init: Vec<u8>,
    /// Runtime bytecode for inspection and independent host verification.
    pub runtime: Vec<u8>,
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

/// Python backend output.
pub struct PythonOutput {
    /// Complete generated Python source.
    pub code: String,
}

/// Novelty Fortran backend output.
#[cfg(feature = "hir-fortran")]
pub struct FortranOutput {
    /// Complete generated Fortran 2008 program.
    pub code: String,
}

/// Novelty Odin backend output.
#[cfg(feature = "hir-odin")]
pub struct OdinOutput {
    /// Complete generated Odin program.
    pub code: String,
}

/// Novelty C backend output.
#[cfg(feature = "hir-c")]
pub struct COutput {
    /// Complete generated C11 translation unit.
    pub code: String,
}

/// Haskell backend output.
pub struct HaskellOutput {
    /// Complete generated Haskell source.
    pub code: String,
}

/// Novelty Free Pascal backend output.
#[cfg(feature = "hir-pascal")]
pub struct PascalOutput {
    /// Complete generated Free Pascal program.
    pub code: String,
}

/// Novelty COBOL backend output.
#[cfg(feature = "hir-cobol")]
pub struct CobolOutput {
    /// Complete generated `GnuCOBOL` free-format source.
    pub code: String,
}

/// Canonical Faber source output.
#[derive(Debug)]
pub struct FaberOutput {
    /// Complete re-emitted Faber source.
    pub code: String,
}

/// Experimental WebAssembly text output.
#[cfg(feature = "mir-wasm")]
pub struct WasmTextOutput {
    /// Complete generated WAT source.
    pub code: String,
}

/// Experimental LLVM text output.
#[cfg(feature = "mir-llvm")]
pub struct LlvmTextOutput {
    /// Complete generated LLVM text source.
    pub code: String,
}

/// Native object output of the public Cranelift target.
#[cfg(feature = "mir-clif")]
pub struct ClifObjectOutput {
    /// Relocatable object file bytes for the host triple.
    pub bytes: Vec<u8>,
}

/// A program compiled into this process's memory.
#[cfg(feature = "mir-clif")]
pub struct ClifJitOutput {
    /// The compiled image; keep it alive while the program runs.
    pub program: radix_mir_clif::JitProgram,
}

/// How the Cranelift target (`clif`) turns the lowered program into code.
///
/// The lowering and the capability pre-pass are the same for both engines;
/// only the module the functions are defined in differs.
#[cfg(feature = "mir-clif")]
#[derive(Debug, Clone, Default)]
pub enum ClifEngine {
    /// Write a relocatable object for the system linker (`faber build`).
    #[default]
    Object,
    /// Compile in memory and hand the image back (`faber run --target clif-jit`).
    Jit(ClifRuntimeTable),
}

/// The runtime's `extern "C"` entry points a JIT image resolves its imports
/// from: each C name with its address.
#[cfg(feature = "mir-clif")]
#[derive(Debug, Clone, Default)]
pub struct ClifRuntimeTable(std::sync::Arc<[(&'static str, usize)]>);

#[cfg(feature = "mir-clif")]
impl ClifRuntimeTable {
    /// A table of `(C name, address)` entries.
    #[must_use]
    pub fn new(symbols: Vec<(&'static str, *const u8)>) -> Self {
        Self(
            symbols
                .into_iter()
                .map(|(name, address)| (name, address as usize))
                .collect(),
        )
    }

    /// The entries as the JIT lowering takes them.
    #[must_use]
    pub fn symbols(&self) -> Vec<(&'static str, *const u8)> {
        self.0
            .iter()
            .map(|(name, address)| (*name, *address as *const u8))
            .collect()
    }
}

/// Experimental Metal Shading Language source output.
#[cfg(feature = "mir-metal")]
pub struct MetalTextOutput {
    /// Complete generated Metal Shading Language source.
    pub code: String,
    /// Optional GPU reflection sidecar generated from kernel ABI/layout facts.
    pub reflection: Option<crate::mir::MirGpuReflection>,
}

/// Experimental WGSL compute-shader source output.
#[cfg(feature = "mir-wgsl")]
pub struct WgslTextOutput {
    /// Complete generated WGSL source.
    pub code: String,
    /// Optional GPU reflection sidecar generated from kernel ABI/layout facts.
    pub reflection: Option<crate::mir::MirGpuReflection>,
}

/// Experimental Racket s-expression output.
#[cfg(feature = "mir-sexp")]
pub struct SexpOutput {
    /// Complete generated Racket source.
    pub code: String,
}

/// Novelty BASIC listing output.
#[cfg(feature = "mir-basic")]
pub struct BasicOutput {
    /// Complete generated line-numbered BASIC listing.
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
    /// This is a program-layer entry: it builds import contracts from the path,
    /// then calls fail-closed module analysis. File I/O errors are reported
    /// through [`CompileResult::diagnostics`] so callers can handle all compile
    /// failures through the same diagnostic channel.
    #[must_use]
    pub fn compile(&self, path: &Path) -> CompileResult {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                return CompileResult { output: None, diagnostics: vec![Diagnostic::io_error(path, &e)] };
            }
        };
        crate::program::compile_prepared_program_path(&self.session, path, &source)
    }

    /// Compile source code from a string.
    ///
    /// This is the core in-memory path used by tests, tools, and file-based
    /// compilation after source loading. `name` is diagnostic identity only; it
    /// does not imply filesystem-backed package behavior.
    #[must_use]
    pub fn compile_str(&self, name: &str, source: &str) -> CompileResult {
        driver::compile(&self.session, name, source, ModuleCompileRequest::default())
    }
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;

#[cfg(test)]
#[path = "parse_probe_test.rs"]
mod parse_probe_test;

#[cfg(test)]
#[path = "reduction_words_pack_test.rs"]
mod reduction_words_pack_test;
