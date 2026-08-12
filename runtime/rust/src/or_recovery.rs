//! `_or` recovery host bindings (promotion packet P6).
//!
//! The `⇥` inline-fallback channel (`expr ↦ T ⇥ recovery`) lowers through the
//! shared ABI as a fixed-signature `(payload + fallback → payload)` row per
//! closed-set conversion. This module is the target-neutral recovery contract
//! hosts implement against: the typed extraction runs first ([`FromValor`],
//! [`Instans::try_from_valor`], octeti decode) and on a missing or
//! wrong-typed payload the fallback value substitutes instead of aborting.
//!
//! The five P6 fixtures exercise the family through the host ABI:
//! - `conversio/valor-scalaria.fab` — `valor_get_i64_or`
//! - `conversio/fallibilis.fab` + `conversio/instans-valor-carrier.fab` —
//!   `instans_from_valor_or`
//! - `conversio/instans.fab` — `instans_from_text_or`
//! - `conversio/octeti.fab` — `octeti_get_text_or` + `octeti_get_ascii_or`
//!
//! Reference semantics: the Rust oracle's `↦ T ⇥ recovery` — the conversion
//! runs first and the recovery value is substituted on failure. The
//! `valor_cape` row covers the `fac`/`cape` channel, not this inline
//! fallback; the parse `_or` rows (`text_parse_integer_or` /
//! `text_parse_float_or`) and `option_get_or` are the established closed-set
//! `_or` precedent.

use crate::{Ascii, FromValor, Instans, InstansPraecisio, Valor};
use std::collections::BTreeMap;

/// `valor_get_i64_or` (`__faber_rt_v1_valor_get_i64_or`): extract the
/// `Numerus` payload of a `valor`; substitute `fallback` on a missing or
/// wrong-typed payload.
#[must_use]
pub fn valor_get_i64_or(valor: &Valor, fallback: i64) -> i64 {
    i64::from_valor(valor).unwrap_or(fallback)
}

/// `valor_get_f64_or` (`__faber_rt_v1_valor_get_f64_or`): extract the
/// `Fractus`/`Numerus` payload of a `valor` (widen); substitute `fallback`
/// on a missing or wrong-typed payload.
#[must_use]
pub fn valor_get_f64_or(valor: &Valor, fallback: f64) -> f64 {
    f64::from_valor(valor).unwrap_or(fallback)
}

/// `valor_get_i1_or` (`__faber_rt_v1_valor_get_i1_or`): extract the
/// `Bivalens` payload of a `valor`; substitute `fallback` on a missing or
/// wrong-typed payload.
#[must_use]
pub fn valor_get_i1_or(valor: &Valor, fallback: bool) -> bool {
    bool::from_valor(valor).unwrap_or(fallback)
}

/// `valor_get_text_or` (`__faber_rt_v1_valor_get_text_or`): extract the
/// `Textus`/`Instans` wire payload of a `valor`; substitute `fallback` on a
/// missing or wrong-typed payload.
#[must_use]
pub fn valor_get_text_or(valor: &Valor, fallback: &str) -> String {
    String::from_valor(valor).unwrap_or_else(|| fallback.to_owned())
}

/// `valor_get_ascii_or` (`__faber_rt_v1_valor_get_ascii_or`): extract the
/// ASCII payload of a `valor`; substitute `fallback` on a missing, non-text,
/// or non-ASCII payload.
#[must_use]
pub fn valor_get_ascii_or(valor: &Valor, fallback: &Ascii) -> Ascii {
    Ascii::from_valor(valor).unwrap_or_else(|| fallback.clone())
}

/// `valor_get_octeti_or` (`__faber_rt_v1_valor_get_octeti_or`): extract the
/// `Octeti` payload of a `valor`; substitute `fallback` on a missing or
/// wrong-typed payload.
#[must_use]
pub fn valor_get_octeti_or(valor: &Valor, fallback: &[u8]) -> Vec<u8> {
    match valor {
        Valor::Octeti(bytes) => bytes.clone(),
        _ => fallback.to_vec(),
    }
}

/// `valor_get_array_or` (`__faber_rt_v1_valor_get_array_or`): extract the
/// `Lista` payload of a `valor`; substitute `fallback` on a missing or
/// wrong-typed payload.
#[must_use]
pub fn valor_get_array_or(valor: &Valor, fallback: &[Valor]) -> Vec<Valor> {
    match valor {
        Valor::Lista(items) => items.clone(),
        _ => fallback.to_vec(),
    }
}

/// `valor_get_map_or` (`__faber_rt_v1_valor_get_map_or`): extract the
/// `Tabula` payload of a `valor`; substitute `fallback` on a missing or
/// wrong-typed payload.
#[must_use]
pub fn valor_get_map_or(
    valor: &Valor,
    fallback: &BTreeMap<String, Valor>,
) -> BTreeMap<String, Valor> {
    match valor {
        Valor::Tabula(entries) => entries.clone(),
        _ => fallback.clone(),
    }
}

/// `valor_get_genus_or` (`__faber_rt_v1_valor_get_genus_or`): extract the
/// genus carrier (`Valor::Tabula`) of a `valor`; substitute `fallback` on a
/// missing or wrong-typed payload. The per-field kind/defaultable layout is
/// the P7 genus field-layout contract; this row applies the fallback at the
/// carrier level.
#[must_use]
pub fn valor_get_genus_or(
    valor: &Valor,
    fallback: &BTreeMap<String, Valor>,
) -> BTreeMap<String, Valor> {
    match valor {
        Valor::Tabula(fields) => fields.clone(),
        _ => fallback.clone(),
    }
}

/// `octeti_get_text_or` (`__faber_rt_v1_octeti_get_text_or`): UTF-8 decode an
/// `octeti` payload; substitute `fallback` on invalid UTF-8 bytes.
#[must_use]
pub fn octeti_get_text_or(bytes: &[u8], fallback: &str) -> String {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .unwrap_or_else(|_| fallback.to_owned())
}

/// `octeti_get_ascii_or` (`__faber_rt_v1_octeti_get_ascii_or`): ASCII-narrow
/// an `octeti` payload; substitute `fallback` on any byte ≥ 128.
#[must_use]
pub fn octeti_get_ascii_or(bytes: &[u8], fallback: &Ascii) -> Ascii {
    Ascii::try_from_bytes(bytes).unwrap_or_else(|| fallback.clone())
}

/// `instans_from_text_or` (`__faber_rt_v1_instans_from_text_or`): parse an
/// RFC3339 wire text into an `instans<N>`; substitute `fallback` when the
/// wire is not a datetime.
#[must_use]
pub fn instans_from_text_or(text: &str, praecisio: InstansPraecisio, fallback: Instans) -> Instans {
    Instans::from_rfc3339(text, praecisio).unwrap_or(fallback)
}

/// `instans_from_valor_or` (`__faber_rt_v1_instans_from_valor_or`): extract an
/// `instans<N>` from a dynamic `valor` carrier (`Instans`/`Textus` wire or
/// `Numerus` epoch); substitute `fallback` on a missing or wrong-typed
/// payload.
#[must_use]
pub fn instans_from_valor_or(
    valor: &Valor,
    praecisio: InstansPraecisio,
    fallback: Instans,
) -> Instans {
    Instans::try_from_valor(valor, praecisio).unwrap_or(fallback)
}

#[cfg(test)]
#[path = "or_recovery_test.rs"]
mod tests;
