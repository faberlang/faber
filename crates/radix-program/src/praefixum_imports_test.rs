//! ST2-2.6 / ST2-2.11: `PackageImportEffects` answers import-effect questions
//! over the analyzed nodes of temp packages, including library callees when
//! the caller supplies the analyzed library bodies. Assertions are on the
//! answer's shape (`Pure`, `Effect` effect and chain, `Unavailable`), never on
//! prose.

use super::*;
use crate::analyze::{AnalyzedProgram, analyze_package};
use crate::test_support::{test_mir_seams, test_temp_dir};
use radix_module::driver::Config;
use radix_module::semantic::passes::praefixum::imported_references;
use std::fs;
use std::path::{Path, PathBuf};

const EFFECT: &str = "scribe";

/// In-tree library home `<repo>/lib` (holds `norma/`).
fn library_home() -> PathBuf {
    let home = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lib");
    assert!(
        home.join("norma/src").is_dir(),
        "expected lib/norma/src under {}",
        home.display()
    );
    home
}

fn analyze(dir: &Path) -> AnalyzedProgram {
    let seams = test_mir_seams();
    let config = Config::default()
        .with_target((seams.executed_lane_target)(&Config::default(), dir).unwrap())
        .with_library_home(library_home());
    analyze_package(&config, dir, &seams.analysis).unwrap()
}

/// Analyze `dir` and also load the analyzed bodies of every expanded library
/// import into a resolver table (the deferral hook's library snapshot).
fn analyze_with_library_bodies(dir: &Path) -> (AnalyzedProgram, LibraryBodies) {
    let seams = test_mir_seams();
    let config = Config::default()
        .with_target((seams.executed_lane_target)(&Config::default(), dir).unwrap())
        .with_library_home(library_home());
    let (program, mut cache) =
        crate::analyze::analyze_package_retaining_library_cache(&config, dir, &seams.analysis)
            .unwrap();
    let mut bodies = LibraryBodies::new();
    for node in program.nodes.values() {
        for import in &node.expanded_library_imports {
            let Ok(analysis) = crate::library::library_cached_analysis(
                import,
                &program.library_resolver,
                &mut cache,
            ) else {
                continue;
            };
            bodies.insert(
                (
                    import.module.package.clone(),
                    import.module.module_path.clone(),
                ),
                analysis.clone(),
            );
        }
    }
    (program, bodies)
}

fn write(dir: &Path, name: &str, source: &str) {
    fs::write(dir.join(name), source).unwrap();
}

/// Every import reference of the function `probe` in `main.fab`, each answered
/// by a `PackageImportEffects` for that module with an empty library table.
fn answers(program: &AnalyzedProgram, dir: &Path) -> Vec<ImportedCallee> {
    answers_with(program, dir, &LibraryBodies::new())
}

/// Every import reference of the function `probe` in `main.fab`, answered by
/// a `PackageImportEffects` holding `bodies` as its library table.
fn answers_with(
    program: &AnalyzedProgram,
    dir: &Path,
    bodies: &LibraryBodies,
) -> Vec<ImportedCallee> {
    let (id, node) = program.node_by_path(&dir.join("main.fab")).unwrap();
    let probe = top_level_function(&node.analysis, "probe").expect("probe is declared");
    let references = imported_references(&node.analysis.hir, &node.analysis.types, probe);
    assert!(!references.is_empty(), "probe must reach an import");
    let mut effects = PackageImportEffects::new(&program.nodes, &program.imports, id.clone())
        .with_library_bodies(bodies.clone());
    references
        .iter()
        .map(|reference| effects.callee(reference))
        .collect()
}

const MAIN_CALLS_SEVEN: &str = "import from \"./consts\" consts\n\nfn probe() {\n    consts.seven()\n}\n\nmain {\n    print 0\n}\n";

const MAIN_CALLS_MATH_ADD: &str = "import from \"norma:math\" math\n\nfn probe() → float {\n    return math.add(1.5, 2.5)\n}\n\nmain {\n    print 0\n}\n";

#[test]
fn pure_callee_is_pure() {
    let dir = test_temp_dir("imports-pure");
    let dir = dir.as_ref();
    write(
        dir,
        "consts.fab",
        "@ public\nfn seven() → int { return 7 }\n",
    );
    write(dir, "main.fab", MAIN_CALLS_SEVEN);
    let program = analyze(dir);
    assert_eq!(answers(&program, dir), vec![ImportedCallee::Pure]);
}

