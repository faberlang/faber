//! Structure inventory: what is counted, and what counts as lost.

use super::{Inventory, lost, parse_inventory};
use crate::codegen::Target;
use crate::driver::{Config, Session};

const SUITE: &str = "\
describe \"outer\" {
    setup {
        print \"each\"
    }
    setup all {
        print \"once\"
    }
    teardown {
        print \"after each\"
    }
    async_setup {
        print \"async\"
    }
    test \"first\" {
        assert true
    }
    describe \"inner\" {
        test \"deep\" {
            assert true
        }
    }
}
";

fn inventory_of(body: &str) -> Inventory {
    let source = format!("+++\nlocale = \"en\"\n+++\n{body}");
    let session = Session::new(
        Config::default()
            .with_target(Target::HirFaber)
            .with_dev_locale_root(),
    );
    parse_inventory(&session, "inventory.fab", &source).expect("source parses")
}

fn count(inv: &Inventory, suite: &str, construct: &str) -> usize {
    inv.get(&(suite.to_owned(), construct.to_owned()))
        .map_or(0, |entry| entry.count)
}

#[test]
fn counts_top_level_statement_kinds() {
    let inv = inventory_of(
        "const int a = 1\nif a within 0‥10 {\n    print \"x\"\n}\nif a within 0‥10 {\n    print \"y\"\n}\nmain {}\n",
    );
    assert_eq!(count(&inv, "", "top:var"), 1);
    assert_eq!(count(&inv, "", "top:si"), 2);
    assert_eq!(count(&inv, "", "top:incipit"), 1);
}

#[test]
fn counts_every_hook_kind_and_suite_member() {
    let inv = inventory_of(&format!("{SUITE}\nmain {{}}\n"));
    assert_eq!(count(&inv, "", "top:probandum"), 1);
    assert_eq!(count(&inv, "/outer", "hook:Praepara:all=false"), 1);
    assert_eq!(count(&inv, "/outer", "hook:Praepara:all=true"), 1);
    assert_eq!(count(&inv, "/outer", "hook:Postpara:all=false"), 1);
    assert_eq!(count(&inv, "/outer", "hook:Praeparabit:all=false"), 1);
    assert_eq!(count(&inv, "/outer", "test:first"), 1);
    assert_eq!(count(&inv, "/outer", "nested-suite:inner"), 1);
    assert_eq!(count(&inv, "/outer/inner", "test:deep"), 1);
}

#[test]
fn does_not_count_statements_inside_bodies() {
    let inv = inventory_of("fn f() → int {\n    if true {\n        print \"x\"\n    }\n    return 1\n}\n");
    assert_eq!(count(&inv, "", "top:func"), 1);
    assert_eq!(count(&inv, "", "top:si"), 0);
}

#[test]
fn lost_reports_a_missing_hook() {
    let source = inventory_of(&format!("{SUITE}\nmain {{}}\n"));
    let output = inventory_of(
        "describe \"outer\" {\n    test \"first\" {\n        assert true\n    }\n    describe \"inner\" {\n        test \"deep\" {\n            assert true\n        }\n    }\n}\n\nmain {}\n",
    );
    let constructs: Vec<String> = lost(&source, &output)
        .into_iter()
        .map(|found| format!("{} {}", found.suite, found.construct))
        .collect();
    assert_eq!(
        constructs,
        [
            "/outer hook:Postpara:all=false",
            "/outer hook:Praepara:all=false",
            "/outer hook:Praepara:all=true",
            "/outer hook:Praeparabit:all=false",
        ]
    );
}

#[test]
fn lost_reports_a_missing_top_level_statement() {
    let source = inventory_of("const int a = 1\nif a within 0‥10 {\n    print \"x\"\n}\nmain {}\n");
    let output = inventory_of("const int a = 1\nmain {}\n");
    let found = lost(&source, &output);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].construct, "top:si");
    assert_eq!((found[0].source_count, found[0].output_count), (1, 0));
}

#[test]
fn lost_reports_a_missing_nested_test() {
    let source = inventory_of(&format!("{SUITE}\nmain {{}}\n"));
    let output = inventory_of(
        "describe \"outer\" {\n    setup {\n        print \"each\"\n    }\n    setup all {\n        print \"once\"\n    }\n    teardown {\n        print \"after each\"\n    }\n    async_setup {\n        print \"async\"\n    }\n    test \"first\" {\n        assert true\n    }\n    describe \"inner\" {\n    }\n}\n\nmain {}\n",
    );
    let found = lost(&source, &output);
    assert_eq!(found.len(), 1);
    assert_eq!(
        (found[0].suite.as_str(), found[0].construct.as_str()),
        ("/outer/inner", "test:deep")
    );
}

#[test]
fn lost_ignores_constructs_only_the_output_has() {
    let source = inventory_of("main {}\n");
    let output = inventory_of("const int a = 1\nmain {}\n");
    assert!(lost(&source, &output).is_empty());
}
