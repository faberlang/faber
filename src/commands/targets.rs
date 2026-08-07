//! Faber-owned target capability reporting.

use radix::tool::{target_capabilities_for_surface, TargetCommandSurface};

const FABER_TARGET_ROWS: &[(radix::codegen::Target, &str)] = &[
    (radix::codegen::Target::HirRust, "rust"),
    (radix::codegen::Target::HirFhir, "fhir"),
    (radix::codegen::Target::MirFmir, "fmir-text"),
    (radix::codegen::Target::MirFmirBinary, "fmir"),
    (radix::codegen::Target::MirFmirBundle, "fmir-bin"),
    (radix::codegen::Target::HirFaber, "faber"),
    (radix::codegen::Target::HirGo, "go"),
    (radix::codegen::Target::MirWasmBinary, "wasm"),
    (radix::codegen::Target::MirWasm, "wasm-text"),
    (radix::codegen::Target::MirLlvm, "llvm-text"),
    (radix::codegen::Target::MirLlvmHost, "llvm-host"),
    (radix::codegen::Target::MirMetal, "metal-text"),
    (radix::codegen::Target::MirWgsl, "wgsl-text"),
    (radix::codegen::Target::MirSexp, "sexp"),
    (radix::codegen::Target::HirTypeScript, "ts"),
];

pub(crate) fn cmd_targets() {
    for &(target, name) in FABER_TARGET_ROWS {
        let capabilities = target_capabilities_for_surface(TargetCommandSurface::Faber, target);
        println!(
            "{} check={} build={} run={} package={} note={}",
            name,
            yes_no(capabilities.check),
            yes_no(capabilities.build),
            yes_no(capabilities.run),
            yes_no(capabilities.package),
            capabilities.note
        );
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}
