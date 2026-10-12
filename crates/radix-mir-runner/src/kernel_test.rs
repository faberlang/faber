//! Integration tests for remaining runner kernel dispatch.
//!
//! JSON and TOML stay runner kernels until Norma owns them. I/O kernels
//! (solum, consolum, processus, aleator) were deleted; those routes run
//! through `ad` / `faber-leaf`.

use super::kernel::dispatch_kernel;
use super::{BufferHost, MapKey, Value};
use radix_lexer::Span;
use radix_mir::KernelModule;
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn kernel_json_parse_parses_object() {
    let mut host = BufferHost::default();
    let raw = Value::Str("{\"ok\": true}".to_owned());
    let value = dispatch_kernel(
        &mut host,
        KernelModule::Json,
        "parse",
        &[raw],
        Span::default(),
    )
    .expect("json parse");
    assert!(matches!(value, Value::Map(_)));
}

#[test]
fn kernel_json_round_trips_and_try_parse_rejects_invalid_input() {
    let mut host = BufferHost::default();
    let raw = Value::Str("{\"items\":[1,true]}".to_owned());
    let parsed = dispatch_kernel(
        &mut host,
        KernelModule::Json,
        "parse",
        &[raw],
        Span::default(),
    )
    .expect("json parse");
    let rendered = dispatch_kernel(
        &mut host,
        KernelModule::Json,
        "stringify",
        &[parsed],
        Span::default(),
    )
    .expect("json stringify");
    assert_eq!(rendered, Value::Str("{\"items\":[1,true]}".to_owned()));

    let invalid = Value::Str("{".to_owned());
    let value = dispatch_kernel(
        &mut host,
        KernelModule::Json,
        "try_parse",
        &[invalid],
        Span::default(),
    )
    .expect("json try_parse");
    assert_eq!(value, Value::Nil);
}

