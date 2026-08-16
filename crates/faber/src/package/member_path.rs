//! Package-contained manifest path resolution (forwarded to `radix-package`).
//!
//! RP2b: the manifest-relative path security boundary is compiler-owned and
//! lives in `radix-package`; this module forwards the moved surface so faber
//! package consumers keep their existing import paths.

pub(crate) use radix_package::member_path::resolve_package_member;
