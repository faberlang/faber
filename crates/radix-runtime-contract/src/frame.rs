//! Radix-owned frame status contract: the lifecycle states of a `sermo`
//! (conversation) frame in the `ad` bridge/gateway architecture.
//!
//! Mirrors `faber::frame::FrameStatus`. Radix is the authority for this
//! contract; the runtime copy is expected to become a generated/committed
//! artifact in a later phase (same direction as `crate::abi::host_abi`). The
//! seven variants and their terminal/content classification are also encoded in
//! `crate::builtins::frame_types::STATUS_VARIANTS` and emitted by
//! `radix_hir_rust::frame_shim` and `radix_hir_go::frame_shim` for every target backend.
//!
//! KEEP IN SYNC: `radix::builtins::frame_types::STATUS_VARIANTS` and the text emissions in
//! `radix_hir_rust::frame_shim` (verified by `#[cfg(test)] mod tests` in this file).

/// Frame lifecycle status for stream `ad` conversations.
///
/// Every variant corresponds to a Faber-language `status` variant registered in
/// [`crate::builtins::frame_types`]. The variant order matches the canonical
/// `STATUS_VARIANTS` list: `request, item, byte, bulk, done, error, cancel`.
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

#[cfg(test)]
#[path = "frame_test.rs"]
mod tests;
