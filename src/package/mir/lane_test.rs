//! FMIR e2e-hardening (CTO-1) lane tests: the two library-import blockers and
//! the PML4 acceptance fixture.
//!
//! (i) Method calls on linked library nominals (`p.dupla()`, `x.typus()`) now
//! link through synthetic-path calls instead of failing with "method call
//! before runtime/provider MIR lowering".
//! (ii) Const data members with static-ascription initializers
//! (`fixum f32 GELU_ALPHA ← (0.79… ∷ f32)`) transplant through the entry's
//! top-level-const seam instead of failing with "package MIR cannot transplant
//! const data member initializer with unsupported expression shape".
//! The PML4 composed loop (gradus/exempla/training-loop-mlp) executes on the
//! FMIR lane with its trajectory matching the train.proba U6 pins.

use super::*;
use radix::mir::BufferHost;
use radix::semantic::{Resolver, Symbol, SymbolKind};
use std::path::PathBuf;

const METHOD_LIB: &str = "genus Punctum {\n    numerus x\n\n    functio dupla() → numerus {\n        redde ego.x * 2\n    }\n}\n\nfunctio structa(numerus x) → Punctum {\n    redde Punctum { x = x }\n}\n";
const METHOD_MAIN: &str = "importa ex \"methlib:lib\" privata lib\n\nincipit {\n    fixum lib.Punctum p ← lib.structa(21)\n    nota p.dupla()\n}\n";

// The forma-shadowing guard (fd0c939) false-positive exclusion: a local
// `h1b` whose method call `h1b.gelu()` must NOT be rewritten into a
// synthetic-path namespace call to the imported `nn.gelu()` just because the
// unit imports a module that exports `gelu`. Pre-guard, the method-name sweep
// rewrote ANY HIR-generated receiver def (>= 1_000_000), so `h1b.gelu()` was
// rewritten to `nn.gelu(h1b)` — an arity-corrupting synthetic-path call.
const GELU_LIB: &str = "functio gelu() → numerus {\n    redde 7\n}\n";
const GELU_MAIN: &str = "importa ex \"gelulib:lib\" privata nn\n\ngenus H1B {\n    numerus x\n\n    functio gelu() → numerus {\n        redde ego.x * 2\n    }\n}\n\nfunctio structa(numerus x) → H1B {\n    redde H1B { x = x }\n}\n\nincipit {\n    fixum numerus nn_value ← nn.gelu()\n    fixum H1B h1b ← structa(21)\n    nota h1b.gelu()\n    nota nn_value\n}\n";

/// The `forma`-shadowing guard (fd0c939) excludes the `h1b.gelu()` false
/// positive: an ordinary method call on a local is NOT rewritten into a
/// synthetic-path namespace call when the method name happens to match an
/// imported module's export. The guard's three conditions (def.0 >= 1_000_000
/// AND SymbolKind::Local AND local-name in the imports) make `h1b` ineligible
/// (its name does not shadow an import binding), so the call runs as the local
/// genus method. Without the guard, the method-name sweep rewrites `h1b.gelu()`
/// → `nn.gelu(h1b)` and the package MIR fails — this test would fail.
#[test]
fn package_mir_shadowed_alias_rewrite_skips_plain_local_method_calls() {
    let dir = crate::package::test_support::test_temp_dir("shadowed-alias-skip");
    let entry = write_library_package(dir.as_ref(), "gelulib", GELU_LIB, GELU_MAIN);
    let config = Config::default().with_stdlib(dir.join("libhome"));

    let mut host = BufferHost::default();
    run_package_mir(&config, &entry, &mut host).expect("h1b.gelu() must not be rewritten into a namespace call");
    assert_eq!(
        host.stdout_lines,
        vec!["42".to_owned(), "7".to_owned()],
        "h1b.gelu() must run as the local genus method (42), nn.gelu() as the namespace call (7)"
    );
}

