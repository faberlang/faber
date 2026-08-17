//! Pretty-v1 layout engine for the `pretty-v1` format policy (FORMAT-PRETTY S3).
//!
//! Rule delivery (delivery §S3): blocks & indentation (one statement per line,
//! 4-space indent constant, braces attached, non-cuddled `si`/`sin`/`secus` and
//! `fac`/`cape` chains, compact one-statement arms, compact empty blocks),
//! width-aware wrapping at syntactic boundaries only, bounded blank lines and
//! comment preservation, binding/operator spacing normalization, and the
//! line-sensitive grammar guard.
//!
//! ## Shared layout module (locale/HIR contract)
//!
//! The layout rules must apply to both the author (AST + trivia) path and the
//! `--locale` HIR re-emit path via one shared layout module. The author path is
//! the primary consumer: this module implements it fully. The HIR re-emit path
//! (`radix-hir-faber`, the engine behind `faber format --locale <X>`) lives in
//! a foundation leaf crate that cannot depend on the monolith, so **it cannot
//! share this module in v1** — recorded gap. Consequences:
//!
//! - Locale changes keyword spelling, never wrapping/indentation; the author
//!   path already honors reader-locale lex packs for input, and `pretty-v1`
//!   emits the same Latin code surface as `normalise-v1`.
//! - CTO fire-16: `--policy normalise-v1 --locale en` must never silently
//!   switch the author normalizer to canonical HIR re-emission. `normalise-v1`
//!   always runs the exact author path (byte-identical contract); localized
//!   HIR re-emission is a separate entry (`compile_canonical`) with no policy
//!   routing, so no combination can silently cross over. When a combination
//!   cannot be honored (pretty layout on the HIR re-emit path), the engine
//!   rejects it explicitly rather than falling back.
//!
//! ## Guard
//!
//! Constructs the printer cannot preserve confidently (compact `ergo` bodies
//! over block-owning statements, lossy annotation tokens) leave the file
//! unchanged, report the path/location, and fail in `--check` mode — a partial
//! rewrite is worse than an unformatted file.

mod emit;
mod guard;
mod layout;
mod trivia;

pub use emit::{emit, PrettyEmitter};
pub use guard::{find_guard_hits, token_is_preservable, GuardKind, GuardReport};
pub use layout::{LineIndex, PrettyWriter, INDENT_WIDTH, SOFT_WIDTH};
