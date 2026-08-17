//! `Cede` / `CursorStream` runtime semantics tests.
//!
//! WHY: `cede` is the cursor yield surface, while await-bind uses
//! `figendum` / `variandum`. `CursorStream` materializes a legacy `@ cursor`
//! generator into a `lista<T>`. These tests lock the runtime behavior directly
//! so it survives exempla churn.

use crate::driver::{Config, Session};
use crate::mir::runner::{run_source, BufferHost};
use std::path::Path;

fn run(source: &str) -> Result<BufferHost, String> {
    let session = Session::new(Config::default());
    let mut host = BufferHost::default();
    match run_source(&session, "cede_cursor_test.fab", source, &mut host) {
        Ok(()) => Ok(host),
        Err(error) => Err(format!("{error:?}")),
    }
}

#[test]
fn figendum_in_incipiet_awaits_synchronously() {
    // Await-bind on a legacy @ futura call resolves synchronously and returns
    // the callee's value (the runner has no real async runtime).
    let source =
        "@ futura\nfunctio accipe() → textus { redde \"paratum\" }\nincipiet { figendum textus v ← accipe()\n nota v }";
    let host = run(source).expect("await runs");
    assert_eq!(host.stdout_lines, vec!["paratum".to_owned()]);
}

#[test]
fn cursor_stream_materializes_yields_into_lista() {
    // A @ cursor function's cede values are collected by CursorStream into a
    // lista<T>; the bare call prints the materialized list.
    let source = "@ cursor\nfunctio stream() → numerus { cede 1\n cede 2\n }\nincipit { nota stream() }";
    let host = run(source).expect("cursor materializes");
    assert_eq!(host.stdout_lines, vec!["[1, 2]".to_owned()]);
}

#[test]
fn cursor_stream_feeds_indexed_iteration() {
    // `itera ex` over a @ cursor call lowers to CursorStream; the materialized
    // lista drives the loop. Each yielded value prints in order.
    let source = "@ cursor\nfunctio numeri() → numerus { cede 0\n cede 1\n cede 2\n }\nincipit {\n  itera ex numeri() fixum n {\n    nota n\n  }\n}";
    let host = run(source).expect("cursor iteration runs");
    assert_eq!(host.stdout_lines, vec!["0".to_owned(), "1".to_owned(), "2".to_owned()]);
}

#[test]
fn cursor_stream_preserves_order_across_many_yields() {
    // WHY: yield order must be preserved (append, not prepend) so generator
    // semantics match Rust iterator order.
    let source = "@ cursor\nfunctio count() → numerus { cede 10\n cede 20\n cede 30\n }\nincipit { nota count() }";
    let host = run(source).expect("ordered yields run");
    assert_eq!(host.stdout_lines, vec!["[10, 20, 30]".to_owned()]);
}

#[test]
fn async_cursor_itera_materializes_yields_in_order() {
    // WHY: `itera ex` over an async generator (`fient`) call must lower
    // through the shared `CollectionIteraKind` surface (P2) and iterate the
    // materialized yields in order, matching the sync cursor contract. The
    // Rust oracle already runs this shape (HIR path); the shared MIR fact is
    // what promotion packet P2 adds.
    let source = "+++\nterm = \"probe\"\nkind = \"test\"\nlocale = \"en\"\n+++\n\nfn grena_futurum(int n) async_generator → int {\n  for range 0‥n const i {\n    yield i\n  }\n}\nasync_main {\n  for from grena_futurum(3) const num {\n    print \"async numerus: §\"(num)\n  }\n}";
    let host = run(source).expect("async cursor iteration runs");
    assert_eq!(
        host.stdout_lines,
        vec![
            "async numerus: 0".to_owned(),
            "async numerus: 1".to_owned(),
            "async numerus: 2".to_owned(),
        ]
    );
}

#[test]
fn cursor_iteratio_fixture_runs_through_shared_path() {
    // WHY: promotion packet P2's done-when is the fixture lowering through
    // the shared MIR path and matching the Rust-oracle expected output
    // (`.expected` at radix/corpus/itera/cursor-iteratio.expected).
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/itera/cursor-iteratio.fab");
    let source = std::fs::read_to_string(&path).expect("read itera/cursor-iteratio.fab");
    let host = run(&source).expect("cursor-iteratio fixture runs through the shared path");
    assert_eq!(
        host.stdout_lines,
        vec![
            "Sync cursor iteration:".to_owned(),
            "  numerus: 0".to_owned(),
            "  numerus: 1".to_owned(),
            "  numerus: 2".to_owned(),
            "Sync collected:".to_owned(),
            "[0, 2, 4, 6, 8]".to_owned(),
            "Async cursor iteration:".to_owned(),
            "  async numerus: 0".to_owned(),
            "  async numerus: 1".to_owned(),
            "  async numerus: 2".to_owned(),
        ]
    );
}