#[test]
fn kernel_json_stringify_sorts_map_keys_deterministically() {
    let mut host = BufferHost::default();
    let mut fields = rustc_hash::FxHashMap::default();
    fields.insert(MapKey::Str("z".to_owned()), Value::Int(1));
    fields.insert(MapKey::Str("a".to_owned()), Value::Int(2));
    let value = Value::Map(Rc::new(RefCell::new(fields)));

    let rendered = dispatch_kernel(
        &mut host,
        KernelModule::Json,
        "stringify",
        &[value],
        Span::default(),
    )
    .expect("json stringify");

    assert_eq!(rendered, Value::Str(r#"{"a":2,"z":1}"#.to_owned()));
}

#[test]
fn kernel_toml_parse_parses_nested_table_and_array_of_tables() {
    let mut host = BufferHost::default();
    let raw = Value::Str(
        "[package]\nname = \"radix\"\n\n[package.metadata]\nfeatured = true\n\n[[bin]]\nname = \"radix\"\n\n[[bin]]\nname = \"faber\"\n"
            .to_owned(),
    );
    let parsed = dispatch_kernel(
        &mut host,
        KernelModule::Toml,
        "parse",
        &[raw],
        Span::default(),
    )
    .expect("toml parse");
    let Value::Map(root) = &parsed else {
        panic!("expected root map");
    };
    let root = root.borrow();
    let Value::Map(package) = root
        .get(&MapKey::Str("package".to_owned()))
        .expect("package table")
    else {
        panic!("expected package map");
    };
    let package = package.borrow();
    assert_eq!(
        package.get(&MapKey::Str("name".to_owned())),
        Some(&Value::Str("radix".to_owned()))
    );
    let Value::Map(metadata) = package
        .get(&MapKey::Str("metadata".to_owned()))
        .expect("metadata table")
    else {
        panic!("expected metadata map");
    };
    let metadata = metadata.borrow();
    assert_eq!(
        metadata.get(&MapKey::Str("featured".to_owned())),
        Some(&Value::Bool(true))
    );
    let Value::Array(bins) = root.get(&MapKey::Str("bin".to_owned())).expect("bin array") else {
        panic!("expected bin array");
    };
    let bins = bins.borrow();
    assert_eq!(bins.len(), 2);
}

#[test]
fn kernel_toml_datetime_lowers_to_instans_and_round_trips() {
    let mut host = BufferHost::default();
    let raw = Value::Str(
        "[pkg]\nreleased = 1979-05-27T07:32:00Z\nlocal = 1979-05-27T07:32:00\n".to_owned(),
    );
    let parsed = dispatch_kernel(
        &mut host,
        KernelModule::Toml,
        "parse",
        &[raw],
        Span::default(),
    )
    .expect("toml parse");
    let Value::Map(root) = &parsed else {
        panic!("expected root map");
    };
    let root = root.borrow();
    let Value::Map(pkg) = root.get(&MapKey::Str("pkg".to_owned())).expect("pkg table") else {
        panic!("expected pkg map");
    };
    let pkg = pkg.borrow();
    assert!(
        matches!(
            pkg.get(&MapKey::Str("released".to_owned())),
            Some(Value::Instans(_))
        ),
        "offset TOML datetime must lower to Instans"
    );
    assert!(
        matches!(
            pkg.get(&MapKey::Str("local".to_owned())),
            Some(Value::Instans(_))
        ),
        "local TOML datetime must lower to Instans (read as UTC)"
    );
    drop(pkg);
    drop(root);

    let rendered = dispatch_kernel(
        &mut host,
        KernelModule::Toml,
        "stringify",
        &[parsed],
        Span::default(),
    )
    .expect("toml stringify");
    let Value::Str(rendered) = rendered else {
        panic!("expected string");
    };
    assert!(rendered.contains("released = 1979-05-27T07:32:00Z"));
    assert!(rendered.contains("local = 1979-05-27T07:32:00Z"));
}

#[test]
fn kernel_toml_round_trips_nested_document() {
    let mut host = BufferHost::default();
    let source = "[owner]\nname = \"Tom\"\n\n[owner.details]\nage = 42\nactive = true\n\n[[products]]\nname = \"Hammer\"\n\n[[products]]\nname = \"Nail\"\n";
    let parsed = dispatch_kernel(
        &mut host,
        KernelModule::Toml,
        "parse",
        &[Value::Str(source.to_owned())],
        Span::default(),
    )
    .expect("toml parse");
    let rendered = dispatch_kernel(
        &mut host,
        KernelModule::Toml,
        "stringify",
        std::slice::from_ref(&parsed),
        Span::default(),
    )
    .expect("toml stringify");
    let Value::Str(rendered) = rendered else {
        panic!("expected string");
    };
    let reparsed = dispatch_kernel(
        &mut host,
        KernelModule::Toml,
        "parse",
        &[Value::Str(rendered)],
        Span::default(),
    )
    .expect("toml re-parse");
    assert_eq!(
        reparsed, parsed,
        "toml parse/stringify round-trip must preserve the valor tree"
    );
}

#[test]
fn kernel_toml_try_parse_rejects_invalid_input_and_parse_errors() {
    let mut host = BufferHost::default();
    let invalid = Value::Str("broken = [".to_owned());
    let value = dispatch_kernel(
        &mut host,
        KernelModule::Toml,
        "try_parse",
        &[invalid],
        Span::default(),
    )
    .expect("toml try_parse");
    assert_eq!(value, Value::Nil);

    let err = dispatch_kernel(
        &mut host,
        KernelModule::Toml,
        "parse",
        &[Value::Str("broken = [".to_owned())],
        Span::default(),
    )
    .expect_err("toml parse must error on invalid input");
    assert!(err.issue.contains("toml_parse_failed"));
}

#[test]
fn kernel_toml_stringify_rejects_non_map_root() {
    let mut host = BufferHost::default();
    let err = dispatch_kernel(
        &mut host,
        KernelModule::Toml,
        "stringify",
        &[Value::Str("scalar".to_owned())],
        Span::default(),
    )
    .expect_err("toml.stringify must reject a non-map root");
    assert!(err.issue.contains("toml_stringify_root_must_be_a_map"));
}
