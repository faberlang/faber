//! Target capability tables and metadata for `radix targets` and `faber targets`.
//!
//! Owns [`discoverable_targets`], [`target_capabilities`] /
//! `*_for_surface`, [`target_name`], and [`target_extension`]; the
//! `cmd_targets` printers in `radix-cli` and `faber` consume these items
//! through the `radix` façade. Capability rows describe CLI support (check,
//! build, run, package) per backend. Delegates no compilation; this is a
//! static metadata registry with terminal output only.

/// The discoverable row list — the **single** copy (defect-audit P6-3).
///
/// This is a selection from the variant universe
/// [`Target::ALL`](crate::codegen::Target::ALL), not an independent
/// enumeration: the delta is exactly the deliberately hidden variants
/// (`MirAmd`, internal until the leaf proves a truthful baseline; `MirScena`,
/// hidden legacy runtime) plus `MirStepper`, which no CLI can select — no
/// `CliTarget`/`FaberCliTarget` spelling exists for it, so it is not a
/// discoverable `radix targets` row (defect-audit S10-12; goal Open
/// question 2 ruling: removed from discovery, note kept steering users at
/// `faber script`/`faber test`). `discoverable_targets_equal_universe_minus_hidden`
/// pins that set-equality. A `Target` variant added to the universe fails
/// this test until it joins this list or is named hidden — the list can no
/// longer silently drop a backend. The order is the report's presentation
/// order and is preserved verbatim for the printed row output.
const DISCOVERABLE_TARGET_ROWS: [crate::codegen::Target; 24] = [
    crate::codegen::Target::HirRust,
    crate::codegen::Target::MirFmir,
    crate::codegen::Target::MirFmirBinary,
    crate::codegen::Target::HirFaber,
    crate::codegen::Target::HirFhir,
    crate::codegen::Target::HirGo,
    crate::codegen::Target::HirSwift,
    crate::codegen::Target::HirPython,
    crate::codegen::Target::HirHaskell,
    crate::codegen::Target::HirPascal,
    crate::codegen::Target::HirCobol,
    crate::codegen::Target::HirFortran,
    crate::codegen::Target::HirOdin,
    crate::codegen::Target::HirC,
    crate::codegen::Target::MirWasm,
    crate::codegen::Target::MirLlvm,
    crate::codegen::Target::MirLlvmHost,
    crate::codegen::Target::MirClif,
    crate::codegen::Target::MirMetal,
    crate::codegen::Target::MirWgsl,
    crate::codegen::Target::MirSexp,
    crate::codegen::Target::MirBasic,
    crate::codegen::Target::MirEvm,
    crate::codegen::Target::HirTypeScript,
];

/// Capability row returned by [`target_capabilities`].
pub struct TargetCapabilities {
    /// Whether `check` is supported for this target.
    pub check: bool,
    /// Whether file-level `build` emission is supported.
    pub build: bool,
    /// Whether `faber run` can execute this target.
    pub run: bool,
    /// Whether `faber` package workflows are supported.
    pub package: bool,
    /// Human-readable capability note for `faber targets` / `radix targets`.
    pub note: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TargetCommandSurface {
    Radix,
    Faber,
}

/// The discoverable target rows, shared by the `radix targets` and
/// `faber targets` printers (re-exported through the `radix` façade).
/// One owner: this list plus [`DISCOVERABLE_TARGET_ROWS`]'s
/// set-equality test against the variant universe — no consumer keeps
/// its own copy.
#[must_use]
pub fn discoverable_targets() -> &'static [crate::codegen::Target] {
    &DISCOVERABLE_TARGET_ROWS
}

