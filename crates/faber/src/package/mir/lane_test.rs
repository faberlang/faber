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
use radix::lexer::Span;
use radix::mir::BufferHost;
use radix::mir::{MirConversion, MirConversionFlavor};
use radix::semantic::Resolver;
use std::path::PathBuf;

const METHOD_LIB: &str = "genus Punctum {\n    numerus x\n\n    functio dupla() → numerus {\n        redde ego.x * 2\n    }\n}\n\nfunctio structa(numerus x) → Punctum {\n    redde Punctum { x = x }\n}\n";
const METHOD_MAIN: &str = "import from \"methlib:lib\" lib\n\nmain {\n    const lib.Punctum p ← lib.structa(21)\n    print p.dupla()\n}\n";

const CALLBACK_LIB: &str = "genus Lectio {\n    bivalens successus\n    numerus valor\n    textus causa\n}\n\nfunctio applica((numerus, numerus) → Lectio f, numerus x) → numerus {\n    fixum Lectio lectio ← f(x, 2)\n    si non lectio.successus ergo redde 0\n    redde lectio.valor\n}\n";
const CALLBACK_MAIN: &str = "importa ex \"callbacklib:lib\" lib\n\nincipit {\n    fixum numerus factor ← 1\n    nota lib.applica((numerus x, numerus y) → lib.Lectio ∴ lib.Lectio { successus = verum, valor = x * y * factor, causa = \"\" }, 21)\n}\n";

// The forma-shadowing guard (fd0c939) false-positive exclusion: a local
// `h1b` whose method call `h1b.gelu()` must NOT be rewritten into a
// synthetic-path namespace call to the imported `nn.gelu()` just because the
// unit imports a module that exports `gelu`. Pre-guard, the method-name sweep
// rewrote ANY HIR-generated receiver def (>= 1_000_000), so `h1b.gelu()` was
// rewritten to `nn.gelu(h1b)` — an arity-corrupting synthetic-path call.
const GELU_LIB: &str = "functio gelu() → numerus {\n    redde 7\n}\n";
const GELU_MAIN: &str = "import from \"gelulib:lib\" nn\n\nclass H1B {\n    int x\n\n    fn gelu() → int {\n        return self.x * 2\n    }\n}\n\nfn structa(int x) → H1B {\n    return H1B { x = x }\n}\n\nmain {\n    const int nn_value ← nn.gelu()\n    const H1B h1b ← structa(21)\n    print h1b.gelu()\n    print nn_value\n}\n";

#[test]
fn package_mir_rewrites_runtime_conversion_types_with_imported_types() {
    let mut statement = MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: None,
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Convert(MirConversion {
                    flavor: MirConversionFlavor::Runtime,
                    source_ty: MirType::semantic(TypeId(1)),
                    target_ty: MirType::semantic(TypeId(2)),
                    params: Vec::new(),
                    recovery: None,
                    struct_defaults: Vec::new(),
                }),
                args: vec![MirOperand::Constant(MirConstant::Int(1))],
                return_ty: MirType::semantic(TypeId(2)),
            },
        },
        span: Span::default(),
    };

    super::lower::rewrite_statement_type_id(
        &mut statement,
        &[(TypeId(1), TypeId(10)), (TypeId(2), TypeId(11))],
    );

    let MirStatementKind::RuntimeCall { call, .. } = statement.kind else {
        panic!("expected a runtime call");
    };
    let MirIntrinsic::Convert(conversion) = call.intrinsic else {
        panic!("expected a conversion intrinsic");
    };
    assert_eq!(conversion.source_ty.semantic_id(), TypeId(10));
    assert_eq!(conversion.target_ty.semantic_id(), TypeId(11));
    assert_eq!(call.return_ty.semantic_id(), TypeId(11));
}

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
    run_package_mir(&config, &entry, &mut host)
        .expect("h1b.gelu() must not be rewritten into a namespace call");
    assert_eq!(
        host.stdout_lines,
        vec!["42".to_owned(), "7".to_owned()],
        "h1b.gelu() must run as the local genus method (42), nn.gelu() as the namespace call (7)"
    );
}

