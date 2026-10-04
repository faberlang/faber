//! K3 parser pins: a `summa` operator word stays an ordinary identifier,
//! call, receiver, or intrinsic method outside the owned `reducta via` form.

use crate::test_support::{assert_parse_error_issue, parse_ok};

/// Kernel source from `examples/training/device-summa/src/device_summa.fab`,
/// verbatim. Used as the live-file fallback in lane worktrees where the
/// sibling `examples/` container is absent.
const DEVICE_SUMMA_KERNEL: &str = "@ nucleum\nfunctio summa(tf32[256] a, tf32[1] out, u32 id) → vacuum {\n    fixum f32 total ← a.summa()\n    out[id] ← total\n}\n";

/// Both kernels from
/// `examples/training/device-summa-recollige/src/device_summa_recollige.fab`,
/// verbatim (fallback copy, same reason as above).
const DEVICE_SUMMA_RECOLLIGE_KERNELS: &str = "@ nucleum\nfunctio collige(tf32[1024] a, tf32[4] medius, u32 id) → vacuum {\n    fixum f32 total ← a.summa()\n    medius[id] ← total\n}\n\n@ nucleum\nfunctio recollige(tf32[4] medius, tf32[1] result, u32 id) → vacuum {\n    fixum f32 total ← medius.summa()\n    result[id] ← total\n}\n";

/// Anchor: the audit-corrected citation for the `.summa` receiver pins is
/// `examples/training/device-summa*`. Read the live exemplar when the
/// workspace container is present; fall back to the embedded verbatim copy
/// in isolated worktrees.
fn exemplar_source(relative: &str, fallback: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|_| {
        eprintln!(
            "SFR-1 pin: live exemplar {} absent (lane worktree?), using embedded copy",
            path.display()
        );
        fallback.to_string()
    })
}

#[test]
fn summa_head_call_stays_ordinary() {
    let _ = parse_ok("functio addere(numerus a, numerus b) → numerus { summa(2, 3) }");
}

#[test]
fn summa_receiver_stays_ordinary_device_summa_exemplar() {
    // `.summa()` receiver anchored at examples/training/device-summa* — the
    // kernel named `summa` AND the method receiver `a.summa()` both stay
    // ordinary today.
    let source = exemplar_source(
        "examples/training/device-summa/src/device_summa.fab",
        DEVICE_SUMMA_KERNEL,
    );
    let _ = parse_ok(&source);
}

#[test]
fn summa_receiver_stays_ordinary_recollige_exemplar() {
    // Second exemplar: two receivers (`a.summa()`, `medius.summa()`) and no
    // head-position `summa` spelling at all.
    let source = exemplar_source(
        "examples/training/device-summa-recollige/src/device_summa_recollige.fab",
        DEVICE_SUMMA_RECOLLIGE_KERNELS,
    );
    let _ = parse_ok(&source);
}

#[test]
fn non_head_summa_identifiers_stay_ordinary() {
    let _ = parse_ok("functio f() → numerus { fixum numerus summa ← 2 redde summa }");
    // Known-good lambda spelling named `summa` (closure parameter position).
    let _ = parse_ok("fixum _ summa ← (numerus a, numerus b) ∴ a + b");
    let _ = parse_ok("functio g(numerus summa) → numerus { redde summa }");
}

#[test]
fn legacy_reduction_heads_report_the_named_migration_issue() {
    for source in [
        "functio f(tensor<numerus, [4]> xs) → numerus { redde summa ex xs }",
        "functio f(tensor<numerus, [4]> xs) → numerus { redde maxima ex xs }",
        "functio f(tensor<numerus, [4]> xs) → numerus { redde minima ex xs vel 0 }",
    ] {
        assert_parse_error_issue(source, "reducta_legacy_head");
    }
}
