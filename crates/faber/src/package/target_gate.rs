//! Single decision point for "is this target compiled into THIS faber binary".
//!
//! The package build paths gate target leaves behind cargo features, so a
//! target the compiler knows about can still be absent from the running
//! binary. This module answers that question once and builds the one
//! diagnostic (`PKG001:target_not_in_this_build`) for the absent case.
//!
//! INVARIANTS
//! ==========
//! - `target_compiled_in` names every [`Target`] variant; a new variant is a
//!   compile error here, never a silent `false`.
//! - The diagnostic carries structured facts (`issue`, `target`, `feature`);
//!   the help text is the only prose that differs by target class.

use radix::module::codegen::Target;
use radix::module::diagnostics::{Diagnostic, render_plain};

/// Diagnostic issue name for a target that this faber build does not contain.
pub const TARGET_NOT_IN_THIS_BUILD: &str = "target_not_in_this_build";

// WHY: the owner of this list is `radix-target/tests/full_targets_membership.rs`;
// these features are default-off and not part of the official faber release.
const EXPERIMENTAL_DEFAULT_OFF: [&str; 7] = [
    "mir-evm",
    "mir-basic",
    "hir-pascal",
    "hir-cobol",
    "hir-fortran",
    "hir-odin",
    "hir-c",
];

/// Feature name reported for targets whose package path is gated on `runner`.
const RUNNER_FEATURE: &str = "runner";

/// Whether `target` is compiled into this faber binary.
#[must_use]
pub fn target_compiled_in(target: Target) -> bool {
    match target {
        Target::MirStepper => true,
        Target::MirScena | Target::MirFmir | Target::MirFmirBinary => {
            cfg!(feature = "runner")
        }
        Target::HirRust
        | Target::HirTypeScript
        | Target::HirGo
        | Target::HirSwift
        | Target::HirPython
        | Target::HirHaskell
        | Target::HirPascal
        | Target::HirCobol
        | Target::HirFortran
        | Target::HirOdin
        | Target::HirC
        | Target::HirFaber
        | Target::HirFhir
        | Target::MirWasm
        | Target::MirLlvm
        | Target::MirLlvmHost
        | Target::MirAmd
        | Target::MirMetal
        | Target::MirWgsl
        | Target::MirSexp
        | Target::MirClif
        | Target::MirEvm
        | Target::MirBasic => target.required_feature().is_some_and(feature_compiled),
    }
}

fn feature_compiled(feature: &str) -> bool {
    match feature {
        "hir-rust" => cfg!(feature = "hir-rust"),
        "hir-faber" => cfg!(feature = "hir-faber"),
        "hir-ts" => cfg!(feature = "hir-ts"),
        "hir-go" => cfg!(feature = "hir-go"),
        "hir-swift" => cfg!(feature = "hir-swift"),
        "hir-python" => cfg!(feature = "hir-python"),
        "hir-haskell" => cfg!(feature = "hir-haskell"),
        "hir-pascal" => cfg!(feature = "hir-pascal"),
        "hir-cobol" => cfg!(feature = "hir-cobol"),
        "hir-fortran" => cfg!(feature = "hir-fortran"),
        "hir-odin" => cfg!(feature = "hir-odin"),
        "hir-c" => cfg!(feature = "hir-c"),
        "hir-fhir" => cfg!(feature = "hir-fhir"),
        "mir-sexp" => cfg!(feature = "mir-sexp"),
        "mir-clif" => cfg!(feature = "mir-clif"),
        "mir-basic" => cfg!(feature = "mir-basic"),
        "mir-evm" => cfg!(feature = "mir-evm"),
        "mir-metal" => cfg!(feature = "mir-metal"),
        "mir-wgsl" => cfg!(feature = "mir-wgsl"),
        "mir-llvm" => cfg!(feature = "mir-llvm"),
        "mir-wasm" => cfg!(feature = "mir-wasm"),
        _ => false,
    }
}

/// The one diagnostic for a target that is not compiled into this build.
#[must_use]
pub fn target_not_in_this_build(target: Target) -> Diagnostic {
    let name = target.as_str();
    let feature = target.required_feature().unwrap_or(RUNNER_FEATURE);
    let help = if EXPERIMENTAL_DEFAULT_OFF.contains(&feature) {
        format!(
            "target `{name}` is experimental and is not part of the official faber release; rebuild faber from source with feature `{feature}`"
        )
    } else {
        format!(
            "target `{name}` ships in the official faber release; use the official faber release to build or emit it"
        )
    };
    crate::package_diagnostic_error(format!(
        "target `{name}` is not compiled into this faber build"
    ))
    .with_arg("issue", TARGET_NOT_IN_THIS_BUILD)
    .with_arg("target", name)
    .with_arg("feature", feature)
    .with_help(help)
}

/// `Ok(())` when `target` is compiled in, otherwise the gate diagnostic.
///
/// # Errors
/// Returns the `PKG001:target_not_in_this_build` diagnostic when this binary
/// does not contain `target`.
pub fn require_target_compiled_in(target: Target) -> Result<(), Diagnostic> {
    if target_compiled_in(target) {
        Ok(())
    } else {
        Err(target_not_in_this_build(target))
    }
}

/// Print the gate diagnostic for `target` to stderr and exit with
/// [`UNAVAILABLE`](crate::exit_status::UNAVAILABLE).
pub fn exit_target_not_in_this_build(target: Target) -> ! {
    let diag = target_not_in_this_build(target);
    eprintln!("{}", render_plain(&diag));
    crate::exit_status::exit_unavailable()
}

#[cfg(test)]
#[path = "target_gate_test.rs"]
mod tests;
