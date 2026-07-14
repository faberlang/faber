//! Faber-owned target capability reporting.

use radix::tool::{target_capabilities_for_surface, TargetCommandSurface};

const FABER_TARGET_ROWS: &[(radix::codegen::Target, &str)] = &[
    (radix::codegen::Target::Rust, "rust"),
    (radix::codegen::Target::FmirText, "fmir-text"),
    (radix::codegen::Target::Fmir, "fmir"),
    (radix::codegen::Target::FmirBin, "fmir-bin"),
    (radix::codegen::Target::Faber, "faber"),
    (radix::codegen::Target::Go, "go"),
    (radix::codegen::Target::Wasm, "wasm"),
    (radix::codegen::Target::WasmText, "wasm-text"),
    (radix::codegen::Target::LlvmText, "llvm-text"),
    (radix::codegen::Target::MetalText, "metal-text"),
    (radix::codegen::Target::WgslText, "wgsl-text"),
    (radix::codegen::Target::Sexp, "sexp"),
    (radix::codegen::Target::TypeScript, "ts"),
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
