//! Faber runtime package copy of the Radix-owned host ABI status/symbol
//! constants the runtime carriers reference.
//!
//! Authority: `radix-host-abi` (the compiler-owned C ABI authority; the
//! `FaberRtStatusV1` status codes and the `__faber_rt_v1_*` symbol names).
//! The runtime package is a standalone public crate, so the ABI status codes
//! and the cursor-stream symbol are committed here as faithful copies of the
//! authority values.
//!
//! KEEP IN SYNC: `radix/crates/radix-host-abi` `STATUS_*` code rows and
//! `SYMBOL_CURSOR_STREAM`.

/// Stable C ABI status carrier (mirror of `radix-host-abi`
/// `FaberRtStatusV1`): `code` is the status discriminator.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaberRtStatusV1 {
    pub code: i32,
}

impl FaberRtStatusV1 {
    #[must_use]
    pub const fn is_ok(self) -> bool {
        self.code == STATUS_OK.code
    }
}

/// Status 0 — the happy path (success payload follows).
pub const STATUS_OK: FaberRtStatusV1 = FaberRtStatusV1 { code: 0 };

/// Status 5 — the failable error channel (typed `ReturnError` payload).
pub const STATUS_FALLIBLE: FaberRtStatusV1 = FaberRtStatusV1 { code: 5 };

/// Cursor-stream materialization host-ABI symbol (`__faber_rt_v1_cursor_stream`).
pub const SYMBOL_CURSOR_STREAM: &str = "__faber_rt_v1_cursor_stream";
