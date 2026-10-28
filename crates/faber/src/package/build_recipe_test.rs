use super::*;
use std::ffi::OsString;

const TOOLCHAIN: Toolchain = Toolchain {
    programs: "Demo",
    issue: "package_demo_tool_missing",
};

fn words(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn expand_replaces_values_in_place_and_lists_as_words() {
    let flags = words(&["-trimpath", "-tags", "a,b"]);
    let argv = words(&["go", "build", "{flags}", "-o", "{artifact}", "."]);
    assert_eq!(
        expand(&argv, &[("artifact", "/out/prog")], &[("flags", &flags)]),
        [
            "go",
            "build",
            "-trimpath",
            "-tags",
            "a,b",
            "-o",
            "/out/prog",
            "."
        ]
    );
    assert_eq!(
        expand(&argv, &[("artifact", "/out/prog")], &[("flags", &[])]),
        ["go", "build", "-o", "/out/prog", "."]
    );
    assert_eq!(
        expand(&words(&["--out={artifact}"]), &[("artifact", "x")], &[]),
        ["--out=x"]
    );
}

#[test]
fn a_missing_tool_fails_naming_the_tool() {
    let empty = OsString::from("/nonexistent-faber-path");
    let diag = require_tool_on(&words(&["frobnicate"]), Some(empty.as_os_str()), &TOOLCHAIN)
        .expect_err("not on PATH");
    assert_eq!(diag.issue(), Some("package_demo_tool_missing"));
    assert!(
        diag.args
            .iter()
            .any(|arg| arg.name == "tool" && arg.value == "frobnicate")
    );
}

#[test]
fn project_commands_replace_only_the_commands_they_state() {
    let base = Commands {
        build: words(&["b"]),
        run: words(&["r"]),
    };
    assert_eq!(base.clone().with_project(None), base);
    let project = ManifestTarget {
        run: Some(words(&["other"])),
        ..ManifestTarget::default()
    };
    let merged = base.clone().with_project(Some(&project));
    assert_eq!(merged.build, base.build);
    assert_eq!(merged.run, ["other"]);
}

#[test]
fn file_stems_are_made_safe() {
    assert_eq!(sanitize_file_stem("my pkg/1"), "my_pkg_1");
    assert_eq!(sanitize_file_stem(""), "program");
}
