use crate::{Json, JsonErrorKind, Valor};
use std::collections::BTreeMap;

fn object(fields: impl IntoIterator<Item = (&'static str, Valor)>) -> Valor {
    Valor::Tabula(
        fields
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

#[test]
fn accepts_object_root_with_nested_json_values() {
    let value = object([
        (
            "items",
            Valor::Lista(vec![
                object([("ok", Valor::Bivalens(true))]),
                Valor::Nihil,
                Valor::Fractus(1.5),
            ]),
        ),
        ("name", Valor::Textus("salve".into())),
    ]);

    let json = Json::try_from(value.clone()).expect("valid json");
    assert_eq!(json.as_valor(), &value);
    assert_eq!(Valor::from(json.clone()), value);
    assert_eq!(
        json.to_wire(),
        r#"{"items":[{"ok":true},null,1.5],"name":"salve"}"#
    );
}

#[test]
fn rejects_non_object_roots() {
    let err = Json::try_from(Valor::Lista(vec![])).expect_err("array root");
    assert_eq!(err.path(), "$");
    assert_eq!(err.kind(), &JsonErrorKind::ExpectedObjectRoot);
}

#[test]
fn rejects_non_json_variants_with_paths() {
    let err = Json::try_from(object([(
        "items",
        Valor::Lista(vec![object([("payload", Valor::Octeti(vec![1]))])]),
    )]))
    .expect_err("octeti");

    assert_eq!(err.path(), "$.items[0].payload");
    assert_eq!(err.kind(), &JsonErrorKind::UnsupportedVariant("octeti"));

    let err = Json::try_from(object([(
        "at",
        Valor::Instans("1979-05-27T07:32:00Z".into()),
    )]))
    .expect_err("instans");
    assert_eq!(err.path(), "$.at");
    assert_eq!(err.kind(), &JsonErrorKind::UnsupportedVariant("instans"));
}

#[test]
fn rejects_non_finite_numbers_with_paths() {
    let err = Json::try_from(object([("n", Valor::Fractus(f64::NAN))])).expect_err("nan");
    assert_eq!(err.path(), "$.n");
    assert_eq!(err.kind(), &JsonErrorKind::NonFiniteNumber);
}

#[test]
fn parses_strict_wire_json_and_preserves_number_shape() {
    let json = Json::parse(
        r#"{
            "int": 9223372036854775807,
            "frac": -1.25e2,
            "unicode": "\uD834\uDD1E",
            "nested": [null, false, {"x": "y"}]
        }"#,
    )
    .expect("parse");

    let fields = json.as_object();
    assert_eq!(fields.get("int"), Some(&Valor::Numerus(i64::MAX)));
    assert_eq!(fields.get("frac"), Some(&Valor::Fractus(-125.0)));
    assert_eq!(fields.get("unicode"), Some(&Valor::Textus("𝄞".into())));
}

#[test]
fn rejects_duplicate_keys_before_btreemap_erases_them() {
    let err = Json::parse(r#"{"id": 1, "id": 2}"#).expect_err("duplicate");
    assert_eq!(err.path(), "$.id");
    assert_eq!(err.kind(), &JsonErrorKind::DuplicateKey("id".into()));
}

#[test]
fn rejects_array_and_scalar_wire_roots() {
    assert_eq!(
        Json::parse(r#"[{"ok": true}]"#)
            .expect_err("array root")
            .kind(),
        &JsonErrorKind::ExpectedObjectRoot
    );
    assert_eq!(
        Json::parse(r#""text""#).expect_err("scalar root").kind(),
        &JsonErrorKind::ExpectedObjectRoot
    );
}

#[test]
fn rejects_invalid_number_forms_and_ranges() {
    assert!(matches!(
        Json::parse(r#"{"n": 01}"#)
            .expect_err("leading zero")
            .kind(),
        JsonErrorKind::InvalidNumber(_)
    ));
    assert!(matches!(
        Json::parse(r#"{"n": 9223372036854775808}"#)
            .expect_err("large integer")
            .kind(),
        JsonErrorKind::InvalidNumber(_)
    ));
    assert!(matches!(
        Json::parse(r#"{"n": 1e999}"#)
            .expect_err("non finite exponent")
            .kind(),
        JsonErrorKind::InvalidNumber(_)
    ));
}

#[test]
fn invalid_number_reports_nested_parse_path() {
    let err = Json::parse(r#"{"a":{"n":01}}"#).expect_err("nested bad number");
    assert_eq!(err.path(), "$.a.n");
    assert!(matches!(err.kind(), JsonErrorKind::InvalidNumber(_)));
}

#[test]
fn renders_compact_deterministic_json() {
    let mut fields = BTreeMap::new();
    fields.insert("b".to_owned(), Valor::Textus("line\nbreak".into()));
    fields.insert("a".to_owned(), Valor::Numerus(1));
    let json = Json::from_object(fields).expect("json");

    assert_eq!(json.to_wire(), r#"{"a":1,"b":"line\nbreak"}"#);
}

#[test]
fn accepts_empty_root_object() {
    let value = object([]);
    let json = Json::try_from(value.clone()).expect("empty object");
    assert_eq!(json.as_valor(), &value);
    assert_eq!(json.to_wire(), "{}");
}

#[test]
fn rejects_non_finite_float_in_nested_position() {
    let err = Json::try_from(object([(
        "nested",
        Valor::Lista(vec![object([("val", Valor::Fractus(f64::NEG_INFINITY))])]),
    )]))
    .expect_err("neg infinity");
    assert_eq!(err.path(), "$.nested[0].val");
    assert_eq!(err.kind(), &JsonErrorKind::NonFiniteNumber);
}