const VERTE_CONST_LIB: &str = "const f32 GELU_ALPHA ← (0.7978845608028654 ∷ f32)\n";
const VERTE_CONST_MAIN: &str = "importa ex \"verteclib:lib\" privata vals\n\nincipit {\n    nota vals.GELU_ALPHA\n}\n";

/// Write a package under a config library home (`libhome/<lib>/src`) with the
/// given library module and entry source, mirroring the S1-U1 constlib
/// fixture style.
fn write_library_package(
    dir: &std::path::Path,
    lib_name: &str,
    library_source: &str,
    main_source: &str,
) -> std::path::PathBuf {
    let src = dir.join("src");
    let library = dir.join("libhome").join(lib_name);
    fs::create_dir_all(&src).expect("create package src");
    fs::create_dir_all(library.join("src")).expect("create library src");
    fs::write(
        dir.join("faber.toml"),
        format!(
            r#"
[package]
name = "fhir-lane-{lib_name}"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
target = "fmir"
kind = "bin"

[dependencies]
{lib_name} = "1.0.0"
"#
        ),
    )
    .expect("write faber.toml");
    let library_display = library.display();
    let lock = format!(
        "[[package]]\nname = \"{lib_name}\"\nversion = \"1.0.0\"\nsource = \"path:{library_display}\"\npackage_root = \"{library_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{library_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n"
    );
    fs::write(dir.join("faber.lock"), lock).expect("write faber.lock");
    fs::write(library.join("src/lib.fab"), library_source).expect("write library module");
    fs::write(src.join("main.fab"), main_source).expect("write main.fab");
    src.join("main.fab")
}

#[test]
fn package_mir_library_nominal_method_call_runs() {
    // CTO-1 blocker (i): a method call on a linked library nominal
    // (`p.dupla()`) used to fail with "method call before runtime/provider MIR
    // lowering". The link pass now registers the library's genus methods with
    // synthetic sources and rewrites `receiver.method(args)` to a
    // synthetic-path call; the struct literal inside the library (Punctum { x
    // = x }) and the field projection (ego.x) also ride the merged nominal
    // remap.
    let dir = crate::package::test_support::test_temp_dir("cto1-method");
    let entry = write_library_package(dir.as_ref(), "methlib", METHOD_LIB, METHOD_MAIN);
    let config = Config::default().with_stdlib(dir.join("libhome"));

    let mut host = BufferHost::default();
    run_package_mir(&config, &entry, &mut host).expect("library method call must run");
    assert_eq!(
        host.stdout_lines,
        vec!["42".to_owned()],
        "the library method must return its oracle"
    );
}

#[test]
fn package_mir_const_data_member_verte_initializer_transplants() {
    // CTO-1 blocker (ii): a linked library const whose initializer uses a
    // static type ascription (`(0.7978845608028654 ∷ f32)`) used to fail with
    // "package MIR cannot transplant const data member initializer with
    // unsupported expression shape". The transplant now carries the Verte
    // shape (source + target type + object entries) through the entry's
    // top-level-const seam.
    let dir = crate::package::test_support::test_temp_dir("cto1-verte-const");
    let entry = write_library_package(dir.as_ref(), "verteclib", VERTE_CONST_LIB, VERTE_CONST_MAIN);
    let config = Config::default().with_stdlib(dir.join("libhome"));

    let mut host = BufferHost::default();
    run_package_mir(&config, &entry, &mut host).expect("∷-cast const member must run");
    let value: f64 = host.stdout_lines[0].parse().expect("the const value prints");
    assert!(
        (value - 0.7978845608028654).abs() < 1e-4,
        "the ascription-cast const must keep its value, got {value}"
    );
}

