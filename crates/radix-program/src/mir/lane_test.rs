//! FMIR e2e-hardening (CTO-1) lane tests: the two library-import blockers and
//! the PML4 acceptance fixture.
//!
//! (i) Method calls on linked library nominals (`p.dupla()`, `x.typus()`) now
//! link through synthetic-path calls instead of failing with "method call
//! before runtime/provider MIR lowering".
//! (ii) Const data members with static-ascription initializers
//! (`sit f32 GELU_ALPHA ← (0.79… ∷ f32)`) transplant through the entry's
//! top-level-const seam instead of failing with "package MIR cannot transplant
//! const data member initializer with unsupported expression shape".
//! (iii) Const-to-const initializers (`ELEMENTS ← N * K`) transplant by
//! rewriting the inner `Path` to the referenced linked const's synthetic def
//! id; a `Path` with no linked const target still fails closed (MC6,
//! 7df48a51).

use super::*;
use radix_mir::{MirConversion, MirConversionFlavor};
use radix_mir_runner::BufferHost;
use radix_module::lexer::Span;
use radix_module::semantic::Resolver;
use std::path::PathBuf;

const METHOD_LIB: &str = "+++\nlocale = \"la\"\n+++\n@ publica\ngenus Punctum {\n    varia numerus x\n\n    functio dupla() → numerus {\n        redde ego.x * 2\n    }\n}\n\n@ publica\nfunctio structa(numerus x) → Punctum {\n    redde Punctum { x = x }\n}\n";
const METHOD_MAIN: &str = "import from \"methlib:lib\" lib\n\nmain {\n    let lib.Punctum p ← lib.structa(21)\n    print p.dupla()\n}\n";
const LOCAL_METHOD_LIB: &str = "@ public {}\nclass Punctum {\n    mut int x\n\n    fn dupla() → int {\n        return self.x * 2\n    }\n}\n\n@ public {}\nfn structa(int x) → Punctum {\n    return Punctum { x = x }\n}\n";
const LOCAL_METHOD_MAIN: &str = "import from \"./math\" math\n\nmain {\n    let math.Punctum p ← math.structa(21)\n    print p.dupla()\n}\n";

const CALLBACK_LIB: &str = "@ publica\ngenus Lectio {\n    varia bivalens successus\n    varia numerus valor\n    varia textus causa\n}\n\n@ publica\nfunctio applica((numerus, numerus) → Lectio f, numerus x) → numerus {\n    sit Lectio lectio ← f(x, 2)\n    si non lectio.successus ergo redde 0\n    redde lectio.valor\n}\n";
const CALLBACK_MAIN: &str = "importa ex \"callbacklib:lib\" lib\n\nincipit {\n    sit numerus factor ← 1\n    nota lib.applica((numerus x, numerus y) → lib.Lectio ∴ lib.Lectio { successus = verum, valor = x * y * factor, causa = \"\" }, 21)\n}\n";

// The forma-shadowing guard (fd0c939) false-positive exclusion: a local
// `h1b` whose method call `h1b.gelu()` must NOT be rewritten into a
// synthetic-path namespace call to the imported `nn.gelu()` just because the
// unit imports a module that exports `gelu`. Pre-guard, the method-name sweep
// rewrote ANY HIR-generated receiver def (>= 1_000_000), so `h1b.gelu()` was
// rewritten to `nn.gelu(h1b)` — an arity-corrupting synthetic-path call.
const GELU_LIB: &str =
    "+++\nlocale = \"la\"\n+++\n@ publica\nfunctio gelu() → numerus {\n    redde 7\n}\n";
const GELU_MAIN: &str = "import from \"gelulib:lib\" nn\n\nclass H1B {\n    mut int x\n\n    fn gelu() → int {\n        return self.x * 2\n    }\n}\n\nfn structa(int x) → H1B {\n    return H1B { x = x }\n}\n\nmain {\n    let int nn_value ← nn.gelu()\n    let H1B h1b ← structa(21)\n    print h1b.gelu()\n    print nn_value\n}\n";

