//! Native effect leaves for Faber.
//!
//! The interpreter (`faber run`) and generated Rust (`faber build`) share
//! this crate. N2: fs, stdio, process, clock, and entropy. HTTP stays out
//! until N5. Nothing is restored from `hosts`.

mod error;
mod value;

pub mod clock;
pub mod entropy;
pub mod fs;
pub mod process;
pub mod stdio;

pub use error::{FsError, FsErrorKind, LeafError, LeafErrorKind};
pub use value::{Arg, Reply};

/// Whether any shared leaf serves `route`.
#[must_use]
pub fn is_route(route: &str) -> bool {
    fs::is_route(route)
        || stdio::is_route(route)
        || process::is_route(route)
        || clock::is_route(route)
        || entropy::is_route(route)
}

/// Answer a shared-leaf route. `None` means no leaf owns it.
pub fn serve(route: &str, args: &[Arg]) -> Option<Result<Reply, LeafError>> {
    if let Some(reply) = fs::serve(route, args) {
        return Some(reply);
    }
    if let Some(reply) = stdio::serve(route, args) {
        return Some(reply);
    }
    if let Some(reply) = process::serve(route, args) {
        return Some(reply);
    }
    if let Some(reply) = clock::serve(route, args) {
        return Some(reply);
    }
    entropy::serve(route, args)
}

/// Reset process-global leaf state that must not cross program executions.
pub fn reset_execution_state() {
    entropy::reset_seed();
    process::reset_argv();
}
