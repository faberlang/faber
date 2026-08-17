//! Target capability tables and metadata for `radix targets` and `faber targets`.
//!
//! Owns [`cmd_targets`], [`discoverable_targets`], [`target_capabilities`] /
//! `*_for_surface`, [`target_name`], and [`target_extension`]. Capability rows
//! describe CLI support (check, build, run, package) per backend. Delegates no
//! compilation; this is a static metadata registry with terminal output only.

const DISCOVERABLE_TARGET_ROWS: [crate::codegen::Target; 18] = [
    crate::codegen::Target::MirStepper,
    crate::codegen::Target::HirRust,
    crate::codegen::Target::MirFmir,
    crate::codegen::Target::MirFmirBinary,
    crate::codegen::Target::MirFmirBundle,
    crate::codegen::Target::HirFaber,
    crate::codegen::Target::HirFhir,
    crate::codegen::Target::HirGo,
    crate::codegen::Target::HirSwift,
    crate::codegen::Target::HirPython,
    crate::codegen::Target::HirHaskell,
    crate::codegen::Target::MirWasm,
    crate::codegen::Target::MirLlvm,
    crate::codegen::Target::MirLlvmHost,
    crate::codegen::Target::MirMetal,
    crate::codegen::Target::MirWgsl,
    crate::codegen::Target::MirSexp,
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

/// Print backend capability rows for terminal discovery.
pub fn cmd_targets() {
    for &target in discoverable_targets() {
        let capabilities = target_capabilities_for_surface(TargetCommandSurface::Radix, target);
        println!(
            "{} available={} check={} build={} run={} package={} note={}",
            target_name(target),
            yes_no(target.is_available()),
            yes_no(capabilities.check),
            yes_no(capabilities.build),
            yes_no(capabilities.run),
            yes_no(capabilities.package),
            capabilities.note
        );
    }
}

#[must_use]
pub(crate) fn discoverable_targets() -> &'static [crate::codegen::Target] {
    &DISCOVERABLE_TARGET_ROWS
}

#[must_use]
pub(crate) fn target_extension(target: crate::codegen::Target) -> &'static str {
    match target {
        crate::codegen::Target::MirStepper => "stepper",
        crate::codegen::Target::HirRust => "rs",
        crate::codegen::Target::HirFaber => "fab",
        crate::codegen::Target::HirTypeScript => "ts",
        crate::codegen::Target::HirGo => "go",
        crate::codegen::Target::HirSwift => "swift",
        crate::codegen::Target::HirPython => "py",
        crate::codegen::Target::HirHaskell => "hs",
        crate::codegen::Target::MirWasm => "wat",
        crate::codegen::Target::MirLlvm => "ll",
        crate::codegen::Target::MirLlvmHost => "run",
        crate::codegen::Target::MirAmd => "ll",
        crate::codegen::Target::MirMetal => "metal",
        crate::codegen::Target::MirWgsl => "wgsl",
        crate::codegen::Target::MirSexp => "rkt",
        crate::codegen::Target::MirScena => "scena",
        crate::codegen::Target::MirFmir => "fmir.txt",
        crate::codegen::Target::MirFmirBinary => "fmir",
        crate::codegen::Target::MirFmirBundle => "run",
        crate::codegen::Target::HirFhir => "fhir",
    }
}