#[test]
fn package_mir_rewrites_runtime_conversion_types_with_imported_types() {
    let mut statement = MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: None,
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Convert(MirConversion {
                    store: None,
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

#[test]
fn package_mir_rewrites_imported_type_check_targets() {
    for positive in [true, false] {
        let mut statement = MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: None,
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::TypeCheck(radix_mir::MirTypeCheckOp {
                        positive,
                        ty: MirType::semantic(TypeId(2)),
                    }),
                    args: vec![MirOperand::Constant(MirConstant::Int(1))],
                    return_ty: MirType::semantic(TypeId(3)),
                },
            },
            span: Span::default(),
        };
        super::lower::rewrite_statement_type_id(
            &mut statement,
            &[(TypeId(2), TypeId(11)), (TypeId(3), TypeId(12))],
        );
        let MirStatementKind::RuntimeCall { call, .. } = statement.kind else {
            panic!("expected a runtime call");
        };
        let MirIntrinsic::TypeCheck(check) = call.intrinsic else {
            panic!("expected a type-check intrinsic");
        };
        assert_eq!(check.ty.semantic_id(), TypeId(11));
        assert_eq!(check.positive, positive);
        assert_eq!(call.return_ty.semantic_id(), TypeId(12));
    }
}

#[test]
fn package_mir_imported_type_checks_match_list_and_map_carriers() {
    let dir = crate::test_support::test_temp_dir("imported-type-check");
    let library = r#"+++
locale = "en"
+++
@ public
fn accepts(value raw) → bool {
    if raw not is list<value> then return false
    let list<value> items ← raw ↦ list<value> ⊥ empty
    for from items let item {
        if item is map<string, value> then return true
    }
    return false
}
"#;
    let main = r#"import from "checklib:lib" lib
class Pad { mut list<string> names = [] }
main {
    mut map<string, value> row ← {}
    row.put("name", "Accept" ↦ value)
    let list<value> rows ← [row ↦ value]
    assert lib.accepts(rows ↦ value)
    assert not lib.accepts(row ↦ value)
    print "accepted"
}
"#;
    let entry = write_library_package(dir.as_ref(), "checklib", library, main);
    let config = Config::default().with_library_home(dir.join("libhome"));
    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("imported type-check targets must use the merged type table");
    assert_eq!(host.stdout_lines, ["accepted"]);
}

/// The `forma`-shadowing guard (fd0c939) excludes the `h1b.gelu()` false
/// positive: an ordinary method call on a local is NOT rewritten into a
/// synthetic-path namespace call when the method name happens to match an
/// imported module's export. The guard's three conditions (def.0 >= `1_000_000`
/// AND `SymbolKind::Local` AND local-name in the imports) make `h1b` ineligible
/// (its name does not shadow an import binding), so the call runs as the local
/// genus method. Without the guard, the method-name sweep rewrites `h1b.gelu()`
/// → `nn.gelu(h1b)` and the package MIR fails — this test would fail.
#[test]
fn package_mir_shadowed_alias_rewrite_skips_plain_local_method_calls() {
    let dir = crate::test_support::test_temp_dir("shadowed-alias-skip");
    let entry = write_library_package(dir.as_ref(), "gelulib", GELU_LIB, GELU_MAIN);
    let config = Config::default().with_library_home(dir.join("libhome"));

    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("h1b.gelu() must not be rewritten into a namespace call");
    assert_eq!(
        host.stdout_lines,
        vec!["42".to_owned(), "7".to_owned()],
        "h1b.gelu() must run as the local genus method (42), nn.gelu() as the namespace call (7)"
    );
}

const VERTE_CONST_LIB: &str =
    "+++\nlocale = \"en\"\n+++\n@ public\nconst f32 GELU_ALPHA = (0.7978845608028654 ∷ f32)\n";
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
    let reader = if lib_name == "callbacklib" {
        "\n[reader]\nlocale = \"la\"\n"
    } else {
        Default::default()
    };
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

fn write_local_method_package(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create local-method package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "fhir-lane-local-method"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
target = "fmir"
kind = "bin"

[reader]
locale = "en"
"#,
    )
    .expect("write local-method faber.toml");
    fs::write(src.join("math.fab"), LOCAL_METHOD_LIB).expect("write local math module");
    fs::write(src.join("main.fab"), LOCAL_METHOD_MAIN).expect("write local main module");
    src.join("main.fab")
}

