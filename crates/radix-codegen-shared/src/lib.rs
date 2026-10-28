//! Shared text-emit utilities for Radix codegen and MIR text backends.
//!
//! This crate is the first workspace leaf of the radix crate DAG. It must never
//! depend on `radix-semantic`, `radix-parser`, or `radix-module`, so any
//! backend can use it without pulling the pipeline. `radix-hir` is an admitted
//! input surface: the `Codegen` trait takes `&HirModule`, and `names.rs` /
//! `validation.rs` walk HIR with `HirVisitor`. The crate must never construct,
//! lower, or re-derive HIR.
//!
//! # Contents
//!
//! The crate owns the frame/loop-branch builders and ident helpers the device
//! emitters share. Device-specific emission belongs in the device emitters, so
//! no Metal-only helper and no `dyn Emitter` / stringly target façade enters
//! this crate. This is the standing accretion law from the CTO b4b goal-close
//! record `eda0566c` (disposition B4B-7/8: `record_risk`); `frame`'s module
//! docs are the admission test for the next helper.
//!
//! # `CodeWriter`
//!
//! Backends emit target source incrementally while walking IR. [`CodeWriter`]
//! owns indentation state, line-start detection, and scoped block helpers.
//!
//! The writer deliberately stays text-oriented. It does not understand target
//! syntax, comments, or tokens; backend modules still decide what to write. Its
//! only policy is when indentation is inserted and how nested formatting scopes
//! are balanced.
//!
//! # Invariants
//!
//! - Indentation is emitted only before the first non-newline character on a
//!   line.
//! - `indent` and `dedent` affect future line starts, not text already written
//!   on the current line.
//! - Scoped helpers restore indentation depth after their closure returns.
//! - Finalization consumes the writer so no later writes can silently diverge
//!   from the returned source string.

pub mod bits_gate;
pub mod breakable;
pub mod cli_ir;
pub mod conversio_arm;
pub mod decimal;
pub mod frame;
pub mod hints;
pub mod hygiene;
pub mod ident;
pub mod leaf_gate;
pub mod ledger_gate;
pub mod library_module;
pub mod list_window_gate;
pub mod lista_lift_gate;
pub mod names;
pub mod ranged_int_gate;
pub mod runtime_api;
pub mod tensor_lift_gate;
pub mod terminates;
pub mod text_literal;
pub mod unbounded_gate;
pub mod unsupported;
pub use text_literal::unescape_faber_text;
pub mod validation;

use radix_diagnostics::DiagnosticArg;
use radix_hir::HirModule;
use radix_lexer::Interner;
use radix_types::TypeTable;

/// Reader-locale intrinsic spelling projected into a target emitter.
///
/// The locale pack owns the mapping from canonical registry names to reader
/// spellings; codegen leaves consume this one narrow contract when installed.
pub trait IntrinsicSurface {
    /// Surface spelling for one canonical intrinsic name, or `None` when the
    /// adapter has no row for it.
    fn intrinsic_surface(&self, canonical: &str) -> Option<String>;
}

/// A [`CodeWriter`] position captured by [`CodeWriter::mark`].
#[derive(Clone, Copy, Debug)]
pub struct WriterMark {
    len: usize,
    indent: usize,
    at_line_start: bool,
}

/// Indentation-aware code writer.
///
/// The struct is intentionally small and backend-agnostic. It provides enough
/// structure for Rust, TypeScript, Go, Faber, and the temporary MIR Rust probe
/// to share formatting mechanics without coupling them to one target's syntax.
pub struct CodeWriter {
    buffer: String,
    indent: usize,
    indent_str: &'static str,
    at_line_start: bool,
}