#[must_use]
pub fn target_extension(target: crate::codegen::Target) -> &'static str {
    match target {
        crate::codegen::Target::MirStepper => "stepper",
        crate::codegen::Target::HirRust => "rs",
        crate::codegen::Target::HirFaber => "fab",
        crate::codegen::Target::HirTypeScript => "ts",
        crate::codegen::Target::HirGo => "go",
        crate::codegen::Target::HirSwift => "swift",
        crate::codegen::Target::HirPython => "py",
        crate::codegen::Target::HirHaskell => "hs",
        crate::codegen::Target::HirPascal => "pas",
        crate::codegen::Target::HirCobol => "cob",
        crate::codegen::Target::HirFortran => "f90",
        crate::codegen::Target::HirOdin => "odin",
        crate::codegen::Target::HirC => "c",
        crate::codegen::Target::MirWasm => "wat",
        crate::codegen::Target::MirLlvm | crate::codegen::Target::MirAmd => "ll",
        crate::codegen::Target::MirLlvmHost => "run",
        crate::codegen::Target::MirClif => "o",
        crate::codegen::Target::MirMetal => "metal",
        crate::codegen::Target::MirWgsl => "wgsl",
        crate::codegen::Target::MirSexp => "rkt",
        crate::codegen::Target::MirBasic => "bas",
        crate::codegen::Target::MirEvm => "evm",
        crate::codegen::Target::MirScena => "scena",
        crate::codegen::Target::MirFmir => "fmir.txt",
        crate::codegen::Target::MirFmirBinary => "fmir",
        crate::codegen::Target::HirFhir => "fhir",
    }
}

/// Leaves this target declares. Empty means every native leaf is refused
/// at compile time (`leaf_unsupported_on_target`).
#[must_use]
pub fn target_leaves(target: crate::codegen::Target) -> &'static [crate::codegen::Leaf] {
    target.supported_leaves()
}

