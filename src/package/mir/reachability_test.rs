//! S1 U3 tests: reachability pruning of linked library functions (codex-gap
//! campaign).
//!
//! Contract (delivery §3.3 + §6 U3, default = pruning): linked library
//! functions are lowered into the merged package-MIR program only when they
//! are reachable from the entry's call graph. Called exports — and the
//! private functions reachable from them — still lower and execute;
//! unreferenced exports are absent from the merged program's function-source
//! set. Cross-library calls (a library function calling another library's
//! export) keep their callees via the global fixpoint.

use super::*;
use radix::mir::BufferHost;

const REACH_UTIL: &str = "functio usata() → textus {\n    redde adjutrix()\n}\n\nfunctio adjutrix() → textus {\n    redde \"usata\"\n}\n\nfunctio non_usata() → textus {\n    redde \"non_usata\"\n}\n";
const REACH_MAIN: &str = "importa ex \"reachlib:vals\" privata vals\n\nfunctio run() → textus {\n    redde vals.usata()\n}\n\nincipit {\n    nota run()\n}\n";

/// Package whose entry imports the `reachlib:vals` library fixture under a
/// config library home (`libhome/reachlib/src`), so both the analysis phase
/// (lock resolver) and the package-MIR link phase (config home resolver)
/// resolve `reachlib:vals`.
fn write_reach_library_package(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("src");
    let libhome = dir.join("libhome");
    let library = libhome.join("reachlib");
    fs::create_dir_all(&src).expect("create package src");
    fs::create_dir_all(library.join("src")).expect("create library src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "fhir-reach-library"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"

[dependencies]
reachlib = "1.0.0"
"#,
    )
    .expect("write faber.toml");
    let library_display = library.display();
    let lock = format!(
        "[[package]]\nname = \"reachlib\"\nversion = \"1.0.0\"\nsource = \"path:{library_display}\"\npackage_root = \"{library_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{library_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n"
    );
    fs::write(dir.join("faber.lock"), lock).expect("write faber.lock");
    fs::write(library.join("src/vals.fab"), REACH_UTIL).expect("write library module");
    fs::write(src.join("main.fab"), REACH_MAIN).expect("write main.fab");
    src.join("main.fab")
}

/// The merged program's linked-library functions: MIR functions whose source
/// is a package-MIR synthetic def id (the linker allocates those only for
/// library and imported-sibling functions; this fixture has no sibling
/// imports, so synthetic sources are exactly the linked library's).
fn library_function_names(lowered: &LoweredMirUnit<'_>) -> Vec<String> {
    lowered
        .program
        .functions
        .iter()
        .filter(|function| {
            function
                .source
                .is_some_and(|source| source.0 >= PACKAGE_MIR_SYNTHETIC_DEF_BASE)
        })
        .filter_map(|function| function.name)
        .map(|name| lowered.interner.resolve(name).to_owned())
        .collect()
}

#[test]
fn package_mir_unused_library_function_not_lowered() {
    // S1 U3 done_when (1): the library exports `usata` (called by the entry)
    // and `non_usata` (never referenced). The merged program must lower only
    // the used export and its private callee `adjutrix`; the unused export's
    // function-source must be absent.
    let dir = crate::package::test_support::test_temp_dir("s1u3-reach");
    let entry = write_reach_library_package(dir.as_ref());
    let config = Config::default().with_stdlib(dir.join("libhome"));

    with_interpreted_lowered_package_mir(&config, &entry, |lowered| {
        let names = library_function_names(lowered);
        assert_eq!(
            names.len(),
            2,
            "only the used export and its private callee must lower, got {names:?}"
        );
        assert!(
            names.contains(&"usata".to_owned()),
            "the used export must lower, got {names:?}"
        );
        assert!(
            names.contains(&"adjutrix".to_owned()),
            "the private callee of the used export must lower (intra-module reachability), got {names:?}"
        );
        assert!(
            !names.contains(&"non_usata".to_owned()),
            "the unused export must not be lowered, got {names:?}"
        );
    })
    .expect("merged package MIR program");

    // S1 U3 done_when (3): no behavioral change for reachable functions —
    // the used export runs and matches the oracle.
    let mut host = BufferHost::default();
    run_package_mir(&config, &entry, &mut host).expect("package MIR run");
    assert_eq!(
        host.stdout_lines,
        vec!["usata".to_owned()],
        "the reachable export must print its oracle"
    );
}

