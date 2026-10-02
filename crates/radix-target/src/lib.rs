//! Neutral compilation-target identity for Radix.
//!
//! Owns the [`Target`] enum, the variant universe ([`Target::ALL`]), its
//! CLI spelling ([`fmt::Display`]), and the feature-gate mapping
//! ([`Target::required_feature`] / [`Target::is_available`]). Target
//! identity is a std-only contract shared
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

    /// Novelty Free Pascal source emitted from checked HIR (scalar subset).
    /// Default-off; fails closed outside the subset.
    HirPascal,

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

    /// Novelty `MIR`-backed classic line-numbered BASIC target (Commodore 64
    /// BASIC V2 compatible subset). Default-off; scalar programs only.
    MirBasic,

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
            Target::HirPascal => write!(f, "pascal"),
            Target::HirFaber => write!(f, "faber"),
            Target::HirFhir => write!(f, "fhir"),
            Target::MirWasm => write!(f, "wasm-text"),
            Target::MirLlvm => write!(f, "llvm-text"),
            Target::MirLlvmHost => write!(f, "llvm-host"),
            Target::MirAmd => write!(f, "amd"),
            Target::MirMetal => write!(f, "metal-text"),
            Target::MirWgsl => write!(f, "wgsl-text"),
            Target::MirSexp => write!(f, "sexp"),
            Target::MirBasic => write!(f, "basic"),
            Target::MirScena => write!(f, "scena"),
            Target::MirFmir => write!(f, "fmir-text"),
            Target::MirFmirBinary => write!(f, "fmir"),
            Target::MirFmirBundle => write!(f, "fmir-bin"),
        }
    }
}

impl Target {
    /// Parse a canonical target spelling into a [`Target`].
    ///
    /// Exact inverse of [`fmt::Display`]: every canonical spelling
    /// round-trips, and nothing else is accepted. Caller-surface aliases
    /// (`typescript` for `ts`, `wasm` for `wasm-text`, …) are CLI
    /// conveniences owned by the caller, not canonical spellings.
    #[must_use]
    pub fn from_spelling(spelling: &str) -> Option<Self> {
        match spelling {
            "stepper" => Some(Self::MirStepper),
            "rust" => Some(Self::HirRust),
            "ts" => Some(Self::HirTypeScript),
            "go" => Some(Self::HirGo),
            "swift" => Some(Self::HirSwift),
            "python" => Some(Self::HirPython),
            "haskell" => Some(Self::HirHaskell),
            "pascal" => Some(Self::HirPascal),
            "faber" => Some(Self::HirFaber),
            "fhir" => Some(Self::HirFhir),
            "wasm-text" => Some(Self::MirWasm),
            "llvm-text" => Some(Self::MirLlvm),
            "llvm-host" => Some(Self::MirLlvmHost),
            "amd" => Some(Self::MirAmd),
            "metal-text" => Some(Self::MirMetal),
            "wgsl-text" => Some(Self::MirWgsl),
            "sexp" => Some(Self::MirSexp),
            "basic" => Some(Self::MirBasic),
            "scena" => Some(Self::MirScena),
            "fmir-text" => Some(Self::MirFmir),
            "fmir" => Some(Self::MirFmirBinary),
            "fmir-bin" => Some(Self::MirFmirBundle),
            _ => None,
        }
    }

