//! Neutral compilation-target identity for Radix.
//!
//! Owns the [`Target`] enum, its CLI spelling ([`fmt::Display`]), and the
//! feature-gate mapping ([`Target::required_feature`] /
//! [`Target::is_available`]). Target identity is a std-only contract shared
//! by semantic policy, the driver, services, and tests; it must not depend
//! on HIR, MIR, codegen, diagnostics, or any backend leaf.
//!
//! Consumers forward their own `hir-*` / `mir-*` Cargo features here 1:1 so
//! [`Target::is_available`] reflects the consumer's build, not this crate's.

use std::fmt;

/// Compilation target selected after analysis and before backend dispatch.
///
/// This enum is the public routing surface for drivers. Each variant commits to
/// a concrete backend module and a matching [`crate::Output`] payload shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Core MIR interpreter target. This is the minimum executable compiler
    /// route, not an external source-code emission backend.
    MirStepper,

    /// Native Rust source, used for package builds and executable output.
    HirRust,

    /// TypeScript source for JavaScript ecosystem targets.
    HirTypeScript,

    /// Go source for experiments in a second systems/runtime target.
    HirGo,

    /// Swift source for the Apple ecosystem target.
    HirSwift,

    /// Python source for the ordinary host Python target.
    HirPython,

    /// Haskell source emitted from checked HIR.
    HirHaskell,

    /// Faber source re-emitted from analyzed HIR.
    ///
    /// Historically the "canonical Faber" surface; with reader locales this
    /// is the HIR→Faber re-emit behind `faber format --locale <X>`.
    HirFaber,

    /// Portable analyzed HIR artifact (postcard binary); serialized in emit workflow, not codegen.
    HirFhir,

    /// Supported-with-limitations `MIR`-backed WebAssembly text target.
    /// Emits `.wat` text only; binary conversion is an external-tool step
    /// (`wat2wasm` / `wasm-tools parse`).
    MirWasm,

    /// Supported-with-limitations `MIR`-backed LLVM text staging target.
    MirLlvm,

    /// Native host executable product target (`faber build/run --target
    /// llvm-host`). Distinct from `llvm-text`: Faber owns build/run
    /// orchestration. Driver generation uses the same MIR-to-LLVM emitter.
    MirLlvmHost,

    /// AMD GPU via the AMDGPU/HIP LLVM emitter family.
    ///
    /// Internal variant on the `mir-llvm` emit surface — not a discoverable
    /// `radix targets` row until the leaf proves a truthful baseline. The
    /// immediate artifact is AMDGPU LLVM IR for the explicit `gfx942` row.
    /// Without `mir-amd` the target is unavailable and the fail-closed
    /// diagnostic names the missing feature.
    MirAmd,

    /// Supported-with-limitations `MIR`-backed Metal Shading Language source target.
    MirMetal,

    /// Supported-with-limitations `MIR`-backed WGSL compute-shader source target.
    MirWgsl,

    /// Supported-with-limitations `MIR`-backed Racket validation target.
    MirSexp,

    /// `MIR` package artifact executed by the `scena` runtime.
    MirScena,

    /// Source-independent package `MIR` text image consumed by the runner.
    MirFmir,

    /// Source-independent package `MIR` binary image consumed by the runner.
    MirFmirBinary,

    /// Direct runnable binary wrapper with embedded FMIR image bytes.
    MirFmirBundle,
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::MirStepper => write!(f, "stepper"),
            Target::HirRust => write!(f, "rust"),
            Target::HirTypeScript => write!(f, "ts"),
            Target::HirGo => write!(f, "go"),
            Target::HirSwift => write!(f, "swift"),
            Target::HirPython => write!(f, "python"),
            Target::HirHaskell => write!(f, "haskell"),
            Target::HirFaber => write!(f, "faber"),
            Target::HirFhir => write!(f, "fhir"),
            Target::MirWasm => write!(f, "wasm-text"),
            Target::MirLlvm => write!(f, "llvm-text"),
            Target::MirLlvmHost => write!(f, "llvm-host"),
            Target::MirAmd => write!(f, "amd"),
            Target::MirMetal => write!(f, "metal-text"),
            Target::MirWgsl => write!(f, "wgsl-text"),
            Target::MirSexp => write!(f, "sexp"),
            Target::MirScena => write!(f, "scena"),
            Target::MirFmir => write!(f, "fmir-text"),
            Target::MirFmirBinary => write!(f, "fmir"),
            Target::MirFmirBundle => write!(f, "fmir-bin"),
        }
    }
}

impl Target {
    /// Feature that enables this target when it is feature-gated.
    #[must_use]
    pub fn required_feature(self) -> Option<&'static str> {
        match self {
            Target::HirRust => Some("hir-rust"),
            Target::HirTypeScript => Some("hir-ts"),
            Target::HirGo => Some("hir-go"),
            Target::HirSwift => Some("hir-swift"),
            Target::HirPython => Some("hir-python"),
            Target::HirHaskell => Some("hir-haskell"),
            Target::HirFaber => Some("hir-faber"),
            Target::HirFhir => Some("hir-fhir"),
            Target::MirMetal => Some("mir-metal"),
            Target::MirSexp => Some("mir-sexp"),
            Target::MirAmd => Some("mir-amd"),
            Target::MirWasm => Some("mir-wasm"),
            Target::MirWgsl => Some("mir-wgsl"),
            Target::MirLlvm | Target::MirLlvmHost => Some("mir-llvm"),
            _ => None,
        }
    }

    /// Whether this target is available in the current Radix build.
    #[must_use]
    // WHY: each arm computes a distinct feature gate; `matches!` would compare
    // patterns and change semantics.
    #[allow(clippy::match_like_matches_macro)]
    pub fn is_available(self) -> bool {
        match self {
            Target::HirRust => cfg!(feature = "hir-rust"),
            Target::HirTypeScript => cfg!(feature = "hir-ts"),
            Target::HirGo => cfg!(feature = "hir-go"),
            Target::HirSwift => cfg!(feature = "hir-swift"),
            Target::HirPython => cfg!(feature = "hir-python"),
            Target::HirHaskell => cfg!(feature = "hir-haskell"),
            Target::HirFaber => cfg!(feature = "hir-faber"),
            Target::HirFhir => cfg!(feature = "hir-fhir"),
            Target::MirMetal => cfg!(feature = "mir-metal"),
            Target::MirSexp => cfg!(feature = "mir-sexp"),
            Target::MirAmd => cfg!(feature = "mir-amd"),
            Target::MirWasm => cfg!(feature = "mir-wasm"),
            Target::MirWgsl => cfg!(feature = "mir-wgsl"),
            Target::MirLlvm | Target::MirLlvmHost => cfg!(feature = "mir-llvm"),
            _ => true,
        }
    }
}
