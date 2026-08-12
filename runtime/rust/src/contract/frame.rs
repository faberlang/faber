//! Faber runtime package copy of the Radix-owned frame status contract: the
//! lifecycle states of a `sermo` (conversation) frame in the `ad`
//! bridge/gateway architecture.
//!
//! Authority: `radix-runtime-contract/src/frame.rs` (the compiler-side
//! authority). The runtime package is a standalone public crate, so the
//! contract material is committed here as a faithful copy. The seven variants
//! and their terminal/content classification are also encoded in
//! `radix::builtins::frame_types::STATUS_VARIANTS` and emitted by
//! `radix::codegen::frame_shim` for every target backend.
//!
//! KEEP IN SYNC: `radix-runtime-contract/src/frame.rs` and
//! `radix::builtins::frame_types::STATUS_VARIANTS`.

/// Frame lifecycle status for stream `ad` conversations.
///
/// Every variant corresponds to a Faber-language `status` variant. The variant
/// order matches the canonical `STATUS_VARIANTS` list:
/// `request, item, byte, bulk, done, error, cancel`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameStatus {
    Request,
    Item,
    Byte,
    Bulk,
    Done,
    Error,
    Cancel,
}

impl FrameStatus {
    /// Terminal statuses end the inbound direction of a conversation.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Done | Self::Error | Self::Cancel)
    }

    /// Content statuses carry user data in the frame payload.
    #[must_use]
    pub fn is_content(self) -> bool {
        matches!(self, Self::Item | Self::Byte | Self::Bulk)
    }
}
