//! Target-capability validation vocabulary for MIR-backed emitters.
//!
//! TARGET: shared lowerability seam for all MIR text/binary targets
//! (llvm-text, wasm-text, metal-text, and future GPU targets).
//!
//! WHY: before this module, each emitter rejected unsupported MIR shapes with
//! ad-hoc `unsupported(shape: impl Into<String>)` calls deep in its print path,
//! producing opaque flat strings (`"MIR-to-LLVM unsupported: try_call"`) with no
//! category and no pre-emission decision. Stage 2 of the MIR LLVM campaign
//! introduces this target-neutral vocabulary so emitters receive a classified
//! [`Lowerability`] verdict *before* they start printing, and so later stages
//! (layout, ABI, failable CFG) plug facts into a shared classification rather
//! than re-deriving it per backend.
//!
//! BOUNDARY: this module classifies *target lowerability* — can this target
//! print this MIR shape? — and must NOT duplicate semantic typechecking. Type
//! correctness is owned by [`crate::validate::MirValidationContext`]; a
//! type-correct program can still be non-lowerable (e.g. an unresolved generic
//! `Type::Param` that survived to MIR, or a `try_call` whose CFG contract the
//! target has not implemented).
//!
//! RECIPROCITY: Wasm and other MIR targets adopt the vocabulary by importing
//! [`CapabilityGap`] and producing a [`Lowerability`] verdict from their own
//! classifier. Each target owns its classifier (which shapes it rejects and into
//! which category); the vocabulary is shared. See
//! `docs/factory/mir-llvm/goals/02-target-capability-validation.md`.