#[test]
fn package_mir_local_nominal_method_call_runs() {
    let dir = crate::test_support::test_temp_dir("local-method");
    let entry = write_local_method_package(dir.as_ref());
    let config = Config::default().with_library_home(dir.join("libhome"));

    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("a sibling nominal method call must link through the package MIR merge");
    assert_eq!(host.stdout_lines, vec!["42".to_owned()]);
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
    let dir = crate::test_support::test_temp_dir("cto1-method");
    let entry = write_library_package(dir.as_ref(), "methlib", METHOD_LIB, METHOD_MAIN);
    let config = Config::default().with_library_home(dir.join("libhome"));

    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("library method call must run");
    assert_eq!(
        host.stdout_lines,
        vec!["42".to_owned()],
        "the library method must return its oracle"
    );
}

/// A package that tests itself through `provider:module` specifiers must keep
/// those members as sibling units. Distinctive Faber arithmetic (`n + 1` in
/// the constructor, `n + 100` in the method, `null` when n == 0) proves the
/// linked body ran; a runner shim cannot invent those results.
#[test]
fn package_test_self_provider_methods_execute_linked_bodies() {
    let dir = crate::test_support::test_temp_dir("self-provider-test");
    let package_root = dir.join("libhome").join("geomlib");
    let src = package_root.join("src");
    fs::create_dir_all(&src).expect("create self-provider src");
    fs::write(
        package_root.join("faber.toml"),
        r#"
[package]
name = "geomlib"
version = "1.0.0"
edition = "2026"

[library]
provider = "geomlib"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["fmir"]

[reader]
locale = "en"
"#,
    )
    .expect("write self-provider faber.toml");
    fs::write(
        src.join("codes.fab"),
        r"
@ public
fn encoded(int n) → int {
    return n + 50
}
",
    )
    .expect("write codes.fab");
    fs::write(
        src.join("data.fab"),
        r#"
import from "geomlib:codes" codes

@ public
class Mesh {
    mut int n

    fn layout_count() → int ∪ none {
        if self.n ≡ 0 then return null
        return codes.encoded(self.n) + 50
    }
}

@ public
fn mesh(int n) → Mesh {
    return Mesh { n = n + 1 }
}
"#,
    )
    .expect("write data.fab");
    fs::write(
        src.join("data.proba"),
        r#"+++
locale = "en"
+++
import from "geomlib:data" data

fn prove_linked_bodies() {
    let data.Mesh ready ← data.mesh(2)
    let data.Mesh blank ← data.mesh(0 - 1)
    print ready.layout_count() coalesce 0
    print blank.layout_count() coalesce 0
}
"#,
    )
    .expect("write data.proba");

    let config = Config::default().with_library_home(dir.join("libhome"));
    let package = crate::analyze::analyze_package_for_tests(
        &config,
        &package_root,
        None,
        &crate::test_support::test_analysis_seams(),
    )
    .expect("analyze self-provider package tests");
    let mut functions = Vec::new();
    for unit in package.nodes.values() {
        let is_proba = unit
            .path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext == "proba");
        if !is_proba {
            continue;
        }
        for item in &unit.analysis.hir.items {
            if let HirItemKind::Function(function) = &item.kind
                && unit.analysis.interner.resolve(function.name) == "prove_linked_bodies"
            {
                functions.push((unit.path.clone(), item.def_id));
            }
        }
    }
    assert_eq!(functions.len(), 1, "expected one prove_linked_bodies case");
    let scope = TestPackageScope::for_program(&package, functions);
    let output = with_linked_test_package_mir(
        package,
        &scope,
        &config,
        &crate::test_support::test_mir_seams(),
        |validated, function_ids| {
            let interner = validated
                .validation()
                .interner
                .as_interner()
                .expect("linked test MIR has an interner");
            let leftover = validated
                .program()
                .functions
                .iter()
                .flat_map(|function| {
                    function.blocks.iter().flat_map(|block| {
                        block.statements.iter().filter_map(|statement| {
                            let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                                return None;
                            };
                            let MirIntrinsic::Provider(provider) = &call.intrinsic else {
                                return None;
                            };
                            if provider.kind != MirProviderKind::Package {
                                return None;
                            }
                            Some(
                                provider
                                    .module
                                    .iter()
                                    .map(|symbol| interner.resolve(*symbol).to_owned())
                                    .chain(std::iter::once(
                                        interner.resolve(provider.name).to_owned(),
                                    ))
                                    .collect::<Vec<_>>()
                                    .join("::"),
                            )
                        })
                    })
                })
                .collect::<Vec<_>>();
            assert!(
                leftover.is_empty(),
                "self-provider members must not remain package providers: {leftover:?}"
            );
            let mut host = BufferHost::default();
            for function_id in function_ids.values() {
                radix_mir_runner::run_function(validated, *function_id, &mut host)
                    .expect("linked Faber bodies must execute");
            }
            host.stdout_lines
        },
    )
    .unwrap_or_else(|diagnostics| {
        panic!(
            "self-provider package test must link: {:?}",
            diagnostics
                .iter()
                .map(|diag| (diag.code, diag.issue()))
                .collect::<Vec<_>>()
        )
    });
    assert_eq!(
        output,
        vec!["103".to_owned(), "0".to_owned()],
        "mesh(2) must run n+1 then n+100; mesh(-1) must return null from the Faber body"
    );
}

