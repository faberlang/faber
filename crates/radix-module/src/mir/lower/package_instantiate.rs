//! Shim: the `package_instantiate` lowering module moved to `radix-mir-lower`;
//! this copy hosts its companion test so the test's module path and `#[path]`
//! stay unchanged.

pub use radix_mir_lower::lower::package_instantiate::*;

// The moved file's import scope, re-exported for its companion test.
pub use super::*;
pub use radix_mir::generic::{
    MirShapeBindings, bind_generic_type_witness_with_shapes, canonical_index, function_contains_shape_size,
    instantiate_existing_generic_type, instantiate_generic_type_with_shapes, substitute_shape_size_constants,
    substitute_tensor_from_flat_shape_size_constants,
};
pub use radix_semantic::file_interface::{
    FileExportKind, FileInterface, InterfaceDeviceContract, InterfaceDeviceRole, InterfaceDeviceVisibility,
};
pub use std::collections::HashMap;

#[cfg(test)]
#[path = "package_instantiate_test.rs"]
mod tests;