#[test]
fn printing_callee_is_an_effect_with_its_own_name_as_chain() {
    let dir = test_temp_dir("imports-print");
    let dir = dir.as_ref();
    write(
        dir,
        "consts.fab",
        "@ public\nfn seven() {\n    print 7\n}\n",
    );
    write(dir, "main.fab", MAIN_CALLS_SEVEN);
    let program = analyze(dir);
    let found = answers(&program, dir);
    let [ImportedCallee::Effect { effect, chain }] = found.as_slice() else {
        panic!("expected one Effect answer");
    };
    assert_eq!(*effect, EFFECT);
    assert_eq!(chain, &vec!["seven".to_owned()]);
}

#[test]
fn callee_reaching_a_third_module_gives_a_chain_across_two_hops() {
    let dir = test_temp_dir("imports-two-hops");
    let dir = dir.as_ref();
    write(
        dir,
        "effects.fab",
        "@ public\nfn loud() {\n    print 1\n}\n",
    );
    write(
        dir,
        "consts.fab",
        "import from \"./effects\" effects\n\n@ public\nfn seven() {\n    effects.loud()\n}\n",
    );
    write(dir, "main.fab", MAIN_CALLS_SEVEN);
    let program = analyze(dir);
    let found = answers(&program, dir);
    let [ImportedCallee::Effect { effect, chain }] = found.as_slice() else {
        panic!("expected one Effect answer");
    };
    assert_eq!(*effect, EFFECT);
    assert_eq!(chain, &vec!["seven".to_owned(), "effects.loud".to_owned()]);
}

#[test]
fn sibling_constant_value_is_pure() {
    let dir = test_temp_dir("imports-const");
    let dir = dir.as_ref();
    write(dir, "consts.fab", "@ public\nconst int BASE = 5\n");
    write(
        dir,
        "main.fab",
        "import from \"./consts\" consts\n\nfn probe() → int {\n    return consts.BASE\n}\n\nmain {\n    print 0\n}\n",
    );
    let program = analyze(dir);
    assert_eq!(answers(&program, dir), vec![ImportedCallee::Pure]);
}

#[test]
fn praefixum_imports_library_without_bodies_is_unavailable() {
    let dir = test_temp_dir("imports-library");
    let dir = dir.as_ref();
    write(
        dir,
        "main.fab",
        "import from \"norma:math\" math\n\nfn probe() → float {\n    return math.PI()\n}\n\nmain {\n    print 0\n}\n",
    );
    let program = analyze(dir);
    assert_eq!(answers(&program, dir), vec![ImportedCallee::Unavailable]);
}

#[test]
fn praefixum_imports_library_pure_callee_is_pure() {
    let dir = test_temp_dir("imports-library-pure");
    let dir = dir.as_ref();
    write(dir, "main.fab", MAIN_CALLS_MATH_ADD);
    let (program, bodies) = analyze_with_library_bodies(dir);
    assert_eq!(
        answers_with(&program, dir, &bodies),
        vec![ImportedCallee::Pure]
    );
}

#[test]
fn praefixum_imports_library_printing_callee_names_the_library_hop() {
    let dir = test_temp_dir("imports-library-effect");
    let dir = dir.as_ref();
    write(
        dir,
        "main.fab",
        "import from \"norma:console\" console\n\nfn probe() {\n    console.write_line(\"x\")\n}\n\nmain {\n    print 0\n}\n",
    );
    let (program, bodies) = analyze_with_library_bodies(dir);
    let (id, node) = program.node_by_path(&dir.join("main.fab")).unwrap();
    let probe = top_level_function(&node.analysis, "probe").unwrap();
    let mut effects = PackageImportEffects::new(&program.nodes, &program.imports, id.clone())
        .with_library_bodies(bodies);
    let answer = function_effect(
        &node.analysis.hir,
        &node.analysis.types,
        &node.analysis.interner,
        probe,
        &mut effects,
    );
    let ImportedCallee::Effect { effect, chain } = answer else {
        panic!("expected one Effect answer");
    };
    assert_eq!(
        chain,
        vec!["probe".to_owned(), "console.write_line".to_owned()]
    );
    assert!(!effect.is_empty());
}

#[test]
fn praefixum_imports_library_unknown_member_is_unavailable() {
    let dir = test_temp_dir("imports-library-unknown");
    let dir = dir.as_ref();
    write(dir, "main.fab", MAIN_CALLS_MATH_ADD);
    let (program, bodies) = analyze_with_library_bodies(dir);
    let (id, reference) = first_reference(&program, dir);
    let (_, main) = program.node_by_path(&dir.join("main.fab")).unwrap();
    let probe = top_level_function(&main.analysis, "probe").unwrap();
    let probe_name = match &main
        .analysis
        .hir
        .items
        .iter()
        .find(|item| item.def_id == probe)
        .unwrap()
        .kind
    {
        HirItemKind::Function(function) => function.name,
        _ => unreachable!(),
    };
    let unknown = ImportedReference {
        member: probe_name,
        ..reference
    };
    let mut effects = PackageImportEffects::new(&program.nodes, &program.imports, id.clone())
        .with_library_bodies(bodies);
    assert_eq!(effects.callee(&unknown), ImportedCallee::Unavailable);
}

