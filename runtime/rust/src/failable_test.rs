//! Failable status/payload binding tests (promotion packet P10).

use super::{error_payload, failable_ok, fallible_error, is_fallible};
use crate::Valor;
use crate::contract::abi::{FaberRtStatusV1, STATUS_FALLIBLE, STATUS_OK};

/// `fallible_error` pairs the error channel status first with the typed
/// error payload — the `ReturnError` carrier of the four P10 fixtures.
#[test]
fn fallible_error_pairs_status_first_with_payload() {
    let error = Valor::Textus("division by zero".to_owned());
    let (status, payload) = fallible_error(error.clone());
    assert_eq!(status, STATUS_FALLIBLE);
    assert_eq!(payload, error);
}

/// The happy path is `(STATUS_OK, payload)` — the discriminator's other arm.
#[test]
fn failable_ok_pairs_ok_status_with_payload() {
    let payload = Valor::Numerus(5);
    let (status, result) = failable_ok(payload.clone());
    assert_eq!(status, STATUS_OK);
    assert_eq!(result, payload);
}

/// `is_fallible` discriminates the error channel only.
#[test]
fn is_fallible_discriminates_the_error_channel() {
    assert!(is_fallible(STATUS_FALLIBLE));
    assert!(!is_fallible(STATUS_OK));
    assert!(!is_fallible(FaberRtStatusV1 { code: 1 }));
}

/// The `cape err` recovery read extracts the typed payload only when the
/// status is the failable channel (the `functio-fallibilis` recovery shape).
#[test]
fn error_payload_extracts_only_on_the_error_channel() {
    let error = Valor::Textus("empty input".to_owned());
    assert_eq!(error_payload(STATUS_FALLIBLE, error.clone()), Some(error),);
    assert_eq!(error_payload(STATUS_OK, Valor::Nihil), None);
}
