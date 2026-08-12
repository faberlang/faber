//! `_or` recovery contract tests (promotion packet P6).
//!
//! The five packet fixtures recover from conversion failure with the `⇥`
//! operator; these tests lock the host-binding semantics so the fixtures'
//! expected behavior holds through the host ABI (the Rust oracle's
//! `expr ↦ T ⇥ recovery`). `.expected` files: `valor-scalaria` prints
//! `0` for the recovered value; `instans`/`instans-valor-carrier` print the
//! fallback instant's RFC3339; `octeti` prints `?` / `x` for the recovered
//! text/ascii.

use super::*;
use crate::{Ascii, Instans, InstansPraecisio, Valor};
use std::collections::BTreeMap;

fn epoch_zero() -> Instans {
    Instans::from_epoch_seconds(0, InstansPraecisio::Secunda)
}

fn utc_1979() -> Instans {
    Instans::from_rfc3339("1979-05-27T07:32:00Z", InstansPraecisio::Secunda)
        .expect("valid RFC3339 wire")
}

#[test]
fn valor_scalaria_extracts_scalars_and_recovers_i64() {
    // conversio/valor-scalaria.fab — `count ≡ 42`, `ratio ≡ 3.5`,
    // `widened ≡ 7.0`, `flag ≡ true`, `greeting ≡ "salve"`,
    // `wire ≡ "1979-05-27T07:32:00Z"`, `token ≡ 'yes'`, and
    // `recovered ← "not-a-number" ↦ int ⇥ 0` → `recovered ≡ 0`.
    assert_eq!(valor_get_i64_or(&Valor::Numerus(42), 0), 42);
    assert_eq!(
        valor_get_i64_or(&Valor::Textus("not-a-number".to_owned()), 0),
        0
    );
    assert_eq!(valor_get_f64_or(&Valor::Fractus(3.5), 0.0), 3.5);
    // Numerus widens to fractus losslessly.
    assert_eq!(valor_get_f64_or(&Valor::Numerus(7), 0.0), 7.0);
    assert_eq!(valor_get_i1_or(&Valor::Bivalens(true), false), true);
    assert_eq!(
        valor_get_text_or(&Valor::Textus("salve".to_owned()), "x"),
        "salve"
    );
    // Instans wire strings are accepted as text.
    assert_eq!(
        valor_get_text_or(&Valor::Instans("1979-05-27T07:32:00Z".to_owned()), "x"),
        "1979-05-27T07:32:00Z"
    );
    assert_eq!(
        valor_get_ascii_or(&Valor::Textus("yes".to_owned()), &Ascii::new("x")),
        Ascii::new("yes")
    );
}

#[test]
fn fallibilis_inline_recovery_matches_epoch_zero() {
    // conversio/fallibilis.fab — `inlineRecovery(bad) ≡ epochZero()` and
    // `inlineRecovery(good) ≡ (good ↦ instant)` (bad = "not-a-datetime").
    let fallback = epoch_zero();
    assert_eq!(
        instans_from_valor_or(
            &Valor::Textus("not-a-datetime".to_owned()),
            InstansPraecisio::Secunda,
            fallback,
        ),
        fallback,
    );
    assert_eq!(
        instans_from_valor_or(
            &Valor::Textus("1979-05-27T07:32:00Z".to_owned()),
            InstansPraecisio::Secunda,
            fallback,
        ),
        utc_1979(),
    );
}

#[test]
fn instans_valor_carrier_extracts_and_recovers_through_valor() {
    // conversio/instans-valor-carrier.fab — `seconds ← utc ↦ instant`,
    // `millis ← utc ↦ instant<ms>`, `normalized ← offset ↦ instant` (the
    // `+0900` offset lands on the same UTC instant), and
    // `recovered ← bad ↦ instant ⇥ seconds`.
    let seconds = utc_1979();
    assert_eq!(
        instans_from_valor_or(
            &Valor::Textus("1979-05-27T07:32:00Z".to_owned()),
            InstansPraecisio::Secunda,
            seconds,
        )
        .to_rfc3339(),
        "1979-05-27T07:32:00Z",
    );
    assert_eq!(
        instans_from_valor_or(
            &Valor::Textus("1979-05-27T07:32:00Z".to_owned()),
            InstansPraecisio::Millisecunda,
            seconds,
        )
        .to_rfc3339(),
        "1979-05-27T07:32:00.000Z",
    );
    assert_eq!(
        instans_from_valor_or(
            &Valor::Textus("1979-05-27T16:32:00+0900".to_owned()),
            InstansPraecisio::Secunda,
            seconds,
        )
        .to_rfc3339(),
        "1979-05-27T07:32:00Z",
    );
    assert_eq!(
        instans_from_valor_or(
            &Valor::Textus("not-a-datetime".to_owned()),
            InstansPraecisio::Secunda,
            seconds,
        ),
        seconds,
    );
}

