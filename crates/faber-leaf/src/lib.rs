//! Native effect leaves shared by the interpreter (`faber run`) and generated
//! Rust (`faber build`).
//!
//! Nine leaves, one route prefix each: `fs` (`solum:`), `stdio` (`consolum:`),
//! `process` (`processus:`), `clock` (`tempus:`), `entropy` (`aleator:`),
//! `tcp` (`caelum:`), `http` (`http:`), `machine` (`machina:`), and
//! `websocket` (`websocket:`).
//!
//! `LEAVES` is the single registration, and both `is_route` and `serve` read
//! it, so a leaf cannot be answerable for one and missing from the other.

mod error;
mod sha256;
mod value;

pub mod clock;
pub mod entropy;
pub mod fs;
pub mod http;
pub mod machine;
pub mod process;
pub mod stdio;
pub mod tcp;
pub mod websocket;

pub use error::{FsError, FsErrorKind, LeafError, LeafErrorKind};
pub use value::{Arg, Reply};

/// Name test a leaf exposes; a pure lookup that runs no effect.
type LeafIsRoute = fn(&str) -> bool;

/// Dispatcher a leaf exposes; `None` when the route is not that leaf's.
type LeafServe = fn(&str, &[Arg]) -> Option<Result<Reply, LeafError>>;

/// Every shared leaf, in dispatch order.
const LEAVES: &[(LeafIsRoute, LeafServe)] = &[
    (fs::is_route, fs::serve),
    (stdio::is_route, stdio::serve),
    (process::is_route, process::serve),
    (clock::is_route, clock::serve),
    (entropy::is_route, entropy::serve),
    (tcp::is_route, tcp::serve),
    (http::is_route, http::serve),
    (machine::is_route, machine::serve),
    (websocket::is_route, websocket::serve),
];

/// Whether any shared leaf serves `route`.
#[must_use]
pub fn is_route(route: &str) -> bool {
    LEAVES.iter().any(|(is_route, _)| is_route(route))
}

/// Answer a shared-leaf route. `None` means no leaf owns it.
pub fn serve(route: &str, args: &[Arg]) -> Option<Result<Reply, LeafError>> {
    LEAVES.iter().find_map(|(_, serve)| serve(route, args))
}

#[cfg(test)]
#[path = "go_registry_test.rs"]
mod go_registry_test;

#[cfg(test)]
mod test_support;

/// Serializes tests that mutate shared process-global leaf state.
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Reset process-global leaf state that must not cross program executions.
pub fn reset_execution_state() {
    entropy::reset_seed();
    process::reset_argv();
    tcp::reset_handles();
    http::reset_handles();
    machine::reset_previous();
    websocket::reset_handles();
}
