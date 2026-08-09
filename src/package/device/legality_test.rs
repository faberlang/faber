use super::*;
use radix::mir::LoweredMirUnit;
use std::path::PathBuf;

/// The dev norma library home (the stdlib fixture path the package test
/// helpers share).
fn dev_norma_library_home() -> PathBuf {
    if let Some(home) = std::env::var_os("FABER_LIBRARY_HOME")
        .map(PathBuf::from)
        .filter(|path| path.join("norma/src").exists())
    {
        return home;
    }
    std::env::var("HOME")
        .map(PathBuf::from)
        .map(|home| home.join("work/ianzepp"))
        .unwrap_or_else(|_| PathBuf::from("/Users/ianzepp/work/ianzepp"))
}

/// Lower a source package and hand the lowered unit to `run` (the
/// `with_inline_package` pattern of the device test surface).
fn with_legality_package<R>(
    name: &str,
    source: &str,
    run: impl for<'a> FnOnce(&LoweredMirUnit<'a>) -> R,
) -> Result<R, Vec<Diagnostic>> {
    let root =
        std::env::temp_dir().join(format!("faber-legality-{}-{}", name, std::process::id()));
    std::fs::create_dir_all(root.join("src")).expect("temp fixture dir");
    let entry = root.join("src").join("probe.fab");
    std::fs::write(&entry, source).expect("write temp fixture");
    let config = radix::driver::Config::default()
        .with_stdlib(dev_norma_library_home())
        .with_target(radix::codegen::Target::MirFmirBinary);
    super::super::super::with_lowered_package_mir(&config, &entry, run)
}

/// DDCP1 fixture 7 / ddpp1-U6 — negative device side: a device function
/// containing `ad` through Sermo rejects DURING faber materialization with
/// the named structured diagnostic (`DeviceFunctionHostileEffect`-class /
/// issue `E_DEVICE_HOSTILE_EFFECT`) — deterministic rejection, never an
/// ad-hoc error.
#[test]
fn device_function_with_ad_rejects_during_materialization() {
    let error = with_legality_package(
        "device-ad-reject",
        r#"@ nucleum
functio ad_in_device(tf32[2] x) → tf32[2] {
    fixum _ conv ← ad 'runtime:echo' ("salve") ↦ vacuum
    redde x.multiplica(x)
}"#,
        |lowered| {
            device_program_for_lowered(
                &lowered.validated,
                &lowered.interner,
                &lowered.companions,
                super::super::DEFAULT_TRAINING_STEPS,
            )
            .expect_err("a device function with ad must reject during materialization")
        },
    )
    .expect("fixture lowers");
    assert!(
        error.iter().any(|diagnostic| {
            diagnostic.issue() == Some("E_DEVICE_HOSTILE_EFFECT")
                && diagnostic.message.contains("device-hostile effect")
                && diagnostic.message.contains("ad")
        }),
        "expected the named device-legality diagnostic naming the `ad` rule, got {error:?}"
    );
}

/// DDCP1 fixture 7 / ddpp1-U6 — positive CPU side (the preservation arm of
/// the DDCP1 hard gate, ddcp0-reconciliation §3.2): the equivalent CPU
/// function with ordinary host `ad` through Sermo is OUTSIDE the device
/// launch universe. It records no device effect fact, the device program
/// materializes unchanged, and the CPU function's MIR body retains the host
/// `ad` (`SermoOpen`) — preservation and rejection are proven together in
/// this same file set, never traded against each other.
#[test]
fn cpu_host_ad_through_sermo_runs_unchanged() {
    let (program, semantics, cpu_keeps_ad) = with_legality_package(
        "cpu-ad-preserve",
        r#"@ nucleum
functio soma(tf32[2] x) → tf32[2] {
    redde x.multiplica(x)
}

functio host_capability(numerus n) → numerus {
    fixum _ conv ← ad 'runtime:echo' ("salve") ↦ vacuum
    redde n
}"#,
        |lowered| {
            let materialized = device_program_for_lowered(
                &lowered.validated,
                &lowered.interner,
                &lowered.companions,
                super::super::DEFAULT_TRAINING_STEPS,
            )
            .expect("the device program materializes alongside the CPU host ad")
            .expect("a device kernel is present");
            // The CPU function is not a device launch: the materializer never
            // inspects or erases it — its MIR body keeps the host `ad`
            // (`SermoOpen`) unchanged.
            let cpu_keeps_ad = lowered
                .validated
                .program()
                .functions
                .iter()
                .find(|function| {
                    function.name.is_some_and(|symbol| {
                        lowered.interner.resolve(symbol) == "host_capability"
                    })
                })
                .is_some_and(|function| {
                    function.blocks.iter().any(|block| {
                        block.statements.iter().any(|statement| {
                            matches!(
                                &statement.kind,
                                MirStatementKind::RuntimeCall { call, .. }
                                    if matches!(call.intrinsic, MirIntrinsic::SermoOpen)
                            )
                        })
                    })
                });
            (materialized.0, materialized.1, cpu_keeps_ad)
        },
    )
    .expect("fixture lowers");
    assert!(
        cpu_keeps_ad,
        "the CPU host `ad` through Sermo must remain in the lowered MIR — runs unchanged"
    );
    program
        .validate_with_semantics(&semantics)
        .expect("the materialized device program stays valid alongside the CPU host ad");
}
