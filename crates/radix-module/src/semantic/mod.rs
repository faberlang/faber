//! Semantic analysis — re-exported from leaf `radix-semantic`.
//!
//! Production code lives in `radix-semantic`. This module keeps the
//! `crate::semantic::*` path stable. Driver/codegen integration tests that
//! cannot live in the leaf stay hosted here.

pub use crate::tensor_goal_gate;
pub use radix_semantic::*;

#[cfg(test)]
#[path = "g2_policy_test.rs"]
mod g2_policy_test;

#[cfg(test)]
#[path = "def_id_closure_emit_test.rs"]
mod def_id_closure_emit_test;

#[cfg(test)]
#[path = "pipeline_remainder_test.rs"]
mod pipeline_remainder_test;

#[cfg(test)]
#[path = "passes/mod.rs"]
pub mod passes;