impl CodeWriter {
    /// Create an empty writer using four-space indentation.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            indent: 0,
            indent_str: "    ",
            at_line_start: true,
        }
    }

    /// Finish emission and return the accumulated source text.
    #[must_use]
    pub fn finish(self) -> String {
        self.buffer
    }

    /// Borrow the accumulated source text without finishing emission.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.buffer
    }

    /// Capture the writer position so a speculative emission can be undone.
    #[must_use]
    pub fn mark(&self) -> WriterMark {
        WriterMark {
            len: self.buffer.len(),
            indent: self.indent,
            at_line_start: self.at_line_start,
        }
    }

    /// Discard everything written since `mark` and restore its indentation.
    pub fn rewind(&mut self, mark: WriterMark) {
        self.buffer.truncate(mark.len);
        self.indent = mark.indent;
        self.at_line_start = mark.at_line_start;
    }

    /// Write text verbatim without applying block indentation on embedded newlines.
    ///
    /// Use for literal payloads (`«…»`, `` `…` ``) whose interior newlines must
    /// not pick up the surrounding block indent level.
    pub fn write_raw(&mut self, s: &str) {
        self.buffer.push_str(s);
        self.at_line_start = s.ends_with('\n');
    }

    /// Write text while applying indentation at each line start.
    ///
    /// Newlines do not themselves receive indentation. The next non-newline
    /// character triggers the pending indentation, which avoids trailing spaces
    /// on intentionally blank lines.
    pub fn write(&mut self, s: &str) {
        for c in s.chars() {
            if c == '\n' {
                self.buffer.push('\n');
                self.at_line_start = true;
            } else {
                if self.at_line_start {
                    for _ in 0..self.indent {
                        self.buffer.push_str(self.indent_str);
                    }
                    self.at_line_start = false;
                }
                self.buffer.push(c);
            }
        }
    }

    /// Write preformatted [`std::fmt::Arguments`] through the same line policy.
    pub fn writef(&mut self, args: std::fmt::Arguments<'_>) {
        let s = format!("{args}");
        self.write(&s);
    }

    /// Write a line (string + newline).
    pub fn writeln(&mut self, s: &str) {
        self.write(s);
        self.write("\n");
    }

    /// Write an empty line.
    pub fn newline(&mut self) {
        self.write("\n");
    }

    /// End the current line unless the writer already sits at a line start.
    pub fn ensure_line_start(&mut self) {
        if !self.at_line_start {
            self.newline();
        }
    }

    /// Increase indentation for subsequent lines.
    pub fn indent(&mut self) {
        self.indent += 1;
    }

    /// Decrease indentation for subsequent lines.
    ///
    /// Saturation keeps formatting helpers from underflowing after a mismatched
    /// manual `dedent`. Structural correctness still belongs to the caller; the
    /// writer only guarantees it will not panic while recovering.
    pub fn dedent(&mut self) {
        if self.indent > 0 {
            self.indent -= 1;
        }
    }

    /// Run a closure with one extra indentation level.
    ///
    /// This is the preferred API for nested emission because it makes the
    /// formatting scope local to the code that writes the nested body.
    pub fn indented<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.indent();
        f(self);
        self.dedent();
    }

    /// Write a brace-delimited block with a scoped indented body.
    ///
    /// The helper is intentionally punctuation-only: it fits Rust, TypeScript,
    /// and Go-style blocks, while targets with different delimiters can compose
    /// `writeln`, `indented`, and `write` directly.
    pub fn block<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.writeln("{");
        self.indented(f);
        self.write("}");
    }
}

impl Default for CodeWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Formatted-writing shorthand for [`CodeWriter`].
///
/// Backend code uses this macro at call sites where standard `write!` syntax
/// reads better than constructing `format_args!` manually, while still routing
/// through [`CodeWriter::write`] for line-start indentation.
#[macro_export]
macro_rules! write_code {
    ($w:expr_2021, $($arg:tt)*) => {
        $w.writef(format_args!($($arg)*))
    };
}

/// Backend emission error after semantic analysis has completed.
///
/// Codegen errors are intentionally narrower than semantic diagnostics. They
/// represent target gaps, unsupported lowering combinations, or internal
/// pipeline violations discovered while emitting source. User-facing type and
/// name errors should be reported before this boundary whenever possible.
#[derive(Debug)]
pub struct CodegenError {
    /// Human-readable emission failure suitable for CLI diagnostic wrapping.
    pub message: String,
    /// Stable machine-readable facts for tests, tools, and rendered diagnostics.
    pub args: Vec<DiagnosticArg>,
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CodegenError {}

/// Target backend contract for HIR-to-source emission.
///
/// Implementors receive the same semantic inputs: HIR for structure, the
/// [`TypeTable`] for resolved type decisions, and the [`Interner`] for
/// recovering source symbols. The output type stays associated so specialized
/// backends can return richer artifacts, while the public [`generate`] router
/// wraps those artifacts into [`crate::Output`].
///
/// # Errors
///
/// Returns [`CodegenError`] when a target gap, unsupported lowering
/// combination, or internal pipeline violation is discovered during emission.
pub trait Codegen {
    type Output;

    /// Run the backend to produce target source from semantic inputs.
    ///
    /// # Errors
    ///
    /// Returns [`CodegenError`] when a target gap, unsupported lowering
    /// combination, or internal pipeline violation is discovered during
    /// emission.
    fn generate(
        &self,
        hir: &HirModule,
        types: &TypeTable,
        interner: &Interner,
    ) -> Result<Self::Output, CodegenError>;
}

pub use bits_gate::{
    is_byte_text_conversion, list_text_conversion, list_width_change, reject_bits_type,
    reject_list_text_conversion, reject_list_width_decode,
};
pub use ident::{
    SCALAR_LOOP_CANDIDATES, SCALAR_LOOP_FALLBACK, VALUE_JOIN_CANDIDATES, VALUE_JOIN_FALLBACK,
    output_resource_name, sanitize_ident, sanitize_rust_module_ident, unclaimed_name,
};
pub use leaf_gate::{collect_ad_routes, reject_unsupported_leaf};
pub use ledger_gate::reject_ledger_type;
pub use list_window_gate::reject_list_window;
pub use ranged_int_gate::{
    reject_ranged_int_type, reject_ranged_list_type, reject_unlowered_value_types,
};
pub use tensor_lift_gate::reject_tensor_lift;
pub use unbounded_gate::{big_literal_unsupported, reject_unbounded_integer};
pub use validation::{find_error_expr_in_program, reject_hir_errors};

#[cfg(test)]
#[path = "writer_test.rs"]
mod writer_tests;