/// Comma-joined leaf spellings for `faber targets` / `radix targets`, or `-`.
#[must_use]
pub fn target_leaves_label(target: crate::codegen::Target) -> String {
    let leaves = target_leaves(target);
    if leaves.is_empty() {
        "-".to_owned()
    } else {
        leaves
            .iter()
            .map(|leaf| leaf.as_str())
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// Return the canonical CLI spelling owned by [`crate::codegen::Target`].
///
/// This includes hidden targets without a [`DISCOVERABLE_TARGET_ROWS`] entry.
#[must_use]
pub fn target_name(target: crate::codegen::Target) -> &'static str {
    target.as_str()
}

/// Return current command-surface support for a backend target.
///
/// These rows describe `CLI` capability, not necessarily backend maturity. For
/// example, a target can support file emission while package compilation still
/// remains unavailable from `radix`.
#[must_use]
pub fn target_capabilities(target: crate::codegen::Target) -> TargetCapabilities {
    target_capabilities_for_surface(TargetCommandSurface::Radix, target)
}

#[must_use]
pub fn target_capabilities_for_surface(
    surface: TargetCommandSurface,
    target: crate::codegen::Target,
) -> TargetCapabilities {
    let capabilities = match surface {
        TargetCommandSurface::Radix => radix_target_capabilities(target),
        TargetCommandSurface::Faber => faber_target_capabilities(target),
    };
    if target.is_available() {
        capabilities
    } else {
        // A capability row describes the command contract, but a target that
        // is not compiled into this binary cannot honestly claim that it can
        // build, run, or package. Keep `check=yes`: semantic checking remains
        // target-neutral, while all target-owned output/workflow operations
        // fail closed at the dispatch boundary with `target_unavailable`.
        TargetCapabilities { build: false, run: false, package: false, ..capabilities }
    }
}

fn radix_target_capabilities(target: crate::codegen::Target) -> TargetCapabilities {
    match target {
        crate::codegen::Target::MirStepper => TargetCapabilities {
            check: true,
            build: false,
            run: true,
            package: false,
            note: "core MIR interpreter target; use faber script/faber test rather than source emit",
        },
        crate::codegen::Target::HirRust => TargetCapabilities {
            check: true,
            build: true,
            run: true,
            package: true,
            note: "primary backend; full package build via `faber build`; `faber run` interprets source rather than running the Rust build",
        },
        crate::codegen::Target::HirFaber => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "canonical Faber re-emission for compiler inspection; package compilation not supported",
        },
        crate::codegen::Target::HirGo
        | crate::codegen::Target::HirSwift
        | crate::codegen::Target::HirPython
        | crate::codegen::Target::HirHaskell
        | crate::codegen::Target::HirTypeScript => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "file emission supported; package compilation not yet supported",
        },
        crate::codegen::Target::HirPascal => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "novelty HIR-backed Free Pascal program (scalar subset: int, float, bool, control flow, calls, print; fail-closed); default-off `hir-pascal` feature",
        },
        crate::codegen::Target::HirCobol => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "novelty GnuCOBOL 3.2 free-format emit (integer scalars, structured control flow, functions, print; fail-closed); default-off `hir-cobol` feature",
        },
        crate::codegen::Target::HirFortran => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "novelty HIR-backed Fortran 2008 program (scalar subset plus rank-1 fixed-shape tensors; fail-closed); default-off `hir-fortran` feature",
        },
        crate::codegen::Target::HirOdin => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "novelty HIR-backed Odin program (scalar-and-print registration spike; fail-closed); default-off `hir-odin` feature",
        },
        crate::codegen::Target::HirC => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "preview: readable C from checked HIR, output matches the runner, no undefined behavior emitted, unsupported constructs rejected by name; build with GCC or Clang; default in faber, outside full-targets",
        },
        crate::codegen::Target::MirWasm => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "supported MIR-backed WAT emit (fail-closed subset); binary conversion via external wat2wasm/wasm-tools; external host + faber_* imports; not faber run/package",
        },
        crate::codegen::Target::MirLlvm => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "supported MIR-backed LLVM IR staging (fail-closed subset); external toolchain verify/link; not native codegen or package runtime",
        },
        crate::codegen::Target::MirLlvmHost => TargetCapabilities {
            check: true,
            build: false,
            run: false,
            package: false,
            note: "native host executable product target delegated to `faber build --target llvm-host`; radix emit rejects this target",
        },
        crate::codegen::Target::MirClif => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "public MIR-backed Cranelift native object for the host triple (fail-closed subset; named gaps); linked against the `runtime/clif` archive by `faber build --target clif`",
        },
        crate::codegen::Target::MirAmd => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "MIR-backed AMDGPU LLVM IR emit for the explicit gfx942 row (mir-amd feature); NOT discoverable until the leaf proves a truthful baseline (OQ2 deferred target-support-matrix posture)",
        },
        crate::codegen::Target::MirMetal => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "supported MIR-backed Metal compute shader source (fail-closed device-safe subset); not GPU launch or package runtime",
        },
        crate::codegen::Target::MirWgsl => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "supported MIR-backed WGSL compute shader source (fail-closed device-safe subset); external naga validate; not GPU launch or package runtime",
        },
        crate::codegen::Target::MirSexp => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "supported MIR-backed Racket validation target (fail-closed subset); run via external racket; not a package runtime",
        },
        crate::codegen::Target::MirEvm => TargetCapabilities {
            check: true,
            build: false,
            run: false,
            package: false,
            note: "experimental Shanghai contracts with static scalar ABI; emit only; no storage or external calls; default-off mir-evm",
        },
        crate::codegen::Target::MirBasic => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "novelty MIR-backed classic line-numbered BASIC listing (Commodore 64 BASIC V2 compatible scalar subset; fail-closed); default-off `mir-basic` feature",
        },
        crate::codegen::Target::MirScena => TargetCapabilities {
            check: true,
            build: false,
            run: false,
            package: false,
            note: "hidden legacy package runtime delegated to `faber build --target scena`; radix emit rejects this target",
        },
        crate::codegen::Target::MirFmir => TargetCapabilities {
            check: true,
            build: false,
            run: false,
            package: false,
            note: "delegated to `faber build --target fmir-text`; radix emit rejects package MIR image targets",
        },
        crate::codegen::Target::MirFmirBinary => TargetCapabilities {
            check: true,
            build: false,
            run: false,
            package: false,
            note: "delegated to `faber build --target fmir`; radix emit rejects package MIR image targets",
        },
        crate::codegen::Target::HirFhir => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "portable analyzed HIR artifact via `radix emit -t fhir`; re-import through loader, no package or execution surface",
        },
    }
}

