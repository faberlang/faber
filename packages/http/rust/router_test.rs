use super::*;
use faber::Valor;
use std::collections::BTreeMap;

fn text(s: &str) -> Valor {
    Valor::Textus(s.to_owned())
}

fn step(method: &str, path: &str, handler: &str) -> Valor {
    Valor::Tabula(BTreeMap::from([
        (KEY_METHOD.to_owned(), text(method)),
        (KEY_PATH.to_owned(), text(path)),
        (KEY_HANDLER.to_owned(), text(handler)),
    ]))
}

#[test]
fn static_get_and_post_dispatch() {
    let mut table = route_table();
    table = add_get(table, "/salve".into(), "greet".into()).expect("get");
    table = add_post(table, "/echo".into(), "echo".into()).expect("post");

    let hit = match_route(table.clone(), "GET".into(), "/salve".into())
        .expect("ok")
        .expect("match");
    let Valor::Tabula(fields) = hit else {
        panic!("tabula");
    };
    assert_eq!(fields.get(KEY_HANDLER), Some(&text("greet")));

    let miss = match_route(table, "GET".into(), "/echo".into()).expect("ok");
    assert!(miss.is_none(), "GET must not hit POST-only path");
}

#[test]
fn dynamic_path_params() {
    let table = add_get(route_table(), "/users/{id}".into(), "show".into()).expect("route");
    let hit = match_route(table, "get".into(), "/users/42".into())
        .expect("ok")
        .expect("match");
    assert_eq!(path_param(hit, "id".into()).as_deref(), Some("42"));
}

#[test]
fn dynamic_path_params_decode_multibyte_utf8() {
    let table = add_get(route_table(), "/users/{id}".into(), "show".into()).expect("route");
    let hit = match_route(table, "GET".into(), "/users/%E2%9C%93".into())
        .expect("ok")
        .expect("match");
    assert_eq!(path_param(hit, "id".into()).as_deref(), Some("\u{2713}"));
}

#[test]
fn duplicate_route_rejected() {
    let table = add_get(route_table(), "/x".into(), "a".into()).expect("first");
    let err = add_get(table, "/x".into(), "b".into()).expect_err("duplicate");
    assert!(err.contains("duplicate"), "{err}");
}

#[test]
fn ambiguous_static_and_dynamic_rejected_at_match() {
    let mut table = route_table();
    table = add_get(table, "/users/me".into(), "me".into()).expect("static");
    table = add_get(table, "/users/{id}".into(), "dyn".into()).expect("dynamic");
    let err = match_route(table, "GET".into(), "/users/me".into()).expect_err("ambiguous");
    assert!(err.contains("ambiguous"), "{err}");
}

#[test]
fn group_prefix_joins_relative_paths() {
    let table = add_group(
        route_table(),
        "/api".into(),
        vec![
            step("GET", "/items", "list"),
            step("GET", "/items/{id}", "item"),
        ],
    )
    .expect("group");
    let hit = match_route(table, "GET".into(), "/api/items/9".into())
        .expect("ok")
        .expect("match");
    assert_eq!(path_param(hit, "id".into()).as_deref(), Some("9"));
}

#[test]
fn middleware_order_preserved_on_match() {
    let mut table = route_table();
    table = add_middleware(table, "auth".into()).expect("auth");
    table = add_middleware(table, "log".into()).expect("log");
    table = add_get(table, "/".into(), "root".into()).expect("route");
    let hit = match_route(table, "GET".into(), "/".into())
        .expect("ok")
        .expect("match");
    let Valor::Tabula(fields) = hit else {
        panic!("tabula");
    };
    let Some(Valor::Lista(mw)) = fields.get(KEY_MIDDLEWARE) else {
        panic!("middleware lista");
    };
    assert_eq!(mw, &vec![text("auth"), text("log")]);
}

#[test]
fn duplicate_middleware_rejected() {
    let table = add_middleware(route_table(), "auth".into()).expect("first");
    let err = add_middleware(table, "auth".into()).expect_err("dup");
    assert!(err.contains("duplicate"), "{err}");
}

#[test]
fn query_and_header_extraction() {
    assert_eq!(
        query_param("a=1&name=Ada+Lovelace".into(), "name".into()).as_deref(),
        Some("Ada Lovelace")
    );
    assert_eq!(
        query_param("mark=%E2%9C%93".into(), "mark".into()).as_deref(),
        Some("\u{2713}")
    );
    let headers = Valor::Tabula(BTreeMap::from([
        ("Content-Type".to_owned(), text("application/json")),
        ("x-request-id".to_owned(), text("r1")),
    ]));
    assert_eq!(
        header_value(headers, "content-type".into()).as_deref(),
        Some("application/json")
    );
}

#[test]
fn json_body_object_and_reject_array() {
    let ok = json_body(r#"{"n":1}"#.into()).expect("object");
    let Valor::Tabula(fields) = ok else {
        panic!("tabula");
    };
    assert_eq!(fields.get("n"), Some(&Valor::Numerus(1)));
    let err = json_body("[1]".into()).expect_err("array root");
    assert!(!err.is_empty());
}

#[test]
fn error_and_success_response_shapes() {
    let err = error_response(404, "missing".into());
    let Valor::Tabula(fields) = err else {
        panic!("tabula");
    };
    assert_eq!(fields.get("status"), Some(&Valor::Numerus(404)));
    assert_eq!(fields.get("error"), Some(&Valor::Bivalens(true)));

    let ok = to_response(200, "ok".into());
    let Valor::Tabula(fields) = ok else {
        panic!("tabula");
    };
    assert_eq!(fields.get("error"), Some(&Valor::Bivalens(false)));
}

#[test]
fn no_match_returns_nihil() {
    let table = add_get(route_table(), "/only".into(), "h".into()).expect("route");
    assert!(match_route(table, "GET".into(), "/other".into())
        .expect("ok")
        .is_none());
}