/// Recursively copy a package directory, skipping `target/` build artifacts
/// (mirrors the bert-tiny image-test helper).
fn copy_package_dir_skipping_target(src: &std::path::Path, dest: &std::path::Path) {
    for entry in std::fs::read_dir(src).expect("read package dir") {
        let entry = entry.expect("read dir entry");
        let dest_path = dest.join(entry.file_name());
        if entry.path().is_dir() {
            if entry.file_name() == "target" {
                continue;
            }
            std::fs::create_dir_all(&dest_path).expect("create sub dir");
            copy_package_dir_skipping_target(&entry.path(), &dest_path);
        } else {
            std::fs::copy(entry.path(), dest_path).expect("copy file");
        }
    }
}

/// The PML4-U6 accepted trajectory (train.proba U6 pins; f64 oracle
/// evaluations of the documented convergence — compared under the documented
/// 5e-4 absolute tolerance against the f32 self-hosted stepper output).
const PML4_PINS: [(usize, f64); 6] = [
    (0, 1.576448169383708),
    (10, 0.7815377070077427),
    (25, 0.4303461875641296),
    (50, 0.13848813116166797),
    (75, 0.04746405569680761),
    (99, 0.017928625511508454),
];

#[test]
// Re-enabled with the LIB-MIR landing (radix 43c0102ba: textus.accipe
// intrinsic + SizedNumeric zero-init arm): the FMIR stepper now lowers
// library-to-library calls in the gradus multi-module library, so the PML4
// executed-lane acceptance runs again (TARGETLANE001 a42bc97 + LIB-MIR).
fn package_mir_training_loop_mlp_runs_on_fmir_lane() {
    // CTO-1 acceptance: the PML4 composed loop (gradus/exempla/
    // training-loop-mlp — a 4×4 two-layer MLP, 100 steps, lr 0.1, the
    // accepted MLP training-proof workload) executes on the FMIR lane
    // (in-memory interpreted package MIR) and its trajectory matches the
    // train.proba U6 pins. This is the executed-evidence prerequisite for the
    // auditor-owned runtime-evidence gate.
    //
    // Re-enabled with TARGETLANE001 executed-lane routing: the executed-
    // package analysis resolves to a MIR-backed target — the manifest
    // `[build] target = "fmir"` propagated to the analysis config — so the
    // air-lane functions pass the lane policy at analysis instead of tripping
    // `lane_requires_mir_backed_target` under the old default `HirRust`.
    let exemplum =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../gradus/exempla/training-loop-mlp");
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let dir = crate::package::test_support::test_temp_dir("pml4-lane");
    copy_package_dir_skipping_target(&exemplum, dir.path());

    let config = radix::driver::Config::default().with_stdlib(workspace);
    let entry = dir.path().join("src/main.fab");
    let mut host = BufferHost::default();
    run_package_mir(&config, &entry, &mut host).expect("the PML4 loop must run on the FMIR lane");

    // The loop prints one `nota metrica.damnum()` per step (lines 0..100),
    // then the checkpoint round-trip and the full loss trace.
    assert!(
        host.stdout_lines.len() >= 101,
        "expected 100 per-step losses plus the checkpoint line, got {} lines",
        host.stdout_lines.len()
    );
    for (step, pin) in PML4_PINS {
        let actual: f64 = host.stdout_lines[step]
            .parse()
            .unwrap_or_else(|_| panic!("step {step} loss must be numeric"));
        assert!(
            (actual - pin).abs() <= 5e-4,
            "step {step}: f32 stepper loss {actual} must match the f64 oracle pin {pin} within 5e-4"
        );
    }
    let initial: f64 = host.stdout_lines[0].parse().expect("initial loss numeric");
    let final_loss: f64 = host.stdout_lines[99].parse().expect("final loss numeric");
    assert!(
        final_loss / initial < 0.1,
        "convergence ratio {final_loss}/{initial} = {} must be < 0.1",
        final_loss / initial
    );
}

