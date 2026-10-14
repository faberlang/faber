//! Ledger MIR lowering and validation (need, unit L2c): a ledger
//! literal, index store and `length` method lower to the same MIR collection
//! operations as their map twins and pass MIR validation, without changing the
//! carrier shape. The runner's generic map carrier then executes the program;
//! a lowering failure would surface as a non-`Ok` error (an unsupported
//! lowering or an `invalid MIR` validation message) instead.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, run_source};

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirTypeScript))
}

#[test]
fn ledger_lowers_and_validates() {
    let source = r#"+++
locale = "en"
+++
main {
    var ledger<string, int> x ← { "b": 2, "a": 1 }
    x["c"] ← 3
    print x.length()
}
"#;
    let mut host = BufferHost::default();
    match run_source(&session(), "ledger-lower.fab", source, &mut host) {
        Ok(()) => assert_eq!(host.stdout_lines, ["3"]),
        other => panic!("expected the ledger program to lower, validate and run, got {other:?}"),
    }
}

#[test]
fn ledger_run_length_has_get() {
    let source = r#"+++
locale = "en"
+++
main {
    var ledger<string, int> x ← { "b": 2, "a": 1, "c": 3 }
    print x.length()
    print x.has("c")
    print x.has("zz")
}
"#;
    let mut host = BufferHost::default();
    match run_source(&session(), "ledger-run-length.fab", source, &mut host) {
        Ok(()) => assert_eq!(host.stdout_lines, ["3", "true", "false"]),
        other => panic!("expected the ledger program to lower, validate and run, got {other:?}"),
    }
}

#[test]
fn ledger_run_loop_follows_insertion_order() {
    let source = r#"+++
locale = "en"
+++
main {
    var ledger<string, int> x ← { "b": 2, "a": 1, "c": 3 }
    for from x at [name] const count {
        print name
    }
}
"#;
    let mut host = BufferHost::default();
    match run_source(&session(), "ledger-run-order.fab", source, &mut host) {
        Ok(()) => assert_eq!(host.stdout_lines, ["b", "a", "c"]),
        other => panic!("expected the ledger program to lower, validate and run, got {other:?}"),
    }
}

#[test]
fn ledger_run_update_keeps_position() {
    let source = r#"+++
locale = "en"
+++
main {
    var ledger<string, int> x ← { "b": 2, "a": 1, "c": 3 }
    x["a"] ← 10
    for from x at [name] const count {
        print "§=§"(name, count)
    }
}
"#;
    let mut host = BufferHost::default();
    match run_source(&session(), "ledger-run-update.fab", source, &mut host) {
        Ok(()) => assert_eq!(host.stdout_lines, ["b=2", "a=10", "c=3"]),
        other => panic!("expected the ledger program to lower, validate and run, got {other:?}"),
    }
}

#[test]
fn ledger_run_remove_then_add_appends() {
    let source = r#"+++
locale = "en"
+++
main {
    var ledger<string, int> x ← { "b": 2, "a": 1, "c": 3 }
    x.delete("b")
    x.put("b", 7)
    for from x at [name] const count {
        print name
    }
}
"#;
    let mut host = BufferHost::default();
    match run_source(&session(), "ledger-run-remove-add.fab", source, &mut host) {
        Ok(()) => assert_eq!(host.stdout_lines, ["a", "c", "b"]),
        other => panic!("expected the ledger program to lower, validate and run, got {other:?}"),
    }
}

#[test]
fn ledger_run_map_unaffected() {
    let source = r#"+++
locale = "en"
+++
main {
    var map<string, int> x ← { "b": 2, "a": 1, "c": 3 }
    for from x at [name] const count {
        print name
    }
}
"#;
    let mut host = BufferHost::default();
    match run_source(&session(), "ledger-run-map.fab", source, &mut host) {
        Ok(()) => {
            let mut keys = host.stdout_lines;
            keys.sort();
            assert_eq!(keys, ["a", "b", "c"]);
        }
        other => panic!("expected the map program to lower, validate and run, got {other:?}"),
    }
}

#[test]
fn ledger_run_index_store_and_put_delete() {
    let source = r#"+++
locale = "en"
+++
main {
    var ledger<string, int> x ← { "b": 2, "a": 1, "c": 3 }
    x["a"] ← 10
    x.put("d", 4)
    x.delete("b")
    print x.length()
    print x["a"]
}
"#;
    let mut host = BufferHost::default();
    match run_source(&session(), "ledger-run-store.fab", source, &mut host) {
        Ok(()) => assert_eq!(host.stdout_lines, ["3", "10"]),
        other => panic!("expected the ledger program to lower, validate and run, got {other:?}"),
    }
}
