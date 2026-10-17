//! C-layout ABI shapes shared by the Cranelift leaf route.
//!
//! The generated program obtains one fault-slot pointer from the runtime symbol
//! `__faber_clif_v1_leaf_fault_slot`, passes it as the last argument of every
//! leaf shim call, and reads the slot only between that call and the next. Faber
//! programs on this target are single-threaded, so one runtime-owned slot is
//! sound.

/// A raw pointer and explicit byte length, with no header at the pointer.
///
/// Use this shape when the callee reads the argument by its explicit length.
#[repr(C)]
pub struct FaberClifRawSlice {
    /// Pointer to the first byte; it is not preceded by a length header.
    pub ptr: *const u8,
    /// Number of bytes available at `ptr`.
    pub len: i64,
}

/// A pointer to a CLIF ABI text handle with a length header.
///
/// The pointer addresses `[length: i64][bytes...]`; the length occupies the
/// first eight bytes, as specified by `TEXT_HEADER` in the CLIF ABI. This is a
/// distinct type from [`FaberClifRawSlice`], whose pointer has no header.
#[repr(C)]
pub struct FaberClifTextHandle {
    /// Pointer to the eight-byte length header followed by text bytes.
    pub ptr: *const u8,
}

/// Fault data written through the CLIF leaf fault out-slot.
///
/// A zero `code` means success. A nonzero `code` is the numeric suffix of the
/// registered `ERRnnn` for the `IoFault` family; it is never a platform message
/// string. The `kind` field contains the Norma `IoFault` variant word, and
/// `path` contains the route path fact. Both are raw slices with explicit
/// lengths and no text-handle header.
#[repr(C)]
pub struct FaberClifLeafFault {
    /// Zero on success; otherwise the numeric suffix of the registered code.
    pub code: i32,
    /// Norma `IoFault` variant word as an unheadered byte slice.
    pub kind: FaberClifRawSlice,
    /// Route path fact as an unheadered byte slice.
    pub path: FaberClifRawSlice,
}

#[cfg(test)]
#[path = "clif_shapes_test.rs"]
mod tests;
