//! The conversation core over the shared native leaves: clock replies, a unit
//! reply, a leaf fault reduced to kind and path, and a route no leaf owns.

use super::{Conversation, Opener};
use faber_leaf::LeafErrorKind;

#[test]
fn clock_routes_reply_with_an_integer() {
    for route in ["tempus:nunc", "tempus:monotonicum"] {
        let mut conversation = Conversation::open(route);
        let nanos = conversation.int().expect("a clock route replies numerus");
        assert!(nanos >= 0, "{route} replied {nanos}");
    }
}

#[test]
fn sleep_with_a_zero_duration_replies_unit() {
    let mut conversation = Conversation::open("tempus:dormiet").with_opener(&[Opener::Int(0)]);
    assert_eq!(conversation.unit(), Ok(()));
}

#[test]
fn missing_file_fault_carries_kind_and_path() {
    const PATH: &str = "/nonexistent/faber-clif-sermo/fault-kind";

    let mut conversation =
        Conversation::open("solum:lege").with_opener(&[Opener::Str(PATH.to_owned())]);
    let fault = conversation
        .materialize()
        .expect_err("a missing path cannot be read");

    assert_eq!(fault.kind(), LeafErrorKind::NotFound);
    assert_eq!(fault.kind_name(), LeafErrorKind::NotFound.variant_name());
    assert_eq!(fault.path(), PATH);
}

#[test]
fn unknown_route_fails_closed_naming_the_route() {
    const ROUTE: &str = "tempus:ignotum";

    let mut conversation = Conversation::open(ROUTE);
    let fault = conversation
        .materialize()
        .expect_err("no leaf owns this route");

    assert_eq!(fault.kind(), LeafErrorKind::Unavailable);
    assert_eq!(fault.path(), ROUTE);
}