#[test]
fn instans_recovers_text_to_instans_at_precision() {
    // conversio/instans.fab — `micros ← utc ↦ instant<us>`,
    // `nanos ← utc ↦ instant<ns>`, and `recovered ← bad ↦ instant ⇥ seconds`.
    let seconds = utc_1979();
    assert_eq!(
        instans_from_text_or(
            "1979-05-27T07:32:00.123456Z",
            InstansPraecisio::Microsecunda,
            seconds,
        )
        .to_rfc3339(),
        "1979-05-27T07:32:00.123456Z",
    );
    assert_eq!(
        instans_from_text_or(
            "1979-05-27T07:32:00.123456Z",
            InstansPraecisio::Nanosecunda,
            seconds,
        )
        .to_rfc3339(),
        "1979-05-27T07:32:00.123456000Z",
    );
    assert_eq!(
        instans_from_text_or("not-a-datetime", InstansPraecisio::Secunda, seconds),
        seconds,
    );
}

#[test]
fn octeti_recovers_text_and_ascii_decode() {
    // conversio/octeti.fab — `raw |68 69| ↦ string` → "hi",
    // `badUtf8 |ff ff| ↦ string ⇥ "?"` → "?", `raw ↦ ascii` → 'hi', and
    // `nonAscii |ff| ↦ ascii ⇥ 'x'` → 'x'.
    assert_eq!(octeti_get_text_or(&[0x68, 0x69], "?"), "hi");
    assert_eq!(octeti_get_text_or(&[0xff, 0xff], "?"), "?");
    assert_eq!(
        octeti_get_ascii_or(&[0x68, 0x69], &Ascii::new("x")),
        Ascii::new("hi"),
    );
    assert_eq!(
        octeti_get_ascii_or(&[0xff], &Ascii::new("x")),
        Ascii::new("x"),
    );
}

#[test]
fn aggregate_family_applies_fallback_on_wrong_variant() {
    // The valor octeti/array/map/genus `_or` rows: the payload extracts on a
    // matching variant and the fallback substitutes otherwise.
    assert_eq!(
        valor_get_octeti_or(&Valor::Octeti(vec![1, 2]), &[9]),
        vec![1, 2],
    );
    assert_eq!(
        valor_get_octeti_or(&Valor::Textus("x".to_owned()), &[9]),
        vec![9]
    );
    assert_eq!(
        valor_get_array_or(&Valor::Lista(vec![Valor::Numerus(1)]), &[Valor::Numerus(9)]),
        vec![Valor::Numerus(1)],
    );
    assert_eq!(
        valor_get_array_or(&Valor::Textus("x".to_owned()), &[Valor::Numerus(9)]),
        vec![Valor::Numerus(9)],
    );

    let mut entries = BTreeMap::new();
    entries.insert("age".to_owned(), Valor::Numerus(30));
    assert_eq!(
        valor_get_map_or(&Valor::Tabula(entries.clone()), &BTreeMap::new()),
        entries,
    );
    assert_eq!(
        valor_get_map_or(&Valor::Numerus(30), &BTreeMap::new()),
        BTreeMap::new(),
    );
    // Genus is a typed field-view over the `Valor::Tabula` carrier: the
    // carrier-level recovery applies when the valor is not a tabula.
    assert_eq!(
        valor_get_genus_or(&Valor::Tabula(entries.clone()), &BTreeMap::new()),
        entries,
    );
    assert_eq!(
        valor_get_genus_or(&Valor::Numerus(30), &BTreeMap::new()),
        BTreeMap::new(),
    );
}
