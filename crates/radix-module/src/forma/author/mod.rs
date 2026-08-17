//! Author-mode Faber formatting from AST + leading trivia.
//!
//! Re-emits Faber source from the parsed AST, preserving `#` comments and blank
//! lines attached via [`crate::syntax::Trivia`]. Canonical spelling helpers are
//! shared where AST and HIR shapes align.

mod emit;
mod layout;
mod trivia;

pub use emit::{emit, AuthorEmitter};
pub use layout::{normalize_leading_trivia, top_level_trivia_scope, TriviaScope};
