//! The process exit status an entry block hands back.
//!
//! `main exit <expr> { … }` lowers to an entry block whose tail expression is
//! the exit status: the body runs, then the process ends with that code. A
//! backend that runs programs ends the process with it. This is the one
//! definition of when a tail expression is an exit status, so every target
//! agrees with the MIR lowering and the runner: only a `numerus` tail is one,
//! and any other tail is not an exit.

use radix_hir::{HirBlock, HirExpression};
use radix_types::{Primitive, TypeTable};

/// The entry block's exit-status expression, when it has one.
#[must_use]
pub fn entry_exit_expr<'a>(entry: &'a HirBlock, types: &TypeTable) -> Option<&'a HirExpression> {
    entry
        .expr
        .as_deref()
        .filter(|expr| expr.ty == Some(types.primitive(Primitive::Numerus)))
}
