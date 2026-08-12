//! `cursor_stream` materialization contract tests (promotion packet P5).
//!
//! The three packet fixtures exercise the generator-as-callback contract:
//! `cede/cede.fab` and `cursor/cursor.fab` materialize a generator into a
//! `lista<numerus>` and print `[1, 2]`; `integratio/fluxus-cede.fab` streams
//! and merges `cede` yields (`paria(8)` → even numbers in order). These tests
//! lock the host-binding semantics so the fixtures' expected behavior holds
//! through the host ABI.

use super::{cursor_stream_abi_symbol, materialize_cursor_stream};

#[test]
fn materializes_cede_yields_into_lista() {
    // cede/cede.fab — `fn stream() generator -> int { yield 1; yield 2 }`,
    // `main { print stream() }` → `[1, 2]`.
    let lista = materialize_cursor_stream(|mut sink| {
        sink.cede(1);
        sink.cede(2);
    });
    assert_eq!(lista, vec![1, 2]);
}

#[test]
fn materializes_cursor_annotated_yields_into_lista() {
    // cursor/cursor.fab — the legacy `@ cursor` posture materializes through
    // the same contract → `[1, 2]`.
    let lista = materialize_cursor_stream(|mut sink| {
        sink.cede(1);
        sink.cede(2);
    });
    assert_eq!(lista, vec![1, 2]);
}

#[test]
fn merges_streamed_cede_yields_in_program_order() {
    // integratio/fluxus-cede.fab — `paria(8)` streams and merges even yields
    // (`0 2 4 6`); order is preserved (append, not prepend).
    let lista = materialize_cursor_stream(|mut sink| {
        for i in 0..8 {
            if i % 2 == 0 {
                sink.cede(i);
            }
        }
    });
    assert_eq!(lista, vec![0, 2, 4, 6]);
}

#[test]
fn discards_the_generators_own_return_value() {
    // Reference semantics: the generator's return value is discarded; the
    // materialized list is the yield sequence alone.
    let lista = materialize_cursor_stream(|mut sink| {
        sink.cede(10);
        sink.cede(20);
        "own value — discarded"
    });
    assert_eq!(lista, vec![10, 20]);
}

#[test]
fn empty_generator_returns_empty_lista() {
    let lista: Vec<i64> = materialize_cursor_stream(|_sink| {});
    assert_eq!(lista, Vec::<i64>::new());
}

#[test]
fn abi_symbol_coheres_with_radix_host_abi() {
    // Authority stays `radix-host-abi`; the standalone package carries a
    // committed copy under `crate::contract::abi` (no private Radix dep).
    assert_eq!(
        cursor_stream_abi_symbol(),
        crate::contract::abi::SYMBOL_CURSOR_STREAM,
    );
    assert_eq!(cursor_stream_abi_symbol(), "__faber_rt_v1_cursor_stream");
}