/// The first import reference of `probe` in a program whose `probe` calls
/// `consts.seven()`, plus a symbol that no sibling exports (`probe` itself).
fn first_reference(program: &AnalyzedProgram, dir: &Path) -> (ModuleId, ImportedReference) {
    let (id, node) = program.node_by_path(&dir.join("main.fab")).unwrap();
    let probe = top_level_function(&node.analysis, "probe").unwrap();
    let references = imported_references(&node.analysis.hir, &node.analysis.types, probe);
    (id.clone(), references[0])
}

#[test]
fn unknown_member_and_method_references_are_unavailable() {
    let dir = test_temp_dir("imports-unknown");
    let dir = dir.as_ref();
    write(
        dir,
        "consts.fab",
        "@ public\nfn seven() → int { return 7 }\n",
    );
    write(dir, "main.fab", MAIN_CALLS_SEVEN);
    let program = analyze(dir);
    let (id, reference) = first_reference(&program, dir);
    let mut effects = PackageImportEffects::new(&program.nodes, &program.imports, id.clone());
    assert_eq!(effects.callee(&reference), ImportedCallee::Pure);

    // A member the sibling does not export (`probe` is main's own function).
    let (_, main) = program.node_by_path(&dir.join("main.fab")).unwrap();
    let probe_name = match &main
        .analysis
        .hir
        .items
        .iter()
        .find(|item| item.def_id == top_level_function(&main.analysis, "probe").unwrap())
        .unwrap()
        .kind
    {
        HirItemKind::Function(function) => function.name,
        _ => unreachable!(),
    };
    let unknown = ImportedReference {
        member: probe_name,
        ..reference
    };
    assert_eq!(effects.callee(&unknown), ImportedCallee::Unavailable);

    // A method reference is never answered from the program layer.
    let method = ImportedReference {
        kind: ImportedReferenceKind::Method,
        ..reference
    };
    assert_eq!(effects.callee(&method), ImportedCallee::Unavailable);

    // A namespace that is no import of the module.
    let stray = ImportedReference {
        namespace: DefId(u32::MAX),
        ..reference
    };
    assert_eq!(effects.callee(&stray), ImportedCallee::Unavailable);
}

/// Package analysis rejects an import cycle, so the cycle guard is proven on
/// a hand-assembled program: `consts.seven` calls `other.eight` and
/// `other.eight` calls `consts.seven`, each node analyzed acyclically in its
/// own package and the back edge added to the module graph.
#[test]
fn call_cycle_terminates() {
    let forward = test_temp_dir("imports-cycle-forward");
    let forward = forward.as_ref();
    write(
        forward,
        "consts.fab",
        "import from \"./other\" other\n\n@ public\nfn seven() → int {\n    return other.eight()\n}\n",
    );
    write(
        forward,
        "other.fab",
        "@ public\nfn eight() → int { return 8 }\n",
    );
    write(forward, "main.fab", MAIN_CALLS_SEVEN);
    let backward = test_temp_dir("imports-cycle-backward");
    let backward = backward.as_ref();
    write(
        backward,
        "consts.fab",
        "@ public\nfn seven() → int { return 7 }\n",
    );
    write(
        backward,
        "other.fab",
        "import from \"./consts\" consts\n\n@ public\nfn eight() → int {\n    return consts.seven()\n}\n",
    );
    write(
        backward,
        "main.fab",
        "import from \"./other\" other\n\nfn probe() → int {\n    return other.eight()\n}\n\nmain {\n    print 0\n}\n",
    );

    let program = analyze(forward);
    let back = analyze(backward);
    let id_of = |program: &AnalyzedProgram, dir: &Path, file: &str| {
        program.node_by_path(&dir.join(file)).unwrap().0.clone()
    };
    let consts_id = id_of(&program, forward, "consts.fab");
    let other_id = id_of(&program, forward, "other.fab");
    let mut nodes = program.nodes.clone();
    let swapped = back.node_by_path(&backward.join("other.fab")).unwrap().1;
    nodes.insert(other_id.clone(), swapped.clone());
    let mut imports = program.imports.clone();
    imports.insert_local(other_id, "consts".to_owned(), consts_id);

    let (id, reference) = first_reference(&program, forward);
    let mut effects = PackageImportEffects::new(&nodes, &imports, id);
    assert_eq!(effects.callee(&reference), ImportedCallee::Pure);
}
