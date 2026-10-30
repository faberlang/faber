//! The conversation core over the shared native leaves: clock replies, a unit
//! reply, a leaf fault reduced to kind and path, a route no leaf owns, and the
//! C handle table's `open` export.

use super::{
    __faber_clif_v1_sermo_drain, __faber_clif_v1_sermo_failed, __faber_clif_v1_sermo_fault_kind,
    __faber_clif_v1_sermo_fault_path, __faber_clif_v1_sermo_materialize_i64,
    __faber_clif_v1_sermo_open, __faber_clif_v1_sermo_set_opener_i64,
    __faber_clif_v1_sermo_set_opener_nil, __faber_clif_v1_sermo_set_opener_valor, CONVERSATIONS,
    Conversation, Opener, slot,
};
use faber_leaf::LeafErrorKind;

fn open(route: &str) -> i64 {
    unsafe {
        __faber_clif_v1_sermo_open(
            crate::test_arena_ptr(),
            route.as_ptr(),
            i64::try_from(route.len()).unwrap(),
        )
    }
}

/// The bytes one fault-field export reports for `handle`.
///
/// # Safety
///
/// The export must write its byte count through the pointer it is given.
unsafe fn raw_field(
    export: unsafe extern "C" fn(*const crate::Arena, i64, *mut i64) -> *const u8,
    handle: i64,
) -> Vec<u8> {
    let mut length = -1_i64;
    let bytes = unsafe { export(crate::test_arena_ptr(), handle, &raw mut length) };
    assert!(!bytes.is_null(), "a raw fault field is never null");
    let length = usize::try_from(length).expect("the export writes a byte count");
    unsafe { std::slice::from_raw_parts(bytes, length) }.to_vec()
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
fn set_opener_i64_reopens_the_conversation() {
    const ROUTE: &str = "tempus:dormiet";

    // An unknown handle is a no-op, not a panic.
    __faber_clif_v1_sermo_set_opener_nil(crate::test_arena_ptr(), 0);
    __faber_clif_v1_sermo_set_opener_i64(crate::test_arena_ptr(), i64::MAX, 1);

    let handle = open(ROUTE);
    assert!(handle > 0, "open returned {handle}");
    __faber_clif_v1_sermo_set_opener_i64(crate::test_arena_ptr(), handle, 0);

    let mut table = CONVERSATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let opened = slot(&mut table, handle).expect("the handle names a slot");
    assert_eq!(
        opened.conversation,
        Conversation::open(ROUTE).with_opener(&[Opener::Int(0)])
    );
    assert!(opened.fault.is_none());
}

#[test]
fn valor_opener_reopens_a_numerus_conversation() {
    const ROUTE: &str = "tempus:dormiet";

    let handle = open(ROUTE);
    assert!(handle > 0, "open returned {handle}");
    let boxed = crate::__faber_clif_v1_opt_some(crate::test_arena_ptr(), crate::TAG_INT, 7);
    unsafe { __faber_clif_v1_sermo_set_opener_valor(crate::test_arena_ptr(), handle, boxed) };

    let mut table = CONVERSATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let opened = slot(&mut table, handle).expect("the handle names a slot");
    assert_eq!(opened.conversation.opener, [faber_leaf::Arg::Int(7)]);
}

#[test]
fn a_null_valor_opener_is_nihil() {
    const ROUTE: &str = "tempus:dormiet";

    let handle = open(ROUTE);
    assert!(handle > 0, "open returned {handle}");
    unsafe {
        __faber_clif_v1_sermo_set_opener_valor(crate::test_arena_ptr(), handle, std::ptr::null())
    };

    let mut table = CONVERSATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let opened = slot(&mut table, handle).expect("the handle names a slot");
    assert_eq!(opened.conversation.opener, [faber_leaf::Arg::Nil]);
}

#[test]
fn an_unsupported_tag_reopens_on_nihil() {
    const ROUTE: &str = "tempus:dormiet";

    let handle = open(ROUTE);
    assert!(handle > 0, "open returned {handle}");
    let boxed = crate::__faber_clif_v1_opt_some(crate::test_arena_ptr(), crate::TAG_FLOAT, 0);
    unsafe { __faber_clif_v1_sermo_set_opener_valor(crate::test_arena_ptr(), handle, boxed) };

    let mut table = CONVERSATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let opened = slot(&mut table, handle).expect("the handle names a slot");
    assert_eq!(opened.conversation.opener, [faber_leaf::Arg::Nil]);
}

#[test]
fn set_opener_nil_reopens_the_conversation() {
    const ROUTE: &str = "tempus:dormiet";

    let handle = open(ROUTE);
    assert!(handle > 0, "open returned {handle}");
    __faber_clif_v1_sermo_set_opener_nil(crate::test_arena_ptr(), handle);

    let mut table = CONVERSATIONS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let opened = slot(&mut table, handle).expect("the handle still names a slot");
    assert_eq!(
        opened.conversation,
        Conversation::open(ROUTE).with_opener(&[Opener::Nil])
    );
    assert!(opened.fault.is_none());
}

#[test]
fn monotonic_materializes_through_the_exports() {
    const ROUTE: &str = "tempus:monotonicum";

    let handle = open(ROUTE);
    assert!(handle > 0, "open returned {handle}");
    // The clock is process-relative: its origin is set by the first read in the
    // process, so a first reading is 0 and only a later one must not go back.
    let nanos = __faber_clif_v1_sermo_materialize_i64(crate::test_arena_ptr(), handle, 0);
    assert!(nanos >= 0, "{ROUTE} materialized {nanos}");
    let again = __faber_clif_v1_sermo_materialize_i64(crate::test_arena_ptr(), open(ROUTE), 0);
    assert!(again >= nanos, "{ROUTE} went back: {nanos} then {again}");
    assert_eq!(
        __faber_clif_v1_sermo_failed(crate::test_arena_ptr(), handle),
        0
    );
}

#[test]
fn invalid_handle_materializes_the_fallback() {
    assert_eq!(
        __faber_clif_v1_sermo_materialize_i64(crate::test_arena_ptr(), 0, 7),
        7
    );
    assert_eq!(__faber_clif_v1_sermo_failed(crate::test_arena_ptr(), 0), 1);
}

#[test]
fn wrong_reply_shape_faults_and_crosses_kind_and_path_raw() {
    const ROUTE: &str = "tempus:dormiet";

    let handle = open(ROUTE);
    assert!(handle > 0, "open returned {handle}");
    // The route replies `nihil`, so asking for a number faults.
    __faber_clif_v1_sermo_set_opener_i64(crate::test_arena_ptr(), handle, 0);
    assert_eq!(
        __faber_clif_v1_sermo_materialize_i64(crate::test_arena_ptr(), handle, 7),
        7
    );
    assert_eq!(
        __faber_clif_v1_sermo_failed(crate::test_arena_ptr(), handle),
        1
    );

    assert_eq!(
        unsafe { raw_field(__faber_clif_v1_sermo_fault_kind, handle) },
        LeafErrorKind::InvalidArgument.variant_name().as_bytes()
    );
    assert_eq!(
        unsafe { raw_field(__faber_clif_v1_sermo_fault_path, handle) },
        ROUTE.as_bytes()
    );
}

#[test]
fn no_fault_reports_an_empty_kind_and_path() {
    const ROUTE: &str = "tempus:nunc";

    let handle = open(ROUTE);
    assert!(handle > 0, "open returned {handle}");
    let nanos = __faber_clif_v1_sermo_materialize_i64(crate::test_arena_ptr(), handle, 0);
    assert!(nanos > 0, "{ROUTE} materialized {nanos}");
    assert_eq!(
        __faber_clif_v1_sermo_failed(crate::test_arena_ptr(), handle),
        0
    );

    assert!(unsafe { raw_field(__faber_clif_v1_sermo_fault_kind, handle) }.is_empty());
    assert!(unsafe { raw_field(__faber_clif_v1_sermo_fault_path, handle) }.is_empty());
}

#[test]
fn unsupported_route_opens_no_handle() {
    assert_eq!(open("tempus:ignotum"), 0);
}

#[test]
fn clock_routes_reply_with_an_integer() {
    for route in ["tempus:nunc", "tempus:monotonicum", "tempus:activum"] {
        let mut conversation = Conversation::open(route);
        let nanos = conversation.int().expect("a clock route replies numerus");
        assert!(nanos >= 0, "{route} replied {nanos}");
    }
}

#[test]
fn drain_serves_a_unit_route_and_clears_the_fault() {
    const ROUTE: &str = "tempus:dormiet";

    let handle = open(ROUTE);
    assert!(handle > 0, "open returned {handle}");
    __faber_clif_v1_sermo_set_opener_i64(crate::test_arena_ptr(), handle, 0);
    __faber_clif_v1_sermo_drain(crate::test_arena_ptr(), handle);
    assert_eq!(
        __faber_clif_v1_sermo_failed(crate::test_arena_ptr(), handle),
        0
    );
}

fn assert_fault_exports_match_leaf(handle: i64, error: &faber_leaf::LeafError) {
    let (kind, path) = error.facts();
    assert_eq!(
        unsafe { raw_field(__faber_clif_v1_sermo_fault_kind, handle) },
        kind.as_bytes()
    );
    assert_eq!(
        unsafe { raw_field(__faber_clif_v1_sermo_fault_path, handle) },
        path.as_bytes()
    );
}

#[test]
fn drain_preserves_the_shared_empty_path_fault_facts() {
    const ROUTE: &str = "tempus:dormiet";

    let leaf_error = faber_leaf::serve(ROUTE, &[faber_leaf::Arg::Int(-1)])
        .expect("clock route")
        .expect_err("negative duration");
    assert_eq!(leaf_error.kind(), LeafErrorKind::InvalidArgument);
    assert!(leaf_error.path().is_empty());

    let handle = open(ROUTE);
    assert!(handle > 0, "open returned {handle}");
    __faber_clif_v1_sermo_set_opener_i64(crate::test_arena_ptr(), handle, -1);
    __faber_clif_v1_sermo_drain(crate::test_arena_ptr(), handle);
    assert_eq!(
        __faber_clif_v1_sermo_failed(crate::test_arena_ptr(), handle),
        1
    );
    assert_fault_exports_match_leaf(handle, &leaf_error);
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
fn c_fault_exports_preserve_the_shared_nonempty_path_facts() {
    const ROUTE: &str = "solum:lege";
    const PATH: &str = "/nonexistent/faber-clif-sermo/cross-lane-fault-path";

    let leaf_error = faber_leaf::serve(ROUTE, &[faber_leaf::Arg::Str(PATH.to_owned())])
        .expect("filesystem route")
        .expect_err("missing path");
    assert_eq!(leaf_error.kind(), LeafErrorKind::NotFound);
    assert_eq!(leaf_error.path(), PATH);

    let handle = open(ROUTE);
    assert!(handle > 0, "open returned {handle}");
    let text = crate::text_new(crate::test_arena_ptr(), PATH.as_bytes());
    let text_bits = i64::try_from(text as usize).expect("text pointer fits ABI payload");
    let boxed =
        crate::__faber_clif_v1_opt_some(crate::test_arena_ptr(), crate::TAG_TEXT, text_bits);
    unsafe { __faber_clif_v1_sermo_set_opener_valor(crate::test_arena_ptr(), handle, boxed) };
    __faber_clif_v1_sermo_drain(crate::test_arena_ptr(), handle);
    assert_eq!(
        __faber_clif_v1_sermo_failed(crate::test_arena_ptr(), handle),
        1
    );
    assert_fault_exports_match_leaf(handle, &leaf_error);
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
