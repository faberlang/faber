use super::{OutputMode, Target};

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
        "cobol",
        "fortran",
        "odin",
        "c",
        "faber",
        "fhir",
        "wasm-text",
        "llvm-text",
        "llvm-host",
        "clif",
        "amd",
        "metal-text",
        "wgsl-text",
        "sexp",
        "basic",
        "scena",
        "fmir-text",
        "fmir",
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

/// Every target declares availability spellings, and the only spelling
/// shared across targets is the deliberate LLVM family sharing between
/// `MirLlvm` and `MirLlvmHost`. A duplicate inside one slice also fails:
/// sharing is only legal between the two LLVM variants.
#[test]
fn availability_spellings_cover_every_target() {
    let mut seen: Vec<(&'static str, Target)> = Vec::new();
    for target in Target::ALL {
        let spellings = target.availability_spellings();
        assert!(
            !spellings.is_empty(),
            "target {target} must declare at least one availability spelling"
        );
        for &spelling in spellings {
            for (existing, owner) in &seen {
                if *existing != spelling {
                    continue;
                }
                let llvm_share = *owner != *target
                    && matches!(owner, Target::MirLlvm | Target::MirLlvmHost)
                    && matches!(target, Target::MirLlvm | Target::MirLlvmHost);
                assert!(
                    llvm_share,
                    "spelling '{spelling}' is shared by {owner} and {target}"
                );
            }
            seen.push((spelling, *target));
        }
    }
}

/// The device backend list is exactly AMD, Metal, WGSL in that order:
/// kernel requirement reports are indexed by it, so order is load-bearing.
#[test]
fn device_targets_are_the_three_device_backends() {
    assert_eq!(
        Target::DEVICE_TARGETS,
        [Target::MirAmd, Target::MirMetal, Target::MirWgsl]
    );
    for target in Target::ALL {
        let expected = Target::DEVICE_TARGETS.contains(target);
        assert_eq!(
            target.is_device_target(),
            expected,
            "is_device_target must agree with DEVICE_TARGETS for {target}"
        );
    }
}

/// Wide (64-bit integer, `d64`) kernel values are a Metal capability; WGSL and
/// AMD refuse them by name until their emitters emulate 64-bit lanes.
#[test]
fn wide_kernel_values_are_metal_only() {
    for target in Target::ALL {
        let expected = matches!(target, Target::MirMetal);
        assert_eq!(
            target.supports_wide_kernel_values(),
            expected,
            "supports_wide_kernel_values for {target}"
        );
    }
}

/// Wrapping kernel integers are a Metal and WGSL capability; AMD is not a
/// wrapping backend even though it is a device target.
#[test]
fn wrapping_kernel_integers_are_metal_and_wgsl_only() {
    for target in Target::ALL {
        let expected = matches!(target, Target::MirMetal | Target::MirWgsl);
        assert_eq!(
            target.supports_wrapping_kernel_integers(),
            expected,
            "wrapping-kernel-integer support mismatch for {target}"
        );
    }
}

/// The Rust and Wasm gates keep their historical spellings after the table
/// moved out of the semantic checker.
#[test]
fn availability_spellings_pin_rust_and_wasm() {
    let rust = Target::HirRust.availability_spellings();
    assert!(rust.contains(&"rs"), "rust spellings: {rust:?}");
    assert!(rust.contains(&"rust"), "rust spellings: {rust:?}");
    assert!(
        Target::MirWasm.availability_spellings().contains(&"wat"),
        "wasm spellings must include wat"
    );
}

/// The two output modes are distinct values: generation paths compare them
/// directly (`OutputMode::Library` selects public declarations, no `@main`).
#[test]
fn output_mode_variants_are_distinct() {
    assert_ne!(OutputMode::Application, OutputMode::Library);
}

/// The CLI-entry capability is exactly Rust, TypeScript, Python, Go, wasm
/// text, and the CLIF object emitter: the driver gate refuses every other target by name with
/// `cli_codegen_rust_only`.
#[test]
fn lowers_cli_entry_is_exactly_rust_ts_py_go_wasm_clif() {
    let capable: Vec<Target> = Target::ALL
        .iter()
        .copied()
        .filter(|target| target.lowers_cli_entry())
        .collect();
    assert_eq!(
        capable,
        [
            Target::HirRust,
            Target::HirTypeScript,
            Target::HirGo,
            Target::HirPython,
            Target::MirWasm,
            Target::MirClif
        ]
    );
}

#[test]
fn import_words_name_source_languages_only() {
    assert_eq!(Target::from_import_word("python"), Some(Target::HirPython));
    assert_eq!(Target::from_import_word("ts"), Some(Target::HirTypeScript));
    assert_eq!(
        Target::from_import_word("typescript"),
        Some(Target::HirTypeScript)
    );
    assert_eq!(Target::from_import_word("rs"), Some(Target::HirRust));
    assert_eq!(Target::from_import_word("go"), Some(Target::HirGo));
    // Not languages whose modules an import can name.
    for word in [
        "faber",
        "fhir",
        "stepper",
        "fmir",
        "wasm-text",
        "metal-text",
        "sexp",
    ] {
        assert_eq!(Target::from_import_word(word), None, "{word}");
    }
    assert_eq!(Target::nearest_import_word("pyton"), Some("python"));
    assert_eq!(Target::nearest_import_word("goo"), Some("go"));
    assert_eq!(Target::nearest_import_word("zzzzzzzz"), None);
}
