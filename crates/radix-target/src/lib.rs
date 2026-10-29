//! Neutral compilation-target identity for Radix.
//!
//! Owns the [`Target`] enum, the variant universe ([`Target::ALL`]), its
//! CLI spelling ([`fmt::Display`]), the availability-gate spelling table
//! ([`Target::availability_spellings`]), the feature-gate mapping
//! ([`Target::required_feature`] / [`Target::is_available`]), and the
//! [`OutputMode`] switch. Target identity is a std-only contract shared by
//! semantic policy, the driver, services, and tests; it must not depend on
//! HIR, MIR, codegen, diagnostics, or any backend leaf.
//!
//! Consumers forward their own `hir-*` / `mir-*` Cargo features here 1:1 so
//! [`Target::is_available`] reflects the consumer's build, not this crate's.

use std::fmt;

mod leaf;
pub use leaf::Leaf;

/// Compilation target selected after analysis and before backend dispatch.
///
/// This enum is the public routing surface for drivers. Each variant commits to
/// a concrete backend module and a matching `Output` payload in `radix-module`.
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
    /// Novelty `GnuCOBOL` 3.2 free-format source emitted from checked HIR.
    /// Default-off; scalar programs only.
    HirCobol,
    /// Novelty Fortran 2008 source emitted from checked HIR (scalar subset and
    /// rank-1 fixed-shape tensors). Default-off; fails closed outside the subset.
    HirFortran,
    /// Novelty Odin source emitted from checked HIR (scalar-and-print
    /// registration spike). Default-off; fails closed outside the subset.
    HirOdin,
    /// Novelty C source emitted from checked HIR (integer-and-text spike
    /// with exact arithmetic). Default-off; fails closed outside the subset.
    HirC,

    /// Faber source re-emitted from analyzed HIR: the surface behind
    /// `faber format` and `faber convert`.
    HirFaber,

    /// Portable analyzed HIR artifact (postcard binary); serialized in emit
    /// workflow, not codegen.
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

    /// Public Cranelift native-object target (`faber build --target clif`).
    /// Emits a relocatable object for the host triple; Faber owns the link
    /// step against the `runtime/clif` archive.
    MirClif,

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
}