const VERTE_CONST_LIB: &str =
    "+++\nlocale = \"en\"\n+++\nconst f32 GELU_ALPHA ← (0.7978845608028654 ∷ f32)\n";
const VERTE_CONST_MAIN: &str =
    "import from \"verteclib:lib\" vals\n\nmain {\n    print vals.GELU_ALPHA\n}\n";

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
    let reader = (lib_name == "callbacklib")
        .then_some("\n[reader]\nlocale = \"la\"\n")
        .unwrap_or_default();
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
{reader}
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
fn package_mir_passes_captured_entry_closure_to_linked_library_function() {
    let dir = crate::package::test_support::test_temp_dir("linked-library-callback");
    let entry = write_library_package(dir.as_ref(), "callbacklib", CALLBACK_LIB, CALLBACK_MAIN);
    let config = Config::default().with_stdlib(dir.join("libhome"));

    let mut host = BufferHost::default();
    run_package_mir(&config, &entry, &mut host)
        .expect("an entry closure with a nominal result must cross the linked-library call boundary and remain callable");
    assert_eq!(host.stdout_lines, vec!["42".to_owned()]);
}

// CG2-R08: a failable method on a linked nominal (`ok.reciproca()` where the
// library method is `→ numerus ⇥ textus`) must keep its try-call edge through
// the LibraryMethodCallRewriter synthetic-path rewrite. Pre-fix the synthetic
// callee carried `ty: None` (outside the namespace-call recovery map), so the
// MIR lowerer lowered it as a plain call and the cape handler's blocks
// orphaned ("block bbN is not reachable from entry", PKG001). The success
// path (`4`) and the error path (`caught`) both prove handler reachability.
const FAILABLE_METHOD_LIB: &str = "genus Divisor {\n    numerus x\n\n    functio reciproca() → numerus ⇥ textus {\n        si ego.x ≡ 0 {\n            iace \"division by zero\"\n        }\n        redde 100 / ego.x\n    }\n}\n\nfunctio structa(numerus x) → Divisor {\n    redde Divisor { x = x }\n}\n";
const FAILABLE_METHOD_MAIN: &str = "import from \"failib:lib\" lib\n\nmain {\n    const lib.Divisor ok ← lib.structa(25)\n    do {\n        const int r ← ok.reciproca()\n        print r\n    }\n    catch err {\n        print \"caught\"\n    }\n    const lib.Divisor bad ← lib.structa(0)\n    do {\n        const int r2 ← bad.reciproca()\n        print r2\n    }\n    catch err {\n        print \"caught\"\n    }\n}\n";

