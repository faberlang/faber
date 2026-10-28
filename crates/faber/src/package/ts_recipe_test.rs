use super::*;
use std::collections::BTreeMap;
use std::ffi::OsString;

fn backend() -> TsRecipe {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    TsRecipe::backend_default(&repository).expect("backend recipe parses")
}

#[test]
fn backend_recipe_builds_with_tsc_project_and_runs_the_launcher() {
    let recipe = backend();
    assert_eq!(recipe.build, ["tsc", "-p", "tsconfig.json"]);
    assert_eq!(
        recipe.run_argv(Path::new("/p/bin/app.js")),
        ["node", "/p/bin/app.js"]
    );
    assert_eq!(recipe.compiler_options["module"], "nodenext");
    assert_eq!(recipe.compiler_options["moduleResolution"], "nodenext");
}

#[test]
fn project_changes_move_only_the_keys_they_state() {
    let project = ManifestTarget {
        compiler_options: BTreeMap::from([("strict".to_owned(), Value::Bool(true))]),
        ..ManifestTarget::default()
    };
    let merged = backend().with_project(Some(&project));
    assert_eq!(merged.compiler_options["strict"], true);
    assert_eq!(merged.compiler_options["target"], "ES2022");
    assert_eq!(merged.build, backend().build);
    assert_eq!(merged.run, backend().run);

    let command = ManifestTarget {
        build: Some(vec!["tsc".into(), "-p".into(), "other.json".into()]),
        ..ManifestTarget::default()
    };
    let merged = backend().with_project(Some(&command));
    assert_eq!(merged.build, ["tsc", "-p", "other.json"]);
    assert_eq!(merged.compiler_options, backend().compiler_options);
}

#[test]
fn tsconfig_carries_the_merged_options() {
    let text = backend().tsconfig();
    let parsed: Value = serde_json::from_str(&text).expect("tsconfig is json");
    assert_eq!(parsed["compilerOptions"]["outDir"], "dist");
    assert_eq!(parsed["include"][0], "src");
}

#[test]
fn missing_tool_fails_by_name() {
    let empty = OsString::from("/nonexistent-faber-path");
    assert!(find_tool("tsc", Some(empty.as_os_str())).is_none());
    assert!(find_tool("tsc", None).is_none());
}

#[test]
fn missing_build_and_run_tools_are_told_apart_by_name() {
    let empty = OsString::from("/nonexistent-faber-path");
    let tsc = require_tool_on(&["tsc".to_owned()], Some(empty.as_os_str())).expect_err("no tsc");
    let node = require_tool_on(&["node".to_owned()], Some(empty.as_os_str())).expect_err("no node");
    assert!(tsc.message.contains("`tsc`"), "{}", tsc.message);
    assert!(node.message.contains("`node`"), "{}", node.message);
    assert!(!tsc.message.contains("`node`"));
}