#[test]
fn package_mir_passes_captured_entry_closure_to_linked_library_function() {
    let dir = crate::test_support::test_temp_dir("linked-library-callback");
    let entry = write_library_package(dir.as_ref(), "callbacklib", CALLBACK_LIB, CALLBACK_MAIN);
    let config = Config::default().with_library_home(dir.join("libhome"));

    let mut host = BufferHost::default();
    run_package_mir(&config, &entry, &crate::test_support::test_mir_seams(), &mut host)
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
const FAILABLE_METHOD_LIB: &str = "+++\nlocale = \"la\"\n+++\n@ publica\ngenus Divisor {\n    varia numerus x\n\n    functio reciproca() → numerus ⇥ textus {\n        si ego.x ≡ 0 {\n            iace \"division by zero\"\n        }\n        redde 100 / ego.x\n    }\n}\n\n@ publica\nfunctio structa(numerus x) → Divisor {\n    redde Divisor { x = x }\n}\n";
const FAILABLE_METHOD_MAIN: &str = "import from \"failib:lib\" lib\n\nmain {\n    let lib.Divisor ok ← lib.structa(25)\n    do {\n        let int r ← ok.reciproca()\n        print r\n    }\n    catch err {\n        print \"caught\"\n    }\n    let lib.Divisor bad ← lib.structa(0)\n    do {\n        let int r2 ← bad.reciproca()\n        print r2\n    }\n    catch err {\n        print \"caught\"\n    }\n}\n";

#[test]
fn package_mir_library_nominal_failable_method_preserves_handler_reachability() {
    // R08 oracle: a linked failable method preserves try-call/handler
    // reachability. The synthetic-path rewrite must recover the method's
    // function type (with its error half) so the MIR lowerer emits the
    // try-call; the success path prints the quotient and the throwing path
    // reaches the cape handler.
    let dir = crate::test_support::test_temp_dir("cg2-r08-failable-method");
    let entry = write_library_package(
        dir.as_ref(),
        "failib",
        FAILABLE_METHOD_LIB,
        FAILABLE_METHOD_MAIN,
    );
    let config = Config::default().with_library_home(dir.join("libhome"));

    let mut host = BufferHost::default();
    run_package_mir(&config, &entry, &crate::test_support::test_mir_seams(), &mut host)
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
    let dir = crate::test_support::test_temp_dir("cto1-verte-const");
    let entry = write_library_package(dir.as_ref(), "verteclib", VERTE_CONST_LIB, VERTE_CONST_MAIN);
    let config = Config::default().with_library_home(dir.join("libhome"));

    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("∷-cast const member must run");
    let value: f64 = host.stdout_lines[0]
        .parse()
        .expect("the const value prints");
    assert!(
        (value - 0.797_884_560_802_865_4).abs() < 1e-4,
        "the ascription-cast const must keep its value, got {value}"
    );
}

// MC6 (7df48a51): const-to-const initializers (`ELEMENTS ← N * K`, the
// gradus kernel.fab GEMV shape) transplant by rewriting the inner `Path`
// to the referenced const's synthetic def id. Under DECISIONS D5.1 the
// referenced N and K stay unmarked (module-private): every top-level const is
// linked for transplant, and only exports become member-reference targets.
const PATH_CONST_SIBLING_MATH: &str =
    "const int N = 320\n\nconst int K = 960\n\n@ internal\nconst int ELEMENTS = N * K\n";
const PATH_CONST_SIBLING_MAIN: &str =
    "import from \"./math\" math\n\nmain {\n    print math.ELEMENTS\n}\n";
const PATH_CONST_LIB: &str = "+++\nlocale = \"en\"\n+++\nconst int N = 320\n\nconst int K = 960\n\n@ public\nconst int ELEMENTS = N * K\n";
const PATH_CONST_LIB_MAIN: &str =
    "import from \"pathclib:lib\" vals\n\nmain {\n    print vals.ELEMENTS\n}\n";
// D5.1: a `Path` to an `@ private` const of the same source unit links — the
// const is transplanted without becoming a member-reference target.
const PATH_CONST_PRIVATA_MATH: &str =
    "@ private\nconst int PRIVATE_N = 320\n\n@ internal\nconst int ELEMENTS = PRIVATE_N * 3\n";
const PATH_CONST_PRIVATA_MAIN: &str =
    "import from \"./math\" math\n\nmain {\n    print math.ELEMENTS\n}\n";

/// Write a two-unit local package (`src/<sibling>.fab` + `src/main.fab`) with
/// the sibling imported as a namespace binding.
fn write_sibling_namespace_package(
    dir: &std::path::Path,
    sibling_name: &str,
    sibling_source: &str,
    main_source: &str,
) -> std::path::PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create sibling package src");
    fs::write(
        dir.join("faber.toml"),
        format!(
            r#"
[package]
name = "fhir-lane-{sibling_name}"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
target = "fmir"
kind = "bin"

[reader]
locale = "en"
"#
        ),
    )
    .expect("write faber.toml");
    fs::write(src.join(format!("{sibling_name}.fab")), sibling_source)
        .expect("write sibling module");
    fs::write(src.join("main.fab"), main_source).expect("write main module");
    src.join("main.fab")
}

