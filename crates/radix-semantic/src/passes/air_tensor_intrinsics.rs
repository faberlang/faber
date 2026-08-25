//! Shared AIR-lane tensor-method admission policy.
//!
//! Both AIR passes inspect tensor method calls before the purity gate runs. Keep
//! their admission row set in one place so an intrinsic cannot pass one stage
//! and decline in the other.

pub(super) fn is_allowed_tensor_intrinsic(method: &str) -> bool {
    matches!(
        method,
        "addita"
            | // S6-C2: the bounded bias-broadcast add is an AIR-lane tensor method.
            "addita_bias"
            | "subtrahe"
            | "multiplica"
            | "divisio"
            | "nega"
            | "summa"
            | "media"
            | "maxima"
            | "minima"
            | "matmul"
            | "transpone"
            | "forma"
            | "exponentia"
            | "logarithmus"
            | "activatio_relu"
            | "radix"
            | "gelu"
            | "silu"
            | "activatio_tanh"
            | "activatio_softmax"
            | "laminatio"
            | "summa_axis"
            | "crux_entropia"
            // CCI-1: the three common-case intrinsic source contracts are
            // AIR-lane tensor methods (forward-only in v1; reverse AD rejects
            // them upstream via `reverse_ad_no_vjp_rule`).
            | "rms_norm"
            | "rope_norm"
            | "gather"
            // ARC-3: higher-order tensor methods admit named AIR combines;
            // closure combines remain rejected by the purity traversal.
            | "reducta"
            | "cumulata"
            | "mappata"
            | "filtrata"
    )
}
