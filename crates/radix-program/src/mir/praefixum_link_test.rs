//! ST2-2.7a: a deferred `comptime` site evaluates over a linked import
//! closure and returns the imported callee's value.

use super::praefixum_link::run_deferred_site;
use super::*;
use crate::praefixum_imports::PackageImportEffects;
use radix_mir_runner::Value;
use radix_module::driver::evaluate_deferred;
use std::fs;

const CONSTS: &str = "@ public\nconst int BASE = 5\n\n@ public\nfn seven() → int { return 7 }\n";
const MAIN: &str = "import from \"./consts\" consts\n\nconst int G = comptime {\n    return consts.seven()\n}\n\nmain {\n    print G\n}\n";

#[test]
fn praefixum_link_runs_deferred_site() {
    let dir = crate::test_support::test_temp_dir("praefixum-link");
    let dir = dir.as_ref();
    fs::write(dir.join("consts.fab"), CONSTS).unwrap();
    fs::write(dir.join("main.fab"), MAIN).unwrap();
    let seams = crate::test_support::test_mir_seams();
    let config = Config::default()
        .with_target((seams.executed_lane_target)(&Config::default(), dir).unwrap())
        .with_defer_imported_praefixum();
    let package = crate::analyze::analyze_package(&config, dir, &seams.analysis).unwrap();
    let (module, node) = package.node_by_path(&dir.join("main.fab")).unwrap();
    let (module, anchor_path) = (module.clone(), node.path.clone());
    assert!(
        node.analysis.hir.items.iter().any(|item| matches!(
            &item.kind,
            HirItemKind::Constant(konst)
                if matches!(konst.value.kind, HirExpressionKind::Praefixum(_))
        )),
        "analysis must leave the imported site deferred"
    );

    let mut unit = node.analysis.clone();
    let mut effects = PackageImportEffects::new(&package.nodes, &package.imports, module.clone());
    let mut library_cache = (seams.library_cache_for_config)(&config);
    let mut evaluated = None;
    evaluate_deferred(
        &mut unit,
        "main.fab",
        MAIN,
        &mut effects,
        &mut |scratch, synthetic, span| {
            let value = run_deferred_site(
                &package.spec,
                &package.imports,
                &package.nodes,
                &module,
                scratch,
                &anchor_path,
                synthetic,
                span,
                &config,
                &package.library_resolver,
                &mut library_cache,
            )?;
            evaluated = Some(value.clone());
            Ok(value)
        },
    )
    .unwrap();

    assert_eq!(evaluated, Some(Value::Int(7)));
}