#[test]
fn package_mir_const_data_member_path_initializer_transplants_sibling() {
    // MC6 admitted shape (sibling variant, the gradus kernel.fab scenario):
    // `let int ELEMENTS ← N * K` references sibling consts N and K. The
    // initializer's `Path` operands rewrite to the linked consts' synthetic
    // def ids, so the transplant runs and `math.ELEMENTS` evaluates 320*960.
    let dir = crate::test_support::test_temp_dir("mc6-path-sibling");
    let entry = write_sibling_namespace_package(
        dir.path(),
        "math",
        PATH_CONST_SIBLING_MATH,
        PATH_CONST_SIBLING_MAIN,
    );
    let config = Config::default().with_library_home(dir.join("libhome"));

    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("const-to-const sibling initializer must transplant");
    assert_eq!(
        host.stdout_lines,
        vec!["307200".to_owned()],
        "N * K must evaluate through the transplanted Path references"
    );
}

#[test]
fn package_mir_const_data_member_path_initializer_transplants_library() {
    // MC6 admitted shape (cross-package variant): the same const-to-const
    // initializer behind a library import rewrites through the linked
    // library's source-path entry of the const rewrite map.
    let dir = crate::test_support::test_temp_dir("mc6-path-library");
    let entry = write_library_package(
        dir.as_ref(),
        "pathclib",
        PATH_CONST_LIB,
        PATH_CONST_LIB_MAIN,
    );
    let config = Config::default().with_library_home(dir.join("libhome"));

    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("const-to-const library initializer must transplant");
    assert_eq!(
        host.stdout_lines,
        vec!["307200".to_owned()],
        "N * K must evaluate through the transplanted Path references"
    );
}

