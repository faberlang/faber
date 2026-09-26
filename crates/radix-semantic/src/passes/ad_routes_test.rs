//! A3/U3: `@ ad` handler legality and per-file route collection.

use super::{ad_route_is_reserved, ad_route_shape_is_valid, route_exports};
use crate::file_interface::{InterfaceTypeSnapshot, RouteHandlerKind};
use crate::test_support::{analyze_ok, assert_rejects_with_issue};
use radix_types::Primitive;

#[test]
fn zero_and_one_parameter_handlers_are_legal() {
    analyze_ok("@ ad 'clock:now'\nfunctio nunc() → numerus { redde 1 }");
    analyze_ok("@ ad 'orders/list'\nfunctio liste(numerus n) → numerus { redde n }");
}

#[test]
fn private_handler_may_shadow_a_host_route() {
    analyze_ok("@ privata\n@ ad 'solum:lege'\nfunctio lege(textus via) → textus { redde via }");
}

#[test]
fn two_parameters_are_rejected() {
    assert_rejects_with_issue(
        "@ ad 'x:y'\nfunctio f(numerus a, numerus b) → numerus { redde a }",
        "ad_handler_arity",
    );
}

#[test]
fn generic_handler_is_rejected() {
    assert_rejects_with_issue(
        "@ ad 'x:y'\nfunctio f<T>(T a) → T { redde a }",
        "ad_handler_generic",
    );
}

#[test]
fn kernel_handler_is_rejected() {
    assert_rejects_with_issue(
        "@ nucleum\n@ ad 'x:y'\nfunctio f() { }",
        "ad_handler_kernel",
    );
}

#[test]
fn second_route_on_one_handler_is_rejected() {
    assert_rejects_with_issue(
        "@ ad 'x:y'\n@ ad 'x:z'\nfunctio f() { }",
        "ad_handler_multiple_routes",
    );
}

#[test]
fn method_handler_is_rejected() {
    assert_rejects_with_issue(
        "genus G {\n    @ ad 'x:y'\n    functio m() { }\n}",
        "ad_handler_not_top_level_functio",
    );
}

#[test]
fn malformed_route_is_rejected() {
    assert_rejects_with_issue("@ ad 'plain'\nfunctio f() { }", "ad_route_shape");
    assert_rejects_with_issue("@ ad ':name'\nfunctio f() { }", "ad_route_shape");
    assert_rejects_with_issue("@ ad 'x: y'\nfunctio f() { }", "ad_route_shape");
}

#[test]
fn reserved_routes_are_rejected() {
    assert_rejects_with_issue("@ ad 'runtime:echo'\nfunctio f() { }", "ad_route_reserved");
    assert_rejects_with_issue("@ ad 'runtime:other'\nfunctio f() { }", "ad_route_reserved");
    assert_rejects_with_issue("@ ad 'processus:exi'\nfunctio f() { }", "ad_route_reserved");
}

#[test]
fn duplicate_route_in_one_file_is_rejected() {
    assert_rejects_with_issue(
        "@ ad 'x:y'\nfunctio f() { }\n@ ad 'x:y'\nfunctio g() { }",
        "ad_route_duplicate",
    );
}

#[test]
fn route_shape_and_reservation_rules() {
    assert!(ad_route_shape_is_valid("solum:lege"));
    assert!(ad_route_shape_is_valid("clock/now"));
    assert!(!ad_route_shape_is_valid(""));
    assert!(!ad_route_shape_is_valid("name:"));
    assert!(ad_route_is_reserved("runtime:x"));
    assert!(!ad_route_is_reserved("runtimes:x"));
}

#[test]
fn route_exports_record_handler_shape() {
    let fixture = analyze_ok(
        "@ ad 'clock:now'\nfunctio nunc() → numerus { redde 1 }\n\
         @ ad 'orders:list'\nfunctio liste(textus f) fiunt → numerus { cede 1 }\n\
         @ ad 'log:write'\nfunctio scribe(textus s) ⇥ textus { }\n\
         functio plain() { }",
    );
    let result = &fixture.result;
    let routes = route_exports(
        result.hir.as_ref().expect("hir"),
        &result.types,
        &fixture.interner,
        &result.resolver,
        None,
    )
    .expect("route snapshots");
    assert_eq!(
        routes.keys().collect::<Vec<_>>(),
        ["clock:now", "log:write", "orders:list"]
    );

    let now = &routes["clock:now"];
    assert_eq!(now.handler, "nunc");
    assert_eq!(now.opener, None);
    assert!(matches!(
        now.reply,
        Some(InterfaceTypeSnapshot::Primitive(Primitive::Numerus { .. }))
    ));
    assert_eq!(now.kind, RouteHandlerKind::Sync);
    assert!(!now.failable);

    let list = &routes["orders:list"];
    assert!(matches!(
        list.opener,
        Some(InterfaceTypeSnapshot::Primitive(Primitive::Textus))
    ));
    assert_eq!(list.kind, RouteHandlerKind::Cursor);

    let write = &routes["log:write"];
    assert_eq!(write.reply, None);
    assert!(write.failable);
}

#[test]
fn route_handler_is_not_an_unused_function() {
    let fixture = analyze_ok("@ ad 'clock:now'\nfunctio nunc() → numerus { redde 1 }");
    assert!(
        !fixture
            .result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("unused_function")),
        "{:?}",
        fixture.result.diagnostics
    );
}
