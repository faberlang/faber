//! Conversio (`↦`) emit-arm detection probe — measurement path only.
//!
//! The conversio coverage matrix distinguishes a **dedicated lowering arm**
//! (✓) from the **unspecialized fallback** (◐ — not guaranteed to compile) per
//! (src family × tgt family × HIR backend). The hand classifier in
//! `radix::codegen::conversio_coverage` predicts that split from source
//! reading; this probe *measures* it by recording, at emission time, which arm
//! each `↦` conversion actually reached.
//!
//! ## Inert by construction
//!
//! Recording is a thread-local push; nothing reads the probe in production and
//! no emitted output changes. The exempla e2e harness clears the probe before
//! a codegen call and takes the records after, attributing them to the
//! (src, tgt) cell under test via the analyzed unit's `TypeTable`.
//!
//! ## Nested conversions
//!
//! Conversion expressions may contain conversions (e.g. `("42" ↦ numerus) ↦
//! textus`). The probe uses a frame stack: each `↦` pushes a frame that starts
//! `Dedicated`; entering the unspecialized fallback marks the top frame
//! `Fallback`. Frames strictly nest (LIFO), so the top frame is always the
//! currently emitting conversion. When a frame drops (normal return or unwind)
//! its record is persisted to the records list for `conversio_arm_take`.

use radix_types::TypeId;
use std::cell::RefCell;

/// Which conversio arm handled a conversion at emission time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversioArm {
    /// A dedicated lowering arm matched (✓).
    Dedicated,
    /// The unspecialized fallback handled it (◐ — may not compile).
    Fallback,
}

impl ConversioArm {
    /// Stable machine-readable arm name used in the harness TSV.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Dedicated => "dedicated",
            Self::Fallback => "fallback",
        }
    }
}

/// One recorded `↦` emission. Family attribution happens after codegen by
/// mapping the `TypeId`s through the analyzed unit's `TypeTable` (see
/// `radix::codegen::conversio_coverage::ConversioTypeFamily::from_type_id`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConversioArmRecord {
    /// Which arm handled the conversion.
    pub arm: ConversioArm,
    /// Source type id, when the conversion source was typed.
    pub source_ty: Option<TypeId>,
    /// Target type id, when the conversion target was a lowered type.
    pub target_ty: Option<TypeId>,
}

/// RAII guard recording one `↦` emission. Create it at the top of a backend's
/// conversio entry point; call [`conversio_arm_mark_fallback`] where the
/// backend's unspecialized fallback runs. The frame is recorded on drop.
#[must_use = "dropping the guard records the emission; keep it alive for the conversion's lifetime"]
pub struct ConversioArmGuard {
    depth: usize,
}

thread_local! {
    static FRAMES: RefCell<Vec<ConversioArmRecord>> = const { RefCell::new(Vec::new()) };
    static RECORDS: RefCell<Vec<ConversioArmRecord>> = const { RefCell::new(Vec::new()) };
}

/// Push a new `Dedicated` frame for the conversion being emitted. The guard's
/// own `#[must_use]` covers the return.
pub fn conversio_arm_begin(
    source_ty: Option<TypeId>,
    target_ty: Option<TypeId>,
) -> ConversioArmGuard {
    let depth = FRAMES.with(|frames| {
        let mut frames = frames.borrow_mut();
        frames.push(ConversioArmRecord {
            arm: ConversioArm::Dedicated,
            source_ty,
            target_ty,
        });
        frames.len()
    });
    ConversioArmGuard { depth }
}

/// Mark the currently emitting conversion as handled by the unspecialized
/// fallback. Call this at the top of the backend's fallback emitter.
pub fn conversio_arm_mark_fallback() {
    FRAMES.with(|frames| {
        if let Some(frame) = frames.borrow_mut().last_mut() {
            frame.arm = ConversioArm::Fallback;
        }
    });
}

impl Drop for ConversioArmGuard {
    fn drop(&mut self) {
        // Frames strictly nest; the guard's frame is the top one at drop time.
        // Persist it into the records list so `conversio_arm_take` sees it
        // after the emission completes.
        FRAMES.with(|frames| {
            let mut frames = frames.borrow_mut();
            debug_assert!(
                frames.len() == self.depth,
                "conversio arm frames must nest (len {} != depth {})",
                frames.len(),
                self.depth
            );
            if let Some(frame) = frames.pop() {
                RECORDS.with(|records| records.borrow_mut().push(frame));
            }
        });
    }
}

/// Discard all pending records (start of a fresh measurement).
pub fn conversio_arm_clear() {
    FRAMES.with(|frames| frames.borrow_mut().clear());
    RECORDS.with(|records| records.borrow_mut().clear());
}

/// Drain all recorded conversions. Records persist when their emission
/// completes (frame drop), so nested conversions appear before their outer
/// conversion. The harness attributes records by family pair, so order is
/// informational.
#[must_use]
pub fn conversio_arm_take() -> Vec<ConversioArmRecord> {
    let mut records = RECORDS.with(|records| std::mem::take(&mut *records.borrow_mut()));
    // Any frames still in flight (codegen interrupted mid-emission) also
    // belong to this measurement.
    FRAMES.with(|frames| records.extend(std::mem::take(&mut *frames.borrow_mut())));
    records
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedicated_frame_records_dedicated() {
        conversio_arm_clear();
        {
            let _guard = conversio_arm_begin(None, None);
        }
        let records = conversio_arm_take();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].arm, ConversioArm::Dedicated);
    }

    #[test]
    fn fallback_mark_flips_top_frame() {
        conversio_arm_clear();
        {
            let _guard = conversio_arm_begin(None, None);
            conversio_arm_mark_fallback();
        }
        let records = conversio_arm_take();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].arm, ConversioArm::Fallback);
    }

    #[test]
    fn nested_frames_mark_the_innermost() {
        conversio_arm_clear();
        {
            let outer = conversio_arm_begin(Some(TypeId(1)), Some(TypeId(2)));
            {
                let _inner = conversio_arm_begin(Some(TypeId(3)), Some(TypeId(4)));
                conversio_arm_mark_fallback();
            }
            drop(outer);
        }
        let records = conversio_arm_take();
        assert_eq!(records.len(), 2);
        // The inner conversion completes (drops) before the outer, so it is
        // recorded first — with the fallback mark, not the outer's dedicated.
        assert_eq!(records[0].arm, ConversioArm::Fallback);
        assert_eq!(records[0].source_ty, Some(TypeId(3)));
        assert_eq!(records[1].arm, ConversioArm::Dedicated);
        assert_eq!(records[1].source_ty, Some(TypeId(1)));
    }
}