#[test]
fn package_mir_const_data_member_path_initializer_module_private_target_transplants() {
    // Pre-D5.1 this pinned a fail-closed boundary: `@ privata` consts were
    // never linked, so `ELEMENTS ← PRIVATE_N * 3` had no synthetic target.
    // D5.1 makes unmarked top-level declarations module-private, which would
    // turn every same-module const reference (`ELEMENTS ← N * K`) into that
    // failure; module-private consts are therefore linked for transplant and
    // only withheld as member-reference targets.
    let dir = crate::test_support::test_temp_dir("mc6-path-privata");
    let entry = write_sibling_namespace_package(
        dir.path(),
        "math",
        PATH_CONST_PRIVATA_MATH,
        PATH_CONST_PRIVATA_MAIN,
    );
    let config = Config::default().with_library_home(dir.join("libhome"));

    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("a module-private const referenced by an exported initializer must link");
    assert_eq!(
        host.stdout_lines,
        vec!["960".to_owned()],
        "PRIVATE_N * 3 must evaluate through the transplanted private const"
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
const FORMA_LIB: &str = "+++\nlocale = \"la\"\n+++\n@ publica\nfunctio quantitas(lista<numerus> forma) → numerus {\n    redde forma.longitudo()\n}\n\n@ publica\nfunctio causa(numerus n) → numerus {\n    redde n + 1\n}\n";
const TYPO_LIB: &str = "+++\nlocale = \"la\"\n+++\n@ publica\nfunctio causa(numerus n) → numerus {\n    redde n + 2\n}\n";
const FORMA_MAIN: &str = "import from \"forma:lib\" forma\nimport from \"typo:lib\" typo\n\n# The local param `forma` shadows the import binding `forma` — the shadowed\n# namespace-reference shape of gradus tensor.fab `_quantitas_forma`.\nfn dubbia(list<int> forma) → int {\n    let int n ← forma.quantitas(forma)\n    return forma.causa(n)\n}\n\nmain {\n    print dubbia([2, 3, 4])\n    print typo.causa(7)\n}\n";

/// The shadowed-alias rewrite fires for synthetic HIR receiver defs: a local
/// param named like an import binding resolves its namespace calls through
/// that import (gradus tensor.fab `_quantitas_forma` regression — the two
/// `forma.quantitas(forma)` / `forma.causa(err)` calls used to fail with
/// "method call before runtime/provider MIR lowering"). The receiver's own
/// import is used, so the `causa` name collision across TWO imports (`forma`
/// + `typo` both export `causa`) is unambiguous.
#[test]
fn package_mir_shadowed_alias_rewrites_shadowing_local_receiver_namespace_calls() {
    let dir = crate::test_support::test_temp_dir("shadowed-alias-rewrite");
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

    let config = Config::default().with_library_home(dir.join("libhome"));
    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect(
        "forma.quantitas(forma) and forma.causa(n) must lower through the shadowed import binding",
    );
    assert_eq!(
        host.stdout_lines,
        vec!["4".to_owned(), "9".to_owned()],
        "the shadowed namespace calls must run through the receiver's import (3+1=4), and the ambiguous causa name must stay unambiguous via the direct typo.causa call (7+2=9)"
    );
}

#[test]
fn package_mir_rewrites_gradient_call_and_selected_arguments() {
    let mut interner = Interner::default();
    let unit_path = PathBuf::from("<gradient-link-test>.fab");
    let forward_namespace = DefId(11);
    let selector_namespace = DefId(12);
    let data_namespace = DefId(13);
    let forward_method = interner.intern("loss");
    let selector_method = interner.intern("argument");
    let data_member = interner.intern("WEIGHT");
    let forward_target = DefId(2_000_000_001);
    let selector_target = DefId(2_000_000_002);
    let data_target = DefId(2_000_000_003);

    let mut targets = NamespaceCallTargets::new();
    targets.insert(
        (unit_path.clone(), forward_namespace, "loss".to_owned()),
        forward_target,
    );
    targets.insert(
        (unit_path.clone(), selector_namespace, "argument".to_owned()),
        selector_target,
    );
    let mut data_member_targets = NamespaceDataMemberTargets::new();
    data_member_targets.insert(
        (unit_path.clone(), data_namespace, "WEIGHT".to_owned()),
        data_target,
    );

    let namespace_call = |namespace, method| HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Call(
            Box::new(HirExpression {
                id: HirId(0),
                kind: HirExpressionKind::Field(
                    Box::new(HirExpression {
                        id: HirId(0),
                        kind: HirExpressionKind::Path(namespace),
                        ty: None,
                        span: Span::default(),
                    }),
                    method,
                ),
                ty: None,
                span: Span::default(),
            }),
            Vec::new(),
            Vec::new(),
            None,
        ),
        ty: None,
        span: Span::default(),
    };
    let mut gradient = HirExpression {
        id: HirId(0),
        kind: HirExpressionKind::Gradient {
            call: Box::new(namespace_call(forward_namespace, forward_method)),
            selected_arguments: vec![
                namespace_call(selector_namespace, selector_method),
                HirExpression {
                    id: HirId(0),
                    kind: HirExpressionKind::Field(
                        Box::new(HirExpression {
                            id: HirId(0),
                            kind: HirExpressionKind::Path(data_namespace),
                            ty: None,
                            span: Span::default(),
                        }),
                        data_member,
                    ),
                    ty: None,
                    span: Span::default(),
                },
            ],
        },
        ty: None,
        span: Span::default(),
    };

    let mut diagnostics = Vec::new();
    super::link::rewrite_expr(
        &unit_path,
        &mut gradient,
        &interner,
        &targets,
        &data_member_targets,
        &NamespaceExports::new(),
        &mut diagnostics,
    );

    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
    let HirExpressionKind::Gradient {
        call,
        selected_arguments,
    } = gradient.kind
    else {
        panic!("the linker must preserve the gradient node");
    };
    let HirExpressionKind::Call(callee, _, _, _) = call.kind else {
        panic!("the gradient's forward expression must remain a call");
    };
    assert_eq!(callee.kind, HirExpressionKind::Path(forward_target));
    let HirExpressionKind::Call(callee, _, _, _) = &selected_arguments[0].kind else {
        panic!("the first selected argument must remain a call");
    };
    assert_eq!(callee.kind, HirExpressionKind::Path(selector_target));
    assert_eq!(
        selected_arguments[1].kind,
        HirExpressionKind::Path(data_target),
        "const data members inside selected arguments must also be linked"
    );
}

/// The `forma`-shadowing shape: a receiver def in the HIR-generated range
/// (`>= 1_000_000`) whose binding name (recovered from the HIR def→name map)
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

#[test]
fn package_mir_return_channel_declared_conversion_applies() {
    let dir = crate::test_support::test_temp_dir("return-channel-conversion");
    let entry = write_return_conversion_package(dir.path(), RETURN_CONVERSION_MLP);
    let config = Config::default().with_library_home(dir.join("libhome"));

    let mut host = BufferHost::default();
    run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect("the declared return-channel conversion must apply before ReturnError");
    assert_eq!(host.stdout_lines, vec!["42".to_owned()]);
}

/// Fail-closed counterpart: without a declared conversion the return-channel
/// mismatch must still be rejected — the lowering only applies an arm the
/// source actually declares.
#[test]
fn package_mir_return_channel_mismatch_without_conversion_fails_closed() {
    let dir = crate::test_support::test_temp_dir("return-channel-no-conversion");
    let entry = write_return_conversion_package(dir.path(), RETURN_MISMATCH_MLP);
    let config = Config::default().with_library_home(dir.join("libhome"));

    let mut host = BufferHost::default();
    let diagnostics = run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect_err("a return-channel mismatch without a declared conversion must fail closed");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("failable_call_conversion_absent")),
        "expected the declared-conversion-absent diagnostic, got {:?}",
        diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code,
                    diagnostic.issue().map(str::to_owned),
                )
            })
            .collect::<Vec<_>>()
    );
}

