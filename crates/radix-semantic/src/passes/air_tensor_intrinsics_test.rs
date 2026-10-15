use super::air_tensor_intrinsics::is_allowed_tensor_intrinsic;

const ADMITTED_INTRINSICS: &[&str] = &[
    "addita",
    "addita_bias",
    "subtracta",
    "multiplicata",
    "divisa",
    // The pre-G-02 spellings `subtrahe`/`multiplica`/`divisio` were admitted
    // only until ruling C of need renamed the Tensor registry rows.
    // That rename landed in 43154f1f59, so they are retired and no longer
    // belong on this list.
    "nega",
    "summa",
    "media",
    "maxima",
    "minima",
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
    "rms_norm",
    "rope_norm",
    "gather",
    "reducta",
    "cumulata",
    "mappata",
    "filtrata",
];

#[test]
fn shared_air_tensor_allowlist_preserves_all_admitted_rows() {
    for method in ADMITTED_INTRINSICS {
        assert!(
            is_allowed_tensor_intrinsic(method),
            "expected AIR tensor intrinsic to remain admitted: {method}"
        );
    }
}

#[test]
fn shared_air_tensor_allowlist_remains_fail_closed() {
    for method in ["unknown", "cumulatax", "reducta_extra"] {
        assert!(
            !is_allowed_tensor_intrinsic(method),
            "unknown tensor intrinsic must remain outside the AIR allowlist: {method}"
        );
    }
}
