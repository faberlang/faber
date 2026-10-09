//! Linked modules merge their display-token tables: every function that holds
//! a print-family site keeps an id, remapped past the unit's id shift, and
//! each id points at its own table entry (need, plumbing only).

use super::*;

const HELPER: &str = r#"@ public
fn greet() → int {
    print "helper"
    return 1
}
"#;

const MAIN: &str = r#"import from "./helper" helper

main {
    print helper.greet()
}
"#;

fn write_package(dir: &Path) -> PathBuf {
    let src = dir.join("src");
    fs::create_dir_all(&src).expect("create package src");
    fs::write(
        dir.join("faber.toml"),
        r#"
[package]
name = "display-tokens-link"
version = "1.0.0"
edition = "2026"

[paths]
source = "src"
entry = "main.fab"

[build]
kind = "bin"
"#,
    )
    .expect("write faber.toml");
    fs::write(src.join("helper.fab"), HELPER).expect("write helper.fab");
    fs::write(src.join("main.fab"), MAIN).expect("write main.fab");
    src.join("main.fab")
}

#[test]
fn linked_modules_remap_display_token_ids_so_each_function_keeps_its_entry() {
    let dir = crate::test_support::test_temp_dir("display-tokens-link");
    let entry = write_package(dir.as_ref());
    with_interpreted_lowered_package_mir(
        &Config::default(),
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let tokens = &lowered.validated.validation().display_tokens;
            assert_eq!(tokens.tokens.len(), 1);
            assert_eq!(tokens.tokens[0].true_, "verum");
            let printing = lowered
                .validated
                .program()
                .functions
                .iter()
                .filter(|function| {
                    function
                        .blocks
                        .iter()
                        .flat_map(|block| &block.statements)
                        .any(|statement| {
                            matches!(
                                &statement.kind,
                                radix_module::mir::MirStatementKind::RuntimeCall { call, .. }
                                    if matches!(call.intrinsic, MirIntrinsic::Diagnostic(_))
                            )
                        })
                })
                .map(|function| function.id)
                .collect::<Vec<_>>();
            assert!(
                printing.len() >= 2,
                "main and the sibling both print: {printing:?}"
            );
            for id in printing {
                assert_eq!(
                    tokens.by_function.get(&id),
                    Some(&radix_module::mir::DisplayTokensId(0)),
                    "{id:?}"
                );
            }
        },
    )
    .unwrap_or_else(|errors| panic!("package must link, got {errors:?}"));
}
