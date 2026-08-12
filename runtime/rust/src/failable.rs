//! Failable status/payload host bindings (promotion packet P10).
//!
//! The shared failable status/payload model is the **status-first multi-value
//! `(i32 status, payload…)`** of the W10 profile: `STATUS_OK` + the success
//! payload on the happy path, `STATUS_FALLIBLE` + the typed `ReturnError`
//! payload when the failable error channel fires. `iace` raises a typed error
//! value, `→ T ⇥ E` propagates it, and `fac { … } cape err { … }` absorbs it
//! with the declared recovery; the runtime's dynamic [`Valor`] carrier is the
//! payload vehicle across the host boundary.
//!
//! The four P10 fixtures exercise the family:
//! - `iace/functio-fallibilis.fab` — fac/cape recovery of a `⇥` error (prints
//!   `5` then `0`, recovered from division-by-zero);
//! - `iace/functio-propagans.fab` — `⇥` propagation through a call chain
//!   (try_call re-throws without a local handler);
//! - `iace/iace-si-guard.fab` — `iace` with a `si` guard (conditional throw);
//! - `operatores/function-types.fab` — `→ T` / `→ T ⇥ E` signature types.
//!
//! The `valor_cape` row covers the `fac`/`cape` channel; this module is the
//! status/payload contract the channel crosses on.

use crate::contract::abi::{FaberRtStatusV1, STATUS_FALLIBLE, STATUS_OK};
use crate::Valor;

/// Build the status-first failable error pair
/// (`__faber_rt_v1_fallible_error`): `(STATUS_FALLIBLE, error)`.
#[must_use]
pub fn fallible_error(error: Valor) -> (FaberRtStatusV1, Valor) {
    (STATUS_FALLIBLE, error)
}

/// Build the status-first happy-path pair: `(STATUS_OK, payload)`.
#[must_use]
pub fn failable_ok(payload: Valor) -> (FaberRtStatusV1, Valor) {
    (STATUS_OK, payload)
}

/// Whether a status discriminates the failable error channel — the
/// status-first check of the `(i32 status, payload…)` shape.
#[must_use]
pub fn is_fallible(status: FaberRtStatusV1) -> bool {
    status.code == STATUS_FALLIBLE.code
}

/// The `cape err` recovery read: extract the typed error payload from a
/// failable result; `None` when the status is not the error channel.
#[must_use]
pub fn error_payload(status: FaberRtStatusV1, payload: Valor) -> Option<Valor> {
    if is_fallible(status) {
        Some(payload)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "failable_test.rs"]
mod tests;
