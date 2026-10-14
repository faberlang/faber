//! Shared AIR-lane tensor-method admission policy.
//!
//! Both AIR passes inspect tensor method calls before the purity gate runs. Keep
//! their admission row set in one place so an intrinsic cannot pass one stage
//! and decline in the other.
//!
//! The row set is exposed behind a `#[doc(hidden)]` accessor: the radix-module
//! bijection ratchet derives its direction-3 candidate universe from it, so
//! allowlist growth here is enforced structurally instead of drifting from a
//! hand-assembled mirror.

const ALLOWED_TENSOR_INTRINSICS: &[&str] = &[
    "addita",
    // S6-C2: the bounded bias-broadcast add is an AIR-lane tensor method.
    "addita_bias",
    "subtracta",
    "multiplicata",
    "divisa",
    // Old spellings stay admitted until the Tensor registry rows are renamed
    // (ruling C of need): the allowlist is a superset of both
    // generations.
    "subtrahe",
    "multiplica",
    "divisio",
    "nega",
    "summa",
    "media",
    "maxima",
    "minima",
    // LSR-W0-SEMANTIC: the argmax/argmin twin identities, registered beside
    // `maxima`/`minima` for the device method family's mechanical adoption.
    // W0 admits the identities only — no `AirTensorOp` mapping yet (the
    // unarmed-admission class of `mappata`/`filtrata`: a kernel-lane call
    // fails closed in AIR lowering today).
    "argmaxima",
    "argminima",
    "matmul",
    "transpone",
    "forma",
    "exponentia",
    "logarithmus",
    "activatio_relu",
    "radix",
    "gelu",
    "silu",
    "activatio_tanh",
    "activatio_softmax",
    "laminatio",
    "summa_axis",
    "crux_entropia",
    // CCI-1: the three common-case intrinsic source contracts are AIR-lane
    // tensor methods (forward-only in v1; reverse AD rejects them upstream via
    // `reverse_ad_no_vjp_rule`).
    "rms_norm",
    "rope_norm",
    "gather",
    // ARC-3: higher-order tensor methods admit named AIR combines; closure
    // combines remain rejected by the purity traversal.
    "reducta",
    "cumulata",
    "mappata",
    "filtrata",
];

/// The AIR-lane tensor-method admission set. Test-support surface: the
/// radix-module bijection ratchet derives its direction-3 candidates from it.
#[doc(hidden)]
#[must_use]
pub fn allowed_tensor_intrinsics() -> &'static [&'static str] {
    ALLOWED_TENSOR_INTRINSICS
}

pub(super) fn is_allowed_tensor_intrinsic(method: &str) -> bool {
    ALLOWED_TENSOR_INTRINSICS.contains(&method)
}
