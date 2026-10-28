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
            assert_eq!(tokens.tokens[0].true_, "true");
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
                                radix_mir::MirStatementKind::RuntimeCall { call, .. }
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
                    Some(&radix_mir::DisplayTokensId(0)),
                    "{id:?}"
                );
            }
        },
    )
    .unwrap_or_else(|errors| panic!("package must link, got {errors:?}"));
}

/// The ids of every function holding a print site, lowest first, with the
/// words each prints through.
fn printing_words(lowered: &LoweredMirUnit<'_>) -> Vec<(MirFunctionId, String)> {
    let tokens = &lowered.validated.validation().display_tokens;
    let mut found = lowered
        .validated
        .program()
        .functions
        .iter()
        .filter(|function| radix_mir_lower::lower::holds_print_site(function))
        .map(|function| {
            let words = tokens
                .for_function(function.id)
                .unwrap_or_else(|| panic!("{:?} prints without an id", function.id));
            (function.id, words.true_.clone())
        })
        .collect::<Vec<_>>();
    found.sort_by_key(|(id, _)| id.0);
    found
}

fn write_two_locale_package(dir: &Path, helper: &str, main: &str) -> PathBuf {
    let entry = write_package(dir);
    let src = dir.join("src");
    fs::write(src.join("helper.fab"), helper).expect("write helper.fab");
    fs::write(&entry, main).expect("write main.fab");
    entry
}

#[test]
fn library_import_in_another_locale_keeps_its_own_tokens() {
    const LA_HELPER: &str = "+++\nlocale = \"la\"\n+++\n@ publica\nfunctio salve() → numerus {\n    nota verum\n    redde 1\n}\n";
    const EN_MAIN: &str = "+++\nlocale = \"en\"\n+++\nimport from \"./helper\" helper\n\nmain {\n    print true\n    print helper.salve()\n}\n";
    let dir = crate::test_support::test_temp_dir("display-tokens-two-locales");
    let entry = write_two_locale_package(dir.as_ref(), LA_HELPER, EN_MAIN);
    with_interpreted_lowered_package_mir(
        &Config::default(),
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let words = printing_words(lowered);
            let mut spelled = words
                .iter()
                .map(|(_, word)| word.as_str())
                .collect::<Vec<_>>();
            spelled.sort_unstable();
            assert_eq!(spelled, ["true", "verum"], "{words:?}");
            assert_eq!(
                words[0].1, "true",
                "the entry file (en) is the lowest id and prints English: {words:?}"
            );
        },
    )
    .unwrap_or_else(|errors| panic!("package must link, got {errors:?}"));
}

#[test]
fn generic_template_tokens_come_from_the_defining_file() {
    // The library's only printing code is a generic template; it is cloned
    // into the English entry file's program.
    const LA_HELPER: &str =
        "+++\nlocale = \"la\"\n+++\n@ publica\nfunctio ostende<T>(T x) {\n    nota x\n}\n";
    const EN_MAIN: &str = "+++\nlocale = \"en\"\n+++\nimport from \"./helper\" helper\n\nmain {\n    helper.ostende(true)\n    print false\n}\n";
    let dir = crate::test_support::test_temp_dir("display-tokens-template");
    let entry = write_two_locale_package(dir.as_ref(), LA_HELPER, EN_MAIN);
    with_interpreted_lowered_package_mir(
        &Config::default(),
        &entry,
        &crate::test_support::test_mir_seams(),
        |lowered| {
            let words = printing_words(lowered);
            let mut spelled = words
                .iter()
                .map(|(_, word)| word.as_str())
                .collect::<Vec<_>>();
            spelled.sort_unstable();
            assert_eq!(
                spelled,
                ["true", "verum"],
                "the clone prints Latin, main English: {words:?}"
            );
            assert_eq!(
                words[0].1, "true",
                "the entry file is the lowest id: {words:?}"
            );
        },
    )
    .unwrap_or_else(|errors| panic!("package must link, got {errors:?}"));
}
