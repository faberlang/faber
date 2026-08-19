//! Compiler-owned receiver intrinsic registry — re-exported from leaf `radix-intrinsics`.
//!
//! The catalog itself (receiver families, param/return shapes, promoted rows,
//! lookups, predicates, MIR classification accessors) lives in the neutral
//! `radix-intrinsics` crate on top of `radix-types`. This module keeps the
//! `crate::intrinsics` path stable for semantic typecheck, lint, locale
//! surfaces, and MIR lowering, and hosts the driver-integration tests that
//! prove the catalog through the full pipeline (pure catalog invariants are
//! proven in the leaf).

pub use radix_intrinsics::{
    is_atomic_operator_method, is_numeric_bitwise_operator, is_numeric_imperativus_operator,
    lookup_method, lookup_method_for_type, mir_atomic_op, mir_collection_op, mir_is_higher_order_lista,
    receivers_for_type, IntrinsicMirKind, IntrinsicParamKind, IntrinsicReceiver, IntrinsicReturnKind,
    MethodIntrinsic, INTRINSICS,
};

#[cfg(test)]
#[path = "approximata_test.rs"]
mod approximata_tests;

#[cfg(test)]
#[path = "operator_methods_test.rs"]
mod operator_methods_tests;

#[cfg(test)]
#[path = "registry_test.rs"]
mod registry_tests;