/// PKG-ADM library-seam converse: the library interface analysis path admits
/// only publica-tier exports (the unit's own export surface), never a
/// blanket kernel-annotation exemption — a `@ private` size-generic
/// `@ kernel` template in an imported library with no same-unit caller
/// stays an ADM001 orphan in the consumer's analysis.
#[test]
fn package_mir_library_kernel_template_admission_stays_fail_closed_for_unexported_templates() {
    let library = r#"+++
locale = "en"
+++
@ kernel
@ private
fn hidden<size M, size N>(tensor<f32, [M, N]> a, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {
    return a + b
}

@ public
fn touch(i32 x) → i32 {
    return x
}
"#;
    let main = "import from \"orphanlib:lib\" orb\n\nmain {\n    print orb.touch(1)\n}\n";
    let dir = crate::test_support::test_temp_dir("library-kernel-admission");
    let entry = write_library_package(dir.path(), "orphanlib", library, main);
    let config = Config::default().with_library_home(dir.join("libhome"));

    let mut host = BufferHost::default();
    let diagnostics = run_package_mir(
        &config,
        &entry,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )
    .expect_err("an unexported library kernel template must stay an ADM001 orphan");
    let orphan = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.issue() == Some("nucleum_orphaned_annotation"))
        .expect("the ADM001 orphan diagnostic must name the library module");
    assert!(
        orphan.file.ends_with("orphanlib/src/lib.fab"),
        "the orphan must be reported on the library unit, got {}",
        orphan.file
    );
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("imported_library_parse_failure")),
        "the fail-closed orphan is an admission fact, not an import-site parse failure"
    );
}