/// The Faber command-surface rows.
///
/// `run` is false in every row on purpose: `faber run` interprets source and
/// takes no `--target`, so no target is "run" by this table. The one target
/// route `faber run` reaches (a device selected with `--device`) is owned by
/// `DeviceChoice::run_target_spelling` in the `faber` crate, and `faber targets` sets
/// the `run` column from it. Keeping the flag here would be a second list.
fn faber_target_capabilities(target: crate::codegen::Target) -> TargetCapabilities {
    let mut capabilities = radix_target_capabilities(target);
    capabilities.run = false;
    match target {
        crate::codegen::Target::MirScena => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: true,
            note: "hidden legacy source-backed MIR package runtime; use `fmir-text` or `fmir` for package MIR artifacts",
        },
        crate::codegen::Target::MirFmir => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: true,
            note: "source-independent package MIR image via `faber build --target fmir-text`; `faber run` does not take the image",
        },
        crate::codegen::Target::MirFmirBinary => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: true,
            note: "compact package MIR binary image via `faber build --target fmir`; `faber run` does not take the image",
        },
        crate::codegen::Target::HirGo => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: true,
            note: "file emission; `faber build` on a package writes a self-contained Go module (`go` from PATH); `faber run --target go` builds and runs it",
        },
        crate::codegen::Target::HirTypeScript => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: true,
            note: "file emission; `faber build` on a package writes a self-contained Node project (`tsc` and `node` from PATH); `faber run --target ts` builds and runs it",
        },
        crate::codegen::Target::HirFhir => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: true,
            note: "portable FHIR package envelope via `faber build --target fhir`; `faber run` does not load the envelope",
        },
        // Faber device execution uses runtime backends rather than
        // target-specific text emitters. These rows describe the runtime
        // surface: `faber run --device <metal|cuda>` on the package source
        // (the FMIR image runner refuses an explicit device);
        // `-t metal-text` / `-t llvm-text` remain emit-only.
        crate::codegen::Target::MirMetal => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: true,
            note: "Metal device execution via `faber run --device metal` (in process, Apple GPUs); `-t metal-text` remains emit-only",
        },
        crate::codegen::Target::MirLlvm => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: true,
            note: "CUDA device execution via `faber run --device cuda` (in process through the NVIDIA driver; PTX built at run time with clang's NVPTX target; whole-tensor kernels, lane-form kernels execute through the NVVM lane emitter (statements, control flow, genus parameters, bounds faults, constant throws)); `-t llvm-text` remains emit-only",
        },
        crate::codegen::Target::HirRust => capabilities,
        // Stage 9 (llvm-host-parity product slice): llvm-host is the native
        // host executable product target. check/build/package are true on the
        // supported local host triple, and package is true because the Stage 8
        // package graph (one `.ll` per unit + entry + Norma modules) is green
        // through the shared package-to-LLVM builder. Build fails with a
        // structured diagnostic when llvm-as/clang/opt/archive is unavailable
        // or the host triple is unsupported (no cc fallback, no Rust codegen).
        crate::codegen::Target::MirLlvmHost => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: true,
            note: "native host executable via `faber build --target llvm-host` (MIR-to-LLVM emitter + llvm-as/clang from PATH + faber-host-llvm runtime archive on the local host triple); `faber run --target llvm-host` builds and runs it",
        },
        _ => {
            capabilities.package = false;
            capabilities
        }
    }
}