/// Output mode controlling access modifiers and entry-point emission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputMode {
    /// Emit a runnable application with `@main` entry point (default).
    Application,
    /// Emit a library module with `public` declarations and no `@main`.
    Library,
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Target {
    /// Canonical target spelling, shared by display and compiler services.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Target::MirStepper => "stepper",
            Target::HirRust => "rust",
            Target::HirTypeScript => "ts",
            Target::HirGo => "go",
            Target::HirSwift => "swift",
            Target::HirPython => "python",
            Target::HirHaskell => "haskell",
            Target::HirPascal => "pascal",
            Target::HirCobol => "cobol",
            Target::HirFortran => "fortran",
            Target::HirOdin => "odin",
            Target::HirC => "c",
            Target::HirFaber => "faber",
            Target::HirFhir => "fhir",
            Target::MirWasm => "wasm-text",
            Target::MirLlvm => "llvm-text",
            Target::MirLlvmHost => "llvm-host",
            Target::MirClif => "clif",
            Target::MirAmd => "amd",
            Target::MirMetal => "metal-text",
            Target::MirWgsl => "wgsl-text",
            Target::MirSexp => "sexp",
            Target::MirBasic => "basic",
            Target::MirScena => "scena",
            Target::MirFmir => "fmir-text",
            Target::MirFmirBinary => "fmir",
        }
    }

    /// Parse a canonical target spelling into a [`Target`].
    ///
    /// Exact inverse of [`fmt::Display`]: every canonical spelling
    /// round-trips, and nothing else is accepted. Caller-surface aliases
    /// (`typescript` for `ts`, `wasm` for `wasm-text`, …) are CLI
    /// conveniences owned by the caller, not canonical spellings.
    #[must_use]
    pub fn from_spelling(spelling: &str) -> Option<Self> {
        // WHY: derive from `Self::ALL` + `Self::as_str` so the spelling table
        // lives once; `all_is_exactly_the_variant_set` keeps `ALL` exhaustive
        // over the variant domain.
        Self::ALL
            .iter()
            .copied()
            .find(|target| target.as_str() == spelling)
    }

    /// Resolve the target word of a target-native import (`import python
    /// "math" m`) to its [`Target`].
    ///
    /// The word is a target CLI name: the canonical [`Self::as_str`] spelling
    /// or one of [`Self::availability_spellings`]. Target names are proper
    /// nouns and are never translated, so there is no locale hop here.
    #[must_use]
    pub fn from_import_word(word: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|target| {
            target.is_source_language()
                && (target.as_str() == word || target.availability_spellings().contains(&word))
        })
    }

    /// Whether the target emits a programming language whose modules a
    /// target-native import can name (not Faber itself, an IR, the runner,
    /// or a MIR text/device target).
    #[must_use]
    pub fn is_source_language(self) -> bool {
        matches!(
            self,
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
        )
    }

    /// Every spelling [`Self::from_import_word`] accepts, canonical first.
    pub fn import_words() -> impl Iterator<Item = &'static str> {
        Self::ALL
            .iter()
            .filter(|target| target.is_source_language())
            .flat_map(|target| {
                std::iter::once(target.as_str())
                    .chain(target.availability_spellings().iter().copied())
            })
    }

    /// The accepted import word nearest to a misspelled one, for a
    /// did-you-mean. `None` when nothing is within two edits.
    #[must_use]
    pub fn nearest_import_word(word: &str) -> Option<&'static str> {
        let mut best: Option<(usize, &'static str)> = None;
        for candidate in Self::import_words() {
            let distance = edit_distance(word, candidate);
            if distance <= 2 && best.is_none_or(|(d, _)| distance < d) {
                best = Some((distance, candidate));
            }
        }
        best.map(|(_, candidate)| candidate)
    }

    /// Spellings an availability gate (`@ nondum` / `@ unstable(target)`)
    /// may use to name this backend.
    ///
    /// A backend describes itself here; the semantic checker must not keep
    /// a second hand-typed table. `MirLlvm` and `MirLlvmHost` deliberately
    /// share the LLVM spellings.
    #[must_use]
    pub fn availability_spellings(self) -> &'static [&'static str] {
        match self {
            Target::MirStepper => &["stepper", "mir-stepper"],
            Target::HirRust => &["rs", "rust"],
            Target::HirFaber => &["faber", "fab"],
            Target::HirTypeScript => &["ts", "typescript"],
            Target::HirGo => &["go"],
            Target::HirSwift => &["swift"],
            Target::HirPython => &["python"],
            Target::HirHaskell => &["haskell"],
            Target::HirPascal => &["pascal", "pas"],
            Target::HirCobol => &["cobol", "cob"],
            Target::HirFortran => &["fortran", "f90"],
            Target::HirOdin => &["odin"],
            Target::HirC => &["c"],
            Target::HirFhir => &["fhir"],
            Target::MirWasm => &["wasm", "wasm-text", "wat"],
            Target::MirLlvm | Target::MirLlvmHost => &["llvm", "llvm-host", "llvm-text"],
            Target::MirClif => &["clif", "cranelift"],
            Target::MirMetal => &["metal", "metal-text"],
            Target::MirAmd => &["amd"],
            Target::MirWgsl => &["wgsl", "wgsl-text"],
            Target::MirSexp => &["sexp", "racket", "lisp", "rkt"],
            Target::MirBasic => &["basic", "bas"],
            Target::MirScena => &["scena"],
            Target::MirFmir => &["fmir-text"],
            Target::MirFmirBinary => &["fmir"],
        }
    }

    /// Every variant of this enum, in declaration order — the single owned
    /// source of the target universe (defect-audit P6-3).
    ///
    /// Any surface that must enumerate targets (report rows, completeness
    /// tests, discovery lists) starts from this list instead of keeping a
    /// second hand-typed copy. The list cannot silently drift from the
    /// enum: `all_is_exactly_the_variant_set` walks the variant domain
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
        Target::HirCobol,
        Target::HirFortran,
        Target::HirOdin,
        Target::HirC,
        Target::HirFaber,
        Target::HirFhir,
        Target::MirWasm,
        Target::MirLlvm,
        Target::MirLlvmHost,
        Target::MirClif,
        Target::MirAmd,
        Target::MirMetal,
        Target::MirWgsl,
        Target::MirSexp,
        Target::MirBasic,
        Target::MirScena,
        Target::MirFmir,
        Target::MirFmirBinary,
    ];

    /// The current device backends, in report order.
    ///
    /// Kernel requirement reports are indexed by this list, so the order
    /// (AMD, Metal, WGSL) is load-bearing and must not change.
    pub const DEVICE_TARGETS: [Self; 3] = [Self::MirAmd, Self::MirMetal, Self::MirWgsl];

    /// Whether this target is one of the portable device backends
    /// ([`Self::DEVICE_TARGETS`]).
    #[must_use]
    pub fn is_device_target(self) -> bool {
        Self::DEVICE_TARGETS.contains(&self)
    }

    /// Whether this target lowers wrapping kernel integers. Metal and WGSL
    /// support wrapping 32-bit integers in kernels; AMD does not.
    #[must_use]
    pub fn supports_wrapping_kernel_integers(self) -> bool {
        matches!(self, Self::MirMetal | Self::MirWgsl)
    }

    /// Whether this target's kernel emitter carries 64-bit integer and 128-bit
    /// `d64` values (operator ruling 2026-10-28). Metal lowers them with MSL
    /// `long`/`ulong`; WGSL and AMD refuse them by name until their emitters
    /// emulate 64-bit lanes.
    #[must_use]
    pub fn supports_wide_kernel_values(self) -> bool {
        matches!(self, Self::MirMetal)
    }

    /// Whether this target applies a ranged store policy
    /// (`saturating|trapping|wrapping<W, lo‥hi>`, need) at the store.
    /// Every other target would read the cell as its bare width and silently
    /// drop the range, so it is refused by name before an emitter runs.
    #[must_use]
    pub fn lowers_ranged_int(self) -> bool {
        matches!(self, Self::HirFaber | Self::HirRust | Self::MirStepper)
    }

    /// Whether this target applies a list length range (`list<T, size lo‥hi>`,
    /// need) where the list grows, shrinks and is built. Every other
    /// target would read the cell as a plain list and silently drop the
    /// range, so it is refused by name before an emitter runs.
    #[must_use]
    pub fn lowers_ranged_list(self) -> bool {
        matches!(self, Self::HirFaber | Self::HirRust | Self::MirStepper)
    }

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
            Target::HirCobol => Some("hir-cobol"),
            Target::HirFortran => Some("hir-fortran"),
            Target::HirOdin => Some("hir-odin"),
            Target::HirC => Some("hir-c"),
            Target::HirFaber => Some("hir-faber"),
            Target::HirFhir => Some("hir-fhir"),
            Target::MirMetal => Some("mir-metal"),
            Target::MirSexp => Some("mir-sexp"),
            Target::MirBasic => Some("mir-basic"),
            Target::MirAmd => Some("mir-amd"),
            Target::MirWasm => Some("mir-wasm"),
            Target::MirWgsl => Some("mir-wgsl"),
            Target::MirLlvm | Target::MirLlvmHost => Some("mir-llvm"),
            Target::MirClif => Some("mir-clif"),
            // Ungated variants, named (no `_`) so a future variant without
            // arms here is a compile error, never a silent `None`.
            Target::MirStepper | Target::MirScena | Target::MirFmir | Target::MirFmirBinary => None,
        }
    }

    /// Whether this target is available in the current Radix build.
    #[must_use]
    pub fn is_available(self) -> bool {
        match self {
            Target::HirRust => cfg!(feature = "hir-rust"),
            Target::HirTypeScript => cfg!(feature = "hir-ts"),
            Target::HirGo => cfg!(feature = "hir-go"),
            Target::HirSwift => cfg!(feature = "hir-swift"),
            Target::HirPython => cfg!(feature = "hir-python"),
            Target::HirHaskell => cfg!(feature = "hir-haskell"),
            Target::HirPascal => cfg!(feature = "hir-pascal"),
            Target::HirCobol => cfg!(feature = "hir-cobol"),
            Target::HirFortran => cfg!(feature = "hir-fortran"),
            Target::HirOdin => cfg!(feature = "hir-odin"),
            Target::HirC => cfg!(feature = "hir-c"),
            Target::HirFaber => cfg!(feature = "hir-faber"),
            Target::HirFhir => cfg!(feature = "hir-fhir"),
            Target::MirMetal => cfg!(feature = "mir-metal"),
            Target::MirSexp => cfg!(feature = "mir-sexp"),
            Target::MirBasic => cfg!(feature = "mir-basic"),
            Target::MirAmd => cfg!(feature = "mir-amd"),
            Target::MirWasm => cfg!(feature = "mir-wasm"),
            Target::MirWgsl => cfg!(feature = "mir-wgsl"),
            Target::MirLlvm | Target::MirLlvmHost => cfg!(feature = "mir-llvm"),
            Target::MirClif => cfg!(feature = "mir-clif"),
            // Ungated variants, named (no `_`) so a future variant without
            // arms here is a compile error, never a silent `true`.
            Target::MirStepper | Target::MirScena | Target::MirFmir | Target::MirFmirBinary => true,
        }
    }

    /// Whether this target's driver arm lowers a CLI entry program.
    ///
    /// A target that returns `false` is refused by name with `CODEGEN002`
    /// (`cli_codegen_rust_only`) before any emitter runs. Exhaustive, no
    /// wildcard: a future variant must declare its answer here.
    #[must_use]
    pub fn lowers_cli_entry(self) -> bool {
        match self {
            Target::HirRust
            | Target::HirGo
            | Target::HirTypeScript
            | Target::HirPython
            | Target::MirWasm
            | Target::MirClif => true,
            Target::MirStepper
            | Target::HirSwift
            | Target::HirHaskell
            | Target::HirPascal
            | Target::HirCobol
            | Target::HirFortran
            | Target::HirOdin
            | Target::HirC
            | Target::HirFaber
            | Target::HirFhir
            | Target::MirLlvm
            | Target::MirLlvmHost
            | Target::MirAmd
            | Target::MirMetal
            | Target::MirWgsl
            | Target::MirSexp
            | Target::MirBasic
            | Target::MirScena
            | Target::MirFmir
            | Target::MirFmirBinary => false,
        }
    }

    /// Leaves this target declares.
    ///
    /// A leaf is declared whole or not at all: partial coverage is never
    /// declared as a whole leaf, so any leaf missing here fails closed
    /// (`leaf_unsupported_on_target`). Rust, the runner family
    /// (`MirStepper`, `MirFmir*`), and Faber/FHIR share `faber-leaf` or
    /// preserve its calls and declare every leaf. Go declares every leaf: each
    /// has a standard-library port in the Go runtime. Clif's clock is served by
    /// the native runtime's `faber-leaf` conversations; every other leaf
    /// fails closed there.
    #[must_use]
    pub fn supported_leaves(self) -> &'static [Leaf] {
        match self {
            Target::HirRust
            | Target::MirStepper
            | Target::MirFmir
            | Target::MirFmirBinary
            | Target::HirFaber
            | Target::HirFhir
            | Target::HirGo => Leaf::ALL,
            Target::MirWasm => &[Leaf::Stdio],
            Target::MirClif => &[Leaf::Clock],
            Target::HirTypeScript
            | Target::HirSwift
            | Target::HirPython
            | Target::HirHaskell
            | Target::HirPascal
            | Target::HirCobol
            | Target::HirFortran
            | Target::HirOdin
            | Target::HirC
            | Target::MirLlvm
            | Target::MirLlvmHost
            | Target::MirAmd
            | Target::MirMetal
            | Target::MirWgsl
            | Target::MirSexp
            | Target::MirBasic
            | Target::MirScena => &[],
        }
    }

    /// Whether this target declares `leaf`.
    #[must_use]
    pub fn supports_leaf(self, leaf: Leaf) -> bool {
        self.supported_leaves().contains(&leaf)
    }
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;

/// Levenshtein distance over chars; the target-word did-you-mean is the only
/// caller, and the word sets are tiny.
fn edit_distance(left: &str, right: &str) -> usize {
    let left: Vec<char> = left.chars().collect();
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];
    for (i, l) in left.iter().enumerate() {
        current[0] = i + 1;
        for (j, r) in right.iter().enumerate() {
            let substitution = previous[j] + usize::from(l != r);
            current[j + 1] = substitution.min(current[j] + 1).min(previous[j + 1] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}
