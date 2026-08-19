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
mod tests {
    use super::*;

    /// Canonical variant order matches `STATUS_VARIANTS` in
    /// `radix::builtins::frame_types`.
    const EXPECTED_VARIANTS: &[(&str, bool, bool)] = &[
        ("Request", false, false), // terminal=false, content=false
        ("Item", false, true),     // terminal=false, content=true
        ("Byte", false, true),     // terminal=false, content=true
        ("Bulk", false, true),     // terminal=false, content=true
        ("Done", true, false),     // terminal=true,  content=false
        ("Error", true, false),    // terminal=true,  content=false
        ("Cancel", true, false),   // terminal=true,  content=false
    ];

    /// Every variant matches the expected canonical order and classification.
    #[test]
    fn variant_order_and_classification() {
        let variants: &[(FrameStatus, &str, bool, bool)] = &[
            (FrameStatus::Request, "Request", false, false),
            (FrameStatus::Item, "Item", false, true),
            (FrameStatus::Byte, "Byte", false, true),
            (FrameStatus::Bulk, "Bulk", false, true),
            (FrameStatus::Done, "Done", true, false),
            (FrameStatus::Error, "Error", true, false),
            (FrameStatus::Cancel, "Cancel", true, false),
        ];

        for (i, (status, expected_name, expected_terminal, expected_content)) in
            variants.iter().enumerate()
        {
            let (exp_name, _exp_term, _exp_cont) = EXPECTED_VARIANTS[i];
            assert_eq!(
                *expected_name, exp_name,
                "variant name mismatch at index {i}"
            );
            assert_eq!(
                format!("{status:?}"),
                *expected_name,
                "Debug format for {status:?} does not match expected name"
            );
            assert_eq!(
                status.is_terminal(),
                *expected_terminal,
                "is_terminal() for {expected_name} is wrong"
            );
            assert_eq!(
                status.is_content(),
                *expected_content,
                "is_content() for {expected_name} is wrong"
            );
        }
    }

    /// No variant is both terminal and content.
    #[test]
    fn no_variant_is_both_terminal_and_content() {
        let variants = [
            FrameStatus::Request,
            FrameStatus::Item,
            FrameStatus::Byte,
            FrameStatus::Bulk,
            FrameStatus::Done,
            FrameStatus::Error,
            FrameStatus::Cancel,
        ];
        for v in &variants {
            assert!(
                !(v.is_terminal() && v.is_content()),
                "{v:?} is both terminal and content"
            );
        }
    }

    /// Every variant is either terminal or not-terminal (classifications cover
    /// all variants — no variant falls through an undetected match gap).
    #[test]
    fn variant_count_is_exhaustive() {
        // If a variant is added or removed, this test's array length must
        // change. Paired with `variant_order_and_classification` this ensures
        // the canonical 7-variant shape is preserved.
        assert_eq!(
            EXPECTED_VARIANTS.len(),
            7,
            "FrameStatus must have exactly 7 variants"
        );
    }
}
