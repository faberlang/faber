//! Lexical front end — re-exported from foundation crate `radix-lexer`.
//!
//! Pack-aware lexing uses [`radix_lexer::KeywordPack`]. `LocalePack`
//! implements that trait in `locale`.

pub use radix_lexer::*;

// Residual unit tests that need monolith-only fixtures stay here when present.
#[cfg(test)]
#[path = "scan_residual_test.rs"]
mod scan_residual_test;

// Re-export for parser residual tests that assert the inline-comment issue id.
pub use radix_lexer::INLINE_COMMENT_ISSUE;