#[test]
fn package_mir_library_nominal_failable_method_preserves_handler_reachability() {
    // R08 oracle: a linked failable method preserves try-call/handler
    // reachability. The synthetic-path rewrite must recover the method's
    // function type (with its error half) so the MIR lowerer emits the
    // try-call; the success path prints the quotient and the throwing path
    // reaches the cape handler.
    let dir = crate::package::test_support::test_temp_dir("cg2-r08-failable-method");
    let entry = write_library_package(
        dir.as_ref(),
        "failib",
        FAILABLE_METHOD_LIB,
        FAILABLE_METHOD_MAIN,
    );
    let config = Config::default().with_stdlib(dir.join("libhome"));

    let mut host = BufferHost::default();
    run_package_mir(&config, &entry, &mut host)
        .expect("a failable linked nominal method must keep its try-call edge; the cape handler must be reachable");
    assert_eq!(
        host.stdout_lines,
        vec!["4".to_owned(), "caught".to_owned()],
        "the success path prints 100/25=4 and the throwing call reaches the cape handler"
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
    let value: f64 = host.stdout_lines[0]
        .parse()
        .expect("the const value prints");
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
    let workspace = radix::paths::faberlang_home().expect("faberlang container root");
    let exemplum = workspace.join("gradus/exempla/training-loop-mlp");
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

// The gradus tensor.fab shadowed-alias shape (GPU last link, hand-1 21e12fb0):
// a function param named `forma: lista<numerus>` shadows the import binding
// `forma` (`importa ex "gradus:shape" forma`). The semantic namespace
// reference `forma.quantitas(forma)` / `forma.causa(err)` then carries the
// param's synthetic HIR def instead of the import item's def, and both
// methods must still resolve through the receiver's OWN import binding —
// never a name sweep across the unit's imports (which bails when more than
// one import exports the method: BOTH `forma` and `typo` export `causa` here,
// mirroring gradus shape.fab + dtype.fab).
const FORMA_LIB: &str = "functio quantitas(lista<numerus> forma) → numerus {\n    redde forma.longitudo()\n}\n\nfunctio causa(numerus n) → numerus {\n    redde n + 1\n}\n";
const TYPO_LIB: &str = "functio causa(numerus n) → numerus {\n    redde n + 2\n}\n";
const FORMA_MAIN: &str = "import from \"forma:lib\" forma\nimport from \"typo:lib\" typo\n\n# The local param `forma` shadows the import binding `forma` — the shadowed\n# namespace-reference shape of gradus tensor.fab `_quantitas_forma`.\nfn dubbia(list<int> forma) → int {\n    const int n ← forma.quantitas(forma)\n    return forma.causa(n)\n}\n\nmain {\n    print dubbia([2, 3, 4])\n    print typo.causa(7)\n}\n";

/// The shadowed-alias rewrite fires for synthetic HIR receiver defs: a local
/// param named like an import binding resolves its namespace calls through
/// that import (gradus tensor.fab `_quantitas_forma` regression — the two
/// `forma.quantitas(forma)` / `forma.causa(err)` calls used to fail with
/// "method call before runtime/provider MIR lowering"). The receiver's own
/// import is used, so the `causa` name collision across TWO imports (`forma`
/// + `typo` both export `causa`) is unambiguous.
#[test]
fn package_mir_shadowed_alias_rewrites_shadowing_local_receiver_namespace_calls() {
    let dir = crate::package::test_support::test_temp_dir("shadowed-alias-rewrite");
    let src = dir.join("src");
    let forma_lib = dir.join("libhome").join("forma");
    let typo_lib = dir.join("libhome").join("typo");
    fs::create_dir_all(&src).expect("create package src");
    fs::create_dir_all(forma_lib.join("src")).expect("create forma lib src");
    fs::create_dir_all(typo_lib.join("src")).expect("create typo lib src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "fhir-lane-shadowed"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
target = "fmir"
kind = "bin"

[dependencies]
forma = "1.0.0"
typo = "1.0.0"
"#,
    )
    .expect("write faber.toml");
    let lock = format!(
        "[[package]]\nname = \"forma\"\nversion = \"1.0.0\"\nsource = \"path:{forma_display}\"\npackage_root = \"{forma_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{forma_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n[[package]]\nname = \"typo\"\nversion = \"1.0.0\"\nsource = \"path:{typo_display}\"\npackage_root = \"{typo_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{typo_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n",
        forma_display = forma_lib.display(),
        typo_display = typo_lib.display(),
    );
    fs::write(dir.join("faber.lock"), lock).expect("write faber.lock");
    fs::write(forma_lib.join("src/lib.fab"), FORMA_LIB).expect("write forma library module");
    fs::write(typo_lib.join("src/lib.fab"), TYPO_LIB).expect("write typo library module");
    let entry = src.join("main.fab");
    fs::write(&entry, FORMA_MAIN).expect("write main.fab");

    let config = Config::default().with_stdlib(dir.join("libhome"));
    let mut host = BufferHost::default();
    run_package_mir(&config, &entry, &mut host).expect(
        "forma.quantitas(forma) and forma.causa(n) must lower through the shadowed import binding",
    );
    assert_eq!(
        host.stdout_lines,
        vec!["4".to_owned(), "9".to_owned()],
        "the shadowed namespace calls must run through the receiver's import (3+1=4), and the ambiguous causa name must stay unambiguous via the direct typo.causa call (7+2=9)"
    );
}

/// The `forma`-shadowing shape: a receiver def in the HIR-generated range
/// (>= 1_000_000) whose binding name (recovered from the HIR def→name map)
/// appears in the unit's imports resolves to the registered synthetic
/// namespace target — the rewrite fires.
#[test]
fn shadowed_alias_shadowing_receiver_rewrites_when_local_name_is_in_imports() {
    let mut interner = Interner::default();
    let resolver = Resolver::new();
    let unit_path = PathBuf::from("<shadowed-alias-test>.fab");

    // The shadowing receiver: a synthetic HIR binding def (>= 1_000_000)
    // named `forma` — the exact name of an import binding. Synthetic HIR
    // bindings are never registered in the resolver symbol table; their names
    // come from the HIR def→name map.
    let receiver_def = DefId(1_000_000);
    let forma = interner.intern("forma");
    let mut local_names = HashMap::new();
    local_names.insert(receiver_def, forma);

    // The import binding `forma` → the imported module's item def, plus a
    // registered namespace-call target for `forma.quantitas(...)`.
    let import_def = DefId(42);
    let mut imports = HashMap::new();
    imports.insert("forma".to_owned(), import_def);
    let synthetic = DefId(2_000_000_000);
    let mut targets: NamespaceCallTargets = HashMap::new();
    targets.insert(
        (unit_path.clone(), import_def, "quantitas".to_owned()),
        synthetic,
    );

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
        local_names: &local_names,
        types: &TypeTable::new(),
        genus_methods: &HashSet::new(),
        diagnostics: &mut diagnostics,
    };
    assert_eq!(
        rewriter.import_target(&receiver, method, 1),
        Some(synthetic),
        "the forma-shadowing receiver (synthetic def >= 1_000_000 whose HIR name appears in the imports) must resolve to the synthetic namespace target"
    );
}

/// The `h1b.gelu()` false-positive exclusion: a synthetic local def whose
/// name does NOT appear in the unit's imports is NOT rewritten — even when
/// the method name matches an imported module's export (`nn` exports `gelu`).
/// Without the guard this call was rewritten into a synthetic-path namespace
/// call and corrupted AIR-lane library functions at lowering.
#[test]
fn shadowed_alias_plain_local_method_call_is_not_rewritten() {
    let mut interner = Interner::default();
    let resolver = Resolver::new();
    let unit_path = PathBuf::from("<plain-local-test>.fab");

    let receiver_def = DefId(1_000_001);
    let h1b = interner.intern("h1b");
    let mut local_names = HashMap::new();
    local_names.insert(receiver_def, h1b);

    // The unit imports a module (`nn`) that DOES export `gelu` and the target
    // is registered — the guard must still skip the h1b receiver because the
    // local's name does not shadow an import binding.
    let import_def = DefId(7);
    let mut imports = HashMap::new();
    imports.insert("nn".to_owned(), import_def);
    let synthetic = DefId(2_000_000_000);
    let mut targets: NamespaceCallTargets = HashMap::new();
    targets.insert(
        (unit_path.clone(), import_def, "gelu".to_owned()),
        synthetic,
    );

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
        local_names: &local_names,
        types: &TypeTable::new(),
        genus_methods: &HashSet::new(),
        diagnostics: &mut diagnostics,
    };
    assert_eq!(
        rewriter.import_target(&receiver, method, 0),
        None,
        "h1b.gelu() must not be rewritten: the local name does not shadow an import binding"
    );
}