#[must_use]
pub(crate) fn target_name(target: crate::codegen::Target) -> &'static str {
    match target {
        crate::codegen::Target::MirStepper => "stepper",
        crate::codegen::Target::HirRust => "rust",
        crate::codegen::Target::HirFaber => "faber",
        crate::codegen::Target::HirTypeScript => "ts",
        crate::codegen::Target::HirGo => "go",
        crate::codegen::Target::HirSwift => "swift",
        crate::codegen::Target::HirPython => "python",
        crate::codegen::Target::HirHaskell => "haskell",
        crate::codegen::Target::MirWasm => "wasm-text",
        crate::codegen::Target::MirLlvm => "llvm-text",
        crate::codegen::Target::MirLlvmHost => "llvm-host",
        crate::codegen::Target::MirAmd => "amd",
        crate::codegen::Target::MirMetal => "metal-text",
        crate::codegen::Target::MirWgsl => "wgsl-text",
        crate::codegen::Target::MirSexp => "sexp",
        crate::codegen::Target::MirScena => "scena",
        crate::codegen::Target::MirFmir => "fmir-text",
        crate::codegen::Target::MirFmirBinary => "fmir",
        crate::codegen::Target::MirFmirBundle => "fmir-bin",
        crate::codegen::Target::HirFhir => "fhir",
    }
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
            note: "primary backend; full package build + run via `faber`",
        },
        crate::codegen::Target::HirFaber => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "canonical Faber re-emission for compiler inspection; package compilation not supported",
        },
        crate::codegen::Target::HirGo => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "file emission supported; package compilation not yet supported",
        },
        crate::codegen::Target::HirSwift => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "file emission supported; package compilation not yet supported",
        },
        crate::codegen::Target::HirPython => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "file emission supported; package compilation not yet supported",
        },
        crate::codegen::Target::HirHaskell => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "file emission supported; package compilation not yet supported",
        },
        crate::codegen::Target::HirTypeScript => TargetCapabilities {
            check: true,
            build: true,
            run: false,
            package: false,
            note: "file emission supported; package compilation not yet supported",
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
            note: "native host executable product target delegated to `faber build/run --target llvm-host`; radix emit rejects this target",
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
        crate::codegen::Target::MirFmirBundle => TargetCapabilities {
            check: true,
            build: false,
            run: false,
            package: false,
            note: "delegated to `faber build --target fmir-bin`; radix emit rejects package executable targets",
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

fn faber_target_capabilities(target: crate::codegen::Target) -> TargetCapabilities {
    let mut capabilities = radix_target_capabilities(target);
    match target {
        crate::codegen::Target::MirScena => TargetCapabilities {
            check: true,
            build: true,
            run: true,
            package: true,
            note: "hidden legacy source-backed MIR package runtime; use `fmir-text`, `fmir`, or `fmir-bin` for package MIR artifacts",
        },
        crate::codegen::Target::MirFmir => TargetCapabilities {
            check: true,
            build: true,
            run: true,
            package: true,
            note: "source-independent package MIR image via `faber build --target fmir-text`; run through FMIR loader",
        },
        crate::codegen::Target::MirFmirBinary => TargetCapabilities {
            check: true,
            build: true,
            run: true,
            package: true,
            note: "compact package MIR binary image via `faber build --target fmir`; run through FMIR loader",
        },
        crate::codegen::Target::MirFmirBundle => TargetCapabilities {
            check: true,
            build: true,
            run: true,
            package: true,
            note: "direct package executable runner via `faber build --target fmir-bin`; embeds FMIR image bytes",
        },
        crate::codegen::Target::HirFhir => TargetCapabilities {
            check: true,
            build: true,
            run: true,
            package: true,
            note: "portable FHIR package envelope via `faber build --target fhir`; load and lower to FMIR for run (Rust stays explicit)",
        },
        // Differentiable-GPU campaign N1.8 (S1-6 → Stage 7, accepted): the
        // ordinary-command gate (E5) then the RC gates (E6 — RC binary on
        // both machines; E7 — clean-room extracted-archive on both machines)
        // passed on the named machines at the pinned revisions (Metal on
        // Apple M5 Max / build host; CUDA on NVIDIA RTX 5070 / build host; faber
        // 1.6.0-rc.1), so the Metal and CUDA device-execution rows carry
        // `run=yes` and `package=yes` at RC level (RC-local posture). No E8,
        // no stable-publication, no broad-hardware wording: the rows are
        // scoped to the named machines at the pinned revisions. The `-t`
        // emit surface is unchanged: device execution is selected by
        // `faber run --backend <metal|cuda>` or the archive FMIR image
        // runner, never by `-t metal-text` / `-t llvm-text` (A6).
        crate::codegen::Target::MirMetal => TargetCapabilities {
            check: true,
            build: true,
            run: true,
            package: true,
            note: "Metal device execution at RC level on Apple M5 Max / build host (faber 1.6.0-rc.1, pinned revisions; E6/E7 receipts): `faber run --backend metal` + archive compiled FMIR images; `-t metal-text` remains emit-only",
        },
        crate::codegen::Target::MirLlvm => TargetCapabilities {
            check: true,
            build: true,
            run: true,
            package: true,
            note: "CUDA device execution at RC level on NVIDIA RTX 5070 / build host (faber 1.6.0-rc.1, pinned revisions; E6/E7 receipts): `faber run --backend cuda` + archive compiled FMIR images; `-t llvm-text` remains emit-only",
        },
        crate::codegen::Target::HirRust => capabilities,
        // Stage 9 (llvm-host-parity product slice): llvm-host is the native
        // host executable product target. check/build/run are true on the
        // supported local host triple, and package is true because the Stage 8
        // package graph (one `.ll` per unit + entry + Norma modules) is green
        // through the shared package-to-LLVM builder. Build fails with a
        // structured diagnostic when llvm-as/clang/opt/archive is unavailable
        // or the host triple is unsupported (no cc fallback, no Rust codegen).
        crate::codegen::Target::MirLlvmHost => TargetCapabilities {
            check: true,
            build: true,
            run: true,
            package: true,
            note: "native host executable via `faber build/run --target llvm-host` (MIR-to-LLVM emitter + llvm-as/clang + faber-host-llvm runtime archive on the local host triple)",
        },
        _ => {
            capabilities.package = false;
            capabilities.run = false;
            capabilities
        },
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}
