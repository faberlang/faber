//! ST2-2.2 clone seam: a cloned import closure links through the test-link
//! machinery and runs an imported call, without touching the original program.

use super::*;
use radix_mir_runner::BufferHost;

#[test]
fn cloned_closure_links_and_runs_imported_call() {
    let dir = crate::test_support::test_temp_dir("clone-link");
    let dir = dir.as_ref();
    fs::write(
        dir.join("consts.fab"),
        "@ public\nconst int BASE = 5\n\n@ public\nfn seven() → int { return 7 }\n",
    )
    .unwrap();
    fs::write(
        dir.join("main.fab"),
        "import from \"./consts\" consts\n\nfn pre() {\n    print consts.seven() + consts.BASE\n}\n\nmain {\n    print 0\n}\n",
    )
    .unwrap();
    let seams = crate::test_support::test_mir_seams();
    let config = Config::default()
        .with_target((seams.executed_lane_target)(&Config::default(), dir).unwrap());
    let package = crate::analyze::analyze_package(&config, dir, &seams.analysis).unwrap();
    let (entry_id, entry_node) = package.node_by_path(&dir.join("main.fab")).unwrap();
    let (entry_id, entry_path) = (entry_id.clone(), entry_node.path.clone());
    let pre = entry_node
        .analysis
        .hir
        .items
        .iter()
        .find(|item| {
            matches!(&item.kind, HirItemKind::Function(f)
                if entry_node.analysis.interner.resolve(f.name) == "pre")
        })
        .map(|item| item.def_id)
        .unwrap();
    let closure = package.imports.local_closure(std::iter::once(&entry_id));
    let mut nodes: BTreeMap<ModuleId, AnalyzedProgramNode> = package
        .nodes
        .iter()
        .filter(|(id, _)| closure.contains(*id))
        .map(|(id, node)| (id.clone(), node.clone()))
        .collect();
    let synthetic = radix_module::hir::DefId(0x3FFF_FF00);
    let node = nodes.get_mut(&entry_id).unwrap();
    let mut item = node
        .analysis
        .hir
        .items
        .iter()
        .find(|i| i.def_id == pre)
        .unwrap()
        .clone();
    item.def_id = synthetic;
    item.id = radix_module::hir::HirId(9_999_999);
    node.analysis.hir.items.push(item);
    let eval = AnalyzedProgram {
        spec: package.spec.clone(),
        entry: Some(entry_id.clone()),
        roots: vec![entry_id],
        nodes,
        imports: package.imports.clone(),
        entry_frontmatter: None,
        diagnostics: Vec::new(),
        library_resolver: package.library_resolver.clone(),
        routes: Default::default(),
    };
    let scope = TestPackageScope {
        remap_anchor: entry_path.clone(),
        functions: vec![(entry_path.clone(), synthetic)],
    };
    let lines = with_linked_test_package_mir(eval, &scope, &config, &seams, |validated, ids| {
        let mut host = BufferHost::default();
        radix_mir_runner::run_function(validated, ids[&(entry_path.clone(), synthetic)], &mut host)
            .unwrap();
        host.stdout_lines
    })
    .unwrap();
    assert_eq!(lines, vec!["12".to_owned()]);
    // The analyzed program was not touched.
    let (_, original) = package.node_by_path(&dir.join("main.fab")).unwrap();
    assert!(
        original
            .analysis
            .hir
            .items
            .iter()
            .all(|item| item.def_id != synthetic)
    );
}