/// Write the two-module library package behind the return-channel rows:
/// `nn.fab` provides a failable namespace verb; `mlp.fab` wraps it on the
/// return channel. `mlp_lib` selects whether the destination union declares
/// the `@ conversion` arm. The `return nn.linear_carrier(x)` shape is exactly
/// the one whose call-site conversion the package-MIR namespace-call rewrite
/// drops, so the MIR lowering must recover the declared arm at the
/// `ReturnError` terminator (gradus mlp.fab PML4 shape).
fn write_return_conversion_package(dir: &std::path::Path, mlp_lib: &str) -> std::path::PathBuf {
    let nn_lib = r#"+++
locale = "en"
+++
@ public
union NnError {
    @ shared
    string message
    NnBad
}

@ public
fn linear_carrier(int x) → int ⇥ NnError {
    return 42
}
"#;
    let src = dir.join("src");
    let library = dir.join("libhome").join("convlib");
    fs::create_dir_all(&src).expect("create package src");
    fs::create_dir_all(library.join("src")).expect("create library src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "fhir-lane-return-conv"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
target = "fmir"
kind = "bin"

[reader]
locale = "en"

[dependencies]
convlib = "1.0.0"
"#,
    )
    .expect("write faber.toml");
    let library_display = library.display();
    let lock = format!(
        "[[package]]\nname = \"convlib\"\nversion = \"1.0.0\"\nsource = \"path:{library_display}\"\npackage_root = \"{library_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{library_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n"
    );
    fs::write(dir.join("faber.lock"), lock).expect("write faber.lock");
    fs::write(library.join("src/nn.fab"), nn_lib).expect("write nn module");
    fs::write(library.join("src/mlp.fab"), mlp_lib).expect("write mlp module");
    let entry = src.join("main.fab");
    let main = "import from \"convlib:mlp\" mlp\n\nmain {\n    do {\n        print mlp.wrapper(1)\n    }\n    catch err {\n        print \"err\"\n    }\n}\n";
    fs::write(&entry, main).expect("write main.fab");
    entry
}

// Respelled onto the schema-7 named standalone `@ conversion fn` form: the
// old typed union-arm payload (`@ conversion nn.NnError err { … }` inside the
// union) is retracted grammar (PARSE018), so the declared arm is enrolled
// from the conversion fn's signature instead. The declaration still names
// the same cross-package pair (nn.NnError → GradusError) the return-channel
// lowering must recover at the `ReturnError` terminator.
const RETURN_CONVERSION_MLP: &str = r#"+++
locale = "en"
+++
import from "convlib:nn" nn

@ public
union GradusError {
    @ shared
    string message
    GradusBad
}

@ conversion
fn from_nn_error(nn.NnError err) → GradusError {
    match err {
        case nn.NnError.NnBad let message { return variant GradusBad {message = message} }
    }
}

@ public
fn wrapper(int x) → int ⇥ GradusError {
    return nn.linear_carrier(x)
}
"#;

const RETURN_MISMATCH_MLP: &str = r#"+++
locale = "en"
+++
import from "convlib:nn" nn

@ public
union GradusError {
    @ shared
    string message
    GradusBad
}

@ public
fn wrapper(int x) → int ⇥ GradusError {
    return nn.linear_carrier(x)
}
"#;
