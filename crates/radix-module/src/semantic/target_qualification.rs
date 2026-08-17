//! Re-export surface for target qualification types.
//!
//! Qualification lives beside the analyzed-application facade so G5 can grow a
//! richer classifier without forking the G1 snapshot contract.

#[allow(unused_imports)] // public re-export surface for G5 consumers
pub use super::analyzed_application::{qualify_for_target, QualificationStatus, TargetQualification};