// Cross-library fixture: `outer.fab` imports `reachlib:inner` and calls its
// `dupla`; both modules also export an unused function. The fixpoint must
// keep `computa` (entry → outer) AND `dupla` (outer → inner) while dropping
// `otiosa` and `inanis`.
const REACH_INNER: &str = "functio dupla(numerus value) → numerus {\n    redde value * 2\n}\n\nfunctio inanis(numerus value) → numerus {\n    redde value\n}\n";
const REACH_OUTER: &str = "importa ex \"reachlib:inner\" privata inner\n\nfunctio computa(numerus value) → numerus {\n    redde inner.dupla(value) + 1\n}\n\nfunctio otiosa(numerus value) → numerus {\n    redde value\n}\n";
const REACH_NESTED_MAIN: &str = "importa ex \"reachlib:outer\" privata outer\n\nincipit {\n    nota outer.computa(4)\n}\n";

fn write_nested_reach_library_package(dir: &std::path::Path) -> std::path::PathBuf {
    let src = dir.join("src");
    let libhome = dir.join("libhome");
    let library = libhome.join("reachlib");
    fs::create_dir_all(&src).expect("create package src");
    fs::create_dir_all(library.join("src")).expect("create library src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "fhir-reach-nested"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"

[dependencies]
reachlib = "1.0.0"
"#,
    )
    .expect("write faber.toml");
    let library_display = library.display();
    let lock = format!(
        "[[package]]\nname = \"reachlib\"\nversion = \"1.0.0\"\nsource = \"path:{library_display}\"\npackage_root = \"{library_display}\"\nkind = \"source\"\ntarget_language = \"\"\ntarget_triple = \"\"\ntarget_manifest = \"\"\ninterface_root = \"{library_display}/src\"\nartifact = \"\"\ncrate = \"\"\nrustc = \"\"\n"
    );
    fs::write(dir.join("faber.lock"), lock).expect("write faber.lock");
    fs::write(library.join("src/inner.fab"), REACH_INNER).expect("write inner module");
    fs::write(library.join("src/outer.fab"), REACH_OUTER).expect("write outer module");
    fs::write(src.join("main.fab"), REACH_NESTED_MAIN).expect("write main.fab");
    src.join("main.fab")
}

#[test]
fn package_mir_cross_library_call_keeps_callee_and_prunes_unused() {
    // S1 U3 fixpoint proof: the entry calls `outer.computa`, whose body calls
    // `inner.dupla` through a `MirCallee::Definition` edge. Both libraries'
    // unused exports (`otiosa`, `inanis`) must be pruned, and the nested
    // callee must survive the closure.
    let dir = crate::package::test_support::test_temp_dir("s1u3-nested-reach");
    let entry = write_nested_reach_library_package(dir.as_ref());
    let config = Config::default().with_stdlib(dir.join("libhome"));

    with_interpreted_lowered_package_mir(&config, &entry, |lowered| {
        let names = library_function_names(lowered);
        assert_eq!(
            names.len(),
            2,
            "computa + dupla must survive the cross-library closure, got {names:?}"
        );
        assert!(
            names.contains(&"computa".to_owned()),
            "the entry-reachable export must lower, got {names:?}"
        );
        assert!(
            names.contains(&"dupla".to_owned()),
            "the cross-library callee must lower, got {names:?}"
        );
        assert!(
            !names.contains(&"otiosa".to_owned()),
            "the outer unused export must be pruned, got {names:?}"
        );
        assert!(
            !names.contains(&"inanis".to_owned()),
            "the inner unused export must be pruned, got {names:?}"
        );
    })
    .expect("merged package MIR program");

    let mut host = BufferHost::default();
    run_package_mir(&config, &entry, &mut host).expect("package MIR run");
    assert_eq!(
        host.stdout_lines,
        vec!["9".to_owned()],
        "computa(4) = dupla(4) + 1 must match the oracle"
    );
}