    /// Every variant of this enum, in declaration order — the single owned
    /// source of the target universe (defect-audit P6-3).
    ///
    /// Any surface that must enumerate targets (report rows, completeness
    /// tests, discovery lists) starts from this list instead of keeping a
    /// second hand-typed copy. The list cannot silently drift from the
    /// enum: [`all_is_exactly_the_variant_set`] walks the variant domain
    /// through an exhaustive match with no wildcard arm, so adding or
    /// removing a variant fails that test's compilation and the assertion
    /// then fails until `ALL` matches the domain exactly.
    pub const ALL: &'static [Self] = &[
        Target::MirStepper,
        Target::HirRust,
        Target::HirTypeScript,
        Target::HirGo,
        Target::HirSwift,
        Target::HirPython,
        Target::HirHaskell,
        Target::HirPascal,
        Target::HirFaber,
        Target::HirFhir,
        Target::MirWasm,
        Target::MirLlvm,
        Target::MirLlvmHost,
        Target::MirAmd,
        Target::MirMetal,
        Target::MirWgsl,
        Target::MirSexp,
        Target::MirBasic,
        Target::MirScena,
        Target::MirFmir,
        Target::MirFmirBinary,
        Target::MirFmirBundle,
    ];

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
            Target::HirPascal => Some("hir-pascal"),
            Target::HirFaber => Some("hir-faber"),
            Target::HirFhir => Some("hir-fhir"),
            Target::MirMetal => Some("mir-metal"),
            Target::MirSexp => Some("mir-sexp"),
            Target::MirBasic => Some("mir-basic"),
            Target::MirAmd => Some("mir-amd"),
            Target::MirWasm => Some("mir-wasm"),
            Target::MirWgsl => Some("mir-wgsl"),
            Target::MirLlvm | Target::MirLlvmHost => Some("mir-llvm"),
            // Ungated variants, named (no `_`) so a future variant without
            // arms here is a compile error, never a silent `None`.
            Target::MirStepper
            | Target::MirScena
            | Target::MirFmir
            | Target::MirFmirBinary
            | Target::MirFmirBundle => None,
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
            Target::HirPascal => cfg!(feature = "hir-pascal"),
            Target::HirFaber => cfg!(feature = "hir-faber"),
            Target::HirFhir => cfg!(feature = "hir-fhir"),
            Target::MirMetal => cfg!(feature = "mir-metal"),
            Target::MirSexp => cfg!(feature = "mir-sexp"),
            Target::MirBasic => cfg!(feature = "mir-basic"),
            Target::MirAmd => cfg!(feature = "mir-amd"),
            Target::MirWasm => cfg!(feature = "mir-wasm"),
            Target::MirWgsl => cfg!(feature = "mir-wgsl"),
            Target::MirLlvm | Target::MirLlvmHost => cfg!(feature = "mir-llvm"),
            // Ungated variants, named (no `_`) so a future variant without
            // arms here is a compile error, never a silent `true`.
            Target::MirStepper
            | Target::MirScena
            | Target::MirFmir
            | Target::MirFmirBinary
            | Target::MirFmirBundle => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Target;

    /// One arm per variant, no wildcard: adding a `Target` variant fails
    /// this function's compilation, forcing the variant into the walk;
    /// removing one fails the same compilation. This match is the domain
    /// anchor that keeps [`Target::ALL`] honest.
    fn universe_spelling(target: Target) -> &'static str {
        match target {
            Target::MirStepper => "stepper",
            Target::HirRust => "rust",
            Target::HirTypeScript => "ts",
            Target::HirGo => "go",
            Target::HirSwift => "swift",
            Target::HirPython => "python",
            Target::HirHaskell => "haskell",
            Target::HirPascal => "pascal",
            Target::HirFaber => "faber",
            Target::HirFhir => "fhir",
            Target::MirWasm => "wasm-text",
            Target::MirLlvm => "llvm-text",
            Target::MirLlvmHost => "llvm-host",
            Target::MirAmd => "amd",
            Target::MirMetal => "metal-text",
            Target::MirWgsl => "wgsl-text",
            Target::MirSexp => "sexp",
            Target::MirBasic => "basic",
            Target::MirScena => "scena",
            Target::MirFmir => "fmir-text",
            Target::MirFmirBinary => "fmir",
            Target::MirFmirBundle => "fmir-bin",
        }
    }

    /// `ALL` set-equals the variant domain named by the exhaustive match
    /// (defect-audit P6-3/P6-4 guard shape). A variant missing from `ALL`
    /// drops its spelling; a duplicate or stale entry changes the vector —
    /// both fail here. Spellings are pinned to `Display` so the walk and
    /// the CLI spelling cannot drift apart either.
    #[test]
    fn all_is_exactly_the_variant_set() {
        let expected: Vec<&'static str> = [
            "stepper",
            "rust",
            "ts",
            "go",
            "swift",
            "python",
            "haskell",
            "pascal",
            "faber",
            "fhir",
            "wasm-text",
            "llvm-text",
            "llvm-host",
            "amd",
            "metal-text",
            "wgsl-text",
            "sexp",
            "basic",
            "scena",
            "fmir-text",
            "fmir",
            "fmir-bin",
        ]
        .into();
        let all: Vec<&'static str> = Target::ALL
            .iter()
            .map(|target| {
                let spelling = universe_spelling(*target);
                assert_eq!(spelling, target.to_string(), "spelling must match Display");
                spelling
            })
            .collect();
        assert_eq!(
            all, expected,
            "Target::ALL must be exactly the variant domain the exhaustive match names"
        );
    }

    /// `from_spelling` is the exact inverse of `Display`: every canonical
    /// spelling round-trips through the full universe, and no non-canonical
    /// input is accepted.
    #[test]
    fn from_spelling_round_trips_every_display_spelling() {
        for target in Target::ALL {
            let spelling = universe_spelling(*target);
            assert_eq!(
                Target::from_spelling(spelling),
                Some(*target),
                "spelling '{spelling}' must round-trip"
            );
        }
    }

    #[test]
    fn from_spelling_rejects_non_canonical_input() {
        for spelling in [
            "",
            "rust ",
            "Rust",
            "TS",
            "typescript",
            "wasm",
            "llvm",
            "not-a-target",
        ] {
            assert_eq!(
                Target::from_spelling(spelling),
                None,
                "spelling '{spelling}' must not parse"
            );
        }
    }
}
