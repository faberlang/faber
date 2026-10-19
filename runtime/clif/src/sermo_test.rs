//! The conversation core over the shared native leaves: clock replies, a unit
//! reply, a leaf fault reduced to kind and path, a route no leaf owns, and the
//! C handle table's `open` export.

use super::{__faber_clif_v1_sermo_open, CONVERSATIONS, Conversation, Opener, slot};
use faber_leaf::LeafErrorKind;

fn open(route: &str) -> i64 {
    unsafe { __faber_clif_v1_sermo_open(route.as_ptr(), i64::try_from(route.len()).unwrap()) }
}

#[test]
fn monotonic_route_opens_a_positive_handle() {
    const ROUTE: &str = "tempus:monotonicum";

    let handle = open(ROUTE);
    assert!(handle > 0, "open returned {handle}");

    let mut table = CONVERSATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let opened = slot(&mut table, handle).expect("the handle names a slot");
    assert_eq!(opened.conversation.route(), ROUTE);
}

#[test]
fn unsupported_route_opens_no_handle() {
    assert_eq!(open("tempus:ignotum"), 0);
}

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
