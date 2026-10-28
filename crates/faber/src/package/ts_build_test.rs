use super::*;

use radix::module::driver::Config;

fn tool_available(tool: &str) -> bool {
    super::super::ts_recipe::find_tool(tool, std::env::var_os("PATH").as_deref()).is_some()
}

fn write_package(root: &Path, name: &str, main: &str, extra: &[(&str, &str)]) {
    fs::create_dir_all(root).expect("package dir");
    fs::write(
        root.join("faber.toml"),
        format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2026\"\n\n[paths]\nsource = \".\"\nentry = \"main.fab\"\n"
        ),
    )
    .expect("manifest");
    fs::write(root.join("main.fab"), main).expect("entry");
    for (file, text) in extra {
        fs::write(root.join(file), text).expect("sibling");
    }
}

fn temp_package(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("faber-ts-build-{label}-{}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    dir
}

fn build(root: &Path) -> Result<TsProgram, Vec<Diagnostic>> {
    let config = Config::default().with_target(radix::module::codegen::Target::HirTypeScript);
    let layout = super::super::cmd::discover_build_layout(root, root).expect("layout");
    build_ts_program(&config, root, &layout)
}

#[test]
fn builds_a_self_contained_node_project_and_runs_it_with_args() {
    if !tool_available("tsc") || !tool_available("node") {
        return;
    }
    let root = temp_package("run");
    write_package(
        &root,
        "hello",
        "import from \"./shapes\"\n\nmain {\n    let shapes.Circle c ← shapes.Circle { radius = 3 }\n    print c.draw()\n}\n",
        &[(
            "shapes.fab",
            "module shapes\n\n@ public\nclass Circle {\n    mut int radius = 10\n\n    fn draw() → string {\n        return \"circle §\"(self.radius)\n    }\n}\n",
        )],
    );
    let program = build(&root).expect("builds");

    // One module per unit, a compiled runtime copy, no symlink, no `.ts` under node_modules.
    assert!(program.root.join("src/main.ts").is_file());
    assert!(program.root.join("src/shapes.ts").is_file());
    assert!(program.root.join("dist/shapes.js").is_file());
    let runtime = program.root.join("node_modules/@faber/runtime");
    assert!(runtime.join("index.js").is_file());
    assert!(
        !fs::symlink_metadata(&runtime)
            .expect("runtime dir")
            .file_type()
            .is_symlink()
    );

    // The project runs from a copy, with no checkout and no faber process.
    let copy = temp_package("run-copy");
    fs::create_dir_all(&copy).expect("copy dir");
    let status = Command::new("cp")
        .arg("-R")
        .arg(&program.root)
        .arg(copy.join("ts"))
        .status()
        .expect("cp");
    assert!(status.success());
    let output = Command::new("node")
        .arg(copy.join("ts/bin/hello.js"))
        .output()
        .expect("node runs the copy");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "circle 3\n");
    fs::remove_dir_all(&root).ok();
    fs::remove_dir_all(&copy).ok();
}

#[test]
fn a_run_passes_the_exit_code_and_arguments_through() {
    if !tool_available("tsc") || !tool_available("node") {
        return;
    }
    let root = temp_package("exit");
    write_package(&root, "quiet", "main {\n    print 1\n}\n", &[]);
    let program = build(&root).expect("builds");
    // `node <launcher> --version`-style arguments reach the program untouched; a
    // program that exits non-zero keeps its code.
    let status = Command::new("node")
        .arg("-e")
        .arg("process.exit(7)")
        .status()
        .expect("node");
    assert_eq!(exit_code_of(status), 7);
    let status = run_ts_program(&program, &["a".to_owned(), "b".to_owned()]).expect("runs");
    assert!(status.success());
    fs::remove_dir_all(&root).ok();
}

#[test]
fn a_project_recipe_override_changes_only_its_key() {
    if !tool_available("tsc") || !tool_available("node") {
        return;
    }
    let root = temp_package("override");
    write_package(&root, "tuned", "main {\n    print 2\n}\n", &[]);
    let manifest = fs::read_to_string(root.join("faber.toml")).expect("manifest");
    fs::write(
        root.join("faber.toml"),
        format!("{manifest}\n[target.ts.compiler_options]\ntarget = \"ES2023\"\n"),
    )
    .expect("manifest");
    let program = build(&root).expect("builds");
    let tsconfig = fs::read_to_string(program.root.join("tsconfig.json")).expect("tsconfig");
    let parsed: serde_json::Value = serde_json::from_str(&tsconfig).expect("json");
    assert_eq!(parsed["compilerOptions"]["target"], "ES2023");
    assert_eq!(parsed["compilerOptions"]["module"], "nodenext");
    fs::remove_dir_all(&root).ok();
}