/// The `forma`-shadowing shape (fd0c939 guard positive condition): a receiver
/// def in the HIR-generated range (>= 1_000_000) whose symbol is a Local
/// whose name appears in the unit's imports resolves to the registered
/// synthetic namespace target — the rewrite still fires.
#[test]
fn shadowed_alias_shadowing_receiver_rewrites_when_local_name_is_in_imports() {
    let mut interner = Interner::default();
    let mut resolver = Resolver::new();
    let unit_path = PathBuf::from("<shadowed-alias-test>.fab");

    // The shadowing receiver: a HIR-generated local def whose symbol is a
    // Local named `forma` — the exact name of an import binding.
    let receiver_def = DefId(1_000_000);
    let forma = interner.intern("forma");
    // Fresh resolver: a symbol with this def/name cannot already exist, so the
    // define cannot collide; discard the result (hygiene: no .expect).
    resolver
        .define(Symbol {
            def_id: receiver_def,
            name: forma,
            kind: SymbolKind::Local,
            ty: None,
            mutable: false,
            span: Default::default(),
        })
        .ok();

    // The import binding `forma` → the imported module's item def, plus a
    // registered namespace-call target for `forma.quantitas(...)`.
    let import_def = DefId(42);
    let mut imports = HashMap::new();
    imports.insert("forma".to_owned(), import_def);
    let synthetic = DefId(2_000_000_000);
    let mut targets: NamespaceCallTargets = HashMap::new();
    targets.insert((unit_path.clone(), import_def, "quantitas".to_owned()), synthetic);

    let receiver = HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Path(receiver_def),
        ty: None,
        span: Default::default(),
    };

    let method = interner.intern("quantitas");
    let mut diagnostics = Vec::new();
    let rewriter = super::link::ShadowedAliasRewriter {
        unit_path: &unit_path,
        resolver: &resolver,
        interner: &interner,
        targets: &targets,
        imports: &imports,
        diagnostics: &mut diagnostics,
    };
    assert_eq!(
        rewriter.import_target(&receiver, method),
        Some(synthetic),
        "the forma-shadowing receiver (Local def >= 1_000_000 whose name appears in the imports) must resolve to the synthetic namespace target"
    );
}

/// The `h1b.gelu()` false-positive exclusion (fd0c939): a local def in the
/// HIR-generated range whose name does NOT appear in the unit's imports is NOT
/// rewritten — even when the method name matches an imported module's export
/// (`nn` exports `gelu`). Without the guard this call was rewritten into a
/// synthetic-path namespace call and corrupted AIR-lane library functions at
/// lowering.
#[test]
fn shadowed_alias_plain_local_method_call_is_not_rewritten() {
    let mut interner = Interner::default();
    let mut resolver = Resolver::new();
    let unit_path = PathBuf::from("<plain-local-test>.fab");

    let receiver_def = DefId(1_000_001);
    let h1b = interner.intern("h1b");
    resolver
        .define(Symbol {
            def_id: receiver_def,
            name: h1b,
            kind: SymbolKind::Local,
            ty: None,
            mutable: false,
            span: Default::default(),
        })
        .ok();

    // The unit imports a module (`nn`) that DOES export `gelu` and the target
    // is registered — the guard must still skip the h1b receiver because the
    // local's name does not shadow an import binding.
    let import_def = DefId(7);
    let mut imports = HashMap::new();
    imports.insert("nn".to_owned(), import_def);
    let synthetic = DefId(2_000_000_000);
    let mut targets: NamespaceCallTargets = HashMap::new();
    targets.insert((unit_path.clone(), import_def, "gelu".to_owned()), synthetic);

    let receiver = HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Path(receiver_def),
        ty: None,
        span: Default::default(),
    };

    let method = interner.intern("gelu");
    let mut diagnostics = Vec::new();
    let rewriter = super::link::ShadowedAliasRewriter {
        unit_path: &unit_path,
        resolver: &resolver,
        interner: &interner,
        targets: &targets,
        imports: &imports,
        diagnostics: &mut diagnostics,
    };
    assert_eq!(
        rewriter.import_target(&receiver, method),
        None,
        "h1b.gelu() must not be rewritten: the local name does not shadow an import binding"
    );
}
