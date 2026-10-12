//! Native effect leaves for Faber.
//!
//! The interpreter (`faber run`) and generated Rust (`faber build`) share
//! this crate. N2 discovery unit: the fs leaf only. Other leaves stay out.

mod error;
pub mod fs;

pub use error::{FsError, FsErrorKind};
