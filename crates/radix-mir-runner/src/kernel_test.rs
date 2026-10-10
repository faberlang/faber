//! Integration tests for MIR runner kernel dispatch.
//!
//! These tests prove that Processus, JSON, Solum, Aleator, and
//! Consolum kernel modules execute correctly through `dispatch_kernel`
//! against a `BufferHost`. They cover subprocess management, JSON
//! round-trips, filesystem operations, random number seeding, and
//! console output.

use super::kernel::dispatch_kernel;
use super::{BufferHost, MapKey, Value};
use radix_lexer::Span;
use radix_mir::KernelModule;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

#[test]
fn kernel_processus_argv_returns_host_args() {
    let mut host = BufferHost::with_argumenta(vec!["alpha".to_owned(), "beta".to_owned()]);
    let value = dispatch_kernel(
        &mut host,
        KernelModule::Processus,
        "argv",
        &[],
        Span::default(),
    )
    .expect("argv");
    let Value::Array(items) = value else {
        panic!("expected array");
    };
    let items = items.borrow();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0], Value::Str("alpha".to_owned()));
}

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

#[test]
fn kernel_processus_spawn_waits_and_returns_status() {
    let mut host = BufferHost::default();
    let argv = Value::Array(Rc::new(RefCell::new(vec![
        Value::Str("sh".to_owned()),
        Value::Str("-c".to_owned()),
        Value::Str("exit 3".to_owned()),
    ])));
    let value = dispatch_kernel(
        &mut host,
        KernelModule::Processus,
        "spawn",
        &[argv, Value::Nil, Value::Nil],
        Span::default(),
    )
    .expect("processus spawn");
    let Value::Map(fields) = value else {
        panic!("expected subprocess status map");
    };
    let fields = fields.borrow();
    assert!(
        matches!(fields.get(&MapKey::Str("pid".to_owned())), Some(Value::Int(pid)) if *pid > 0)
    );
    assert_eq!(
        fields.get(&MapKey::Str("status".to_owned())),
        Some(&Value::Int(3))
    );
}

#[test]
fn kernel_processus_captura_returns_status_stdout_and_stderr() {
    let mut host = BufferHost::default();
    let argv = Value::Array(Rc::new(RefCell::new(vec![
        Value::Str("sh".to_owned()),
        Value::Str("-c".to_owned()),
        Value::Str("printf out; printf err >&2; exit 7".to_owned()),
    ])));
    let value = dispatch_kernel(
        &mut host,
        KernelModule::Processus,
        "captura",
        &[argv],
        Span::default(),
    )
    .expect("processus captura");
    let Value::Map(fields) = value else {
        panic!("expected captured process map");
    };
    let fields = fields.borrow();
    assert_eq!(
        fields.get(&MapKey::Str("status".to_owned())),
        Some(&Value::Int(7))
    );
    assert_eq!(
        fields.get(&MapKey::Str("stdout".to_owned())),
        Some(&Value::Str("out".to_owned()))
    );
    assert_eq!(
        fields.get(&MapKey::Str("stderr".to_owned())),
        Some(&Value::Str("err".to_owned()))
    );
}

fn solum_fixture(label: &str) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::Builder::new()
        .prefix(&format!("kernel-{label}-"))
        .tempdir()
        .expect("create solum fixture dir");
    let nested = root.path().join("nested");
    std::fs::create_dir_all(&nested).expect("create solum fixture dir");
    (root, nested)
}

fn solum_dispatch(host: &mut BufferHost, verb: &str, args: &[Value]) -> Value {
    dispatch_kernel(host, KernelModule::Solum, verb, args, Span::default())
        .unwrap_or_else(|e| panic!("solum {verb} failed: {e:?}"))
}

#[test]
fn kernel_solum_write_file_read_file_round_trips_text() {
    let (_root, nested) = solum_fixture("write_file-read_file");
    let path = nested.join("demo.txt");
    let text = path.to_string_lossy().into_owned();
    solum_dispatch(
        &mut BufferHost::default(),
        "write_file",
        &[
            Value::Str(text.clone()),
            Value::Str("alpha\nbeta".to_owned()),
        ],
    );

    assert_eq!(
        solum_dispatch(&mut BufferHost::default(), "read_file", &[Value::Str(text)]),
        Value::Str("alpha\nbeta".to_owned())
    );
}

#[test]
fn kernel_solum_read_bytes_reads_file_as_octeti() {
    let (_root, nested) = solum_fixture("read_bytes");
    let path = nested.join("demo.txt");
    let text = path.to_string_lossy().into_owned();
    solum_dispatch(
        &mut BufferHost::default(),
        "write_file",
        &[
            Value::Str(text.clone()),
            Value::Str("alpha\nbeta".to_owned()),
        ],
    );

    assert_eq!(
        solum_dispatch(
            &mut BufferHost::default(),
            "read_bytes",
            &[Value::Str(text)]
        ),
        Value::Octeti(b"alpha\nbeta".to_vec())
    );
}

#[test]
fn kernel_solum_write_bytes_writes_bytes() {
    let (_root, nested) = solum_fixture("write_bytes");
    let path = nested.join("bytes.bin");
    let text = path.to_string_lossy().into_owned();
    solum_dispatch(
        &mut BufferHost::default(),
        "write_bytes",
        &[Value::Str(text.clone()), Value::Octeti(vec![0, 1, 2, 255])],
    );

    assert_eq!(
        std::fs::read(&path).expect("read bytes file"),
        vec![0, 1, 2, 255]
    );
}

#[test]
fn kernel_solum_exists_returns_true_for_existing_file() {
    let (_root, nested) = solum_fixture("exists");
    let path = nested.join("exists.txt");
    let text = path.to_string_lossy().into_owned();
    solum_dispatch(
        &mut BufferHost::default(),
        "write_file",
        &[Value::Str(text.clone()), Value::Str("data".to_owned())],
    );

    assert_eq!(
        solum_dispatch(&mut BufferHost::default(), "exists", &[Value::Str(text)]),
        Value::Bool(true)
    );
}

#[test]
fn kernel_solum_is_regular_returns_true_for_file() {
    let (_root, nested) = solum_fixture("is_regular");
    let path = nested.join("regular.txt");
    let text = path.to_string_lossy().into_owned();
    solum_dispatch(
        &mut BufferHost::default(),
        "write_file",
        &[Value::Str(text.clone()), Value::Str("data".to_owned())],
    );

    assert_eq!(
        solum_dispatch(
            &mut BufferHost::default(),
            "is_regular",
            &[Value::Str(text)]
        ),
        Value::Bool(true)
    );
}

#[test]
fn kernel_solum_is_directory_returns_true_for_dir() {
    let (_root, nested) = solum_fixture("is_directory");
    let dir_text = nested.to_string_lossy().into_owned();

    assert_eq!(
        solum_dispatch(
            &mut BufferHost::default(),
            "is_directory",
            &[Value::Str(dir_text)]
        ),
        Value::Bool(true)
    );
}

#[test]
fn kernel_solum_byte_length_returns_byte_count() {
    let (_root, nested) = solum_fixture("byte_length");
    let path = nested.join("measured.txt");
    let text = path.to_string_lossy().into_owned();
    solum_dispatch(
        &mut BufferHost::default(),
        "write_file",
        &[
            Value::Str(text.clone()),
            Value::Str("alpha\nbeta".to_owned()),
        ],
    );

    assert_eq!(
        solum_dispatch(
            &mut BufferHost::default(),
            "byte_length",
            &[Value::Str(text)]
        ),
        Value::Int(10)
    );
}

#[test]
fn kernel_solum_stat_returns_map() {
    let (_root, nested) = solum_fixture("stat");
    let path = nested.join("described.txt");
    let text = path.to_string_lossy().into_owned();
    solum_dispatch(
        &mut BufferHost::default(),
        "write_file",
        &[Value::Str(text.clone()), Value::Str("data".to_owned())],
    );

    assert!(matches!(
        solum_dispatch(&mut BufferHost::default(), "stat", &[Value::Str(text)]),
        Value::Map(_)
    ));
}

#[test]
fn kernel_solum_copy_copies_file() {
    let (_root, nested) = solum_fixture("copy");
    let src = nested.join("source.txt");
    let dst = nested.join("copy.txt");
    let src_text = src.to_string_lossy().into_owned();
    let dst_text = dst.to_string_lossy().into_owned();
    solum_dispatch(
        &mut BufferHost::default(),
        "write_file",
        &[Value::Str(src_text), Value::Str("content".to_owned())],
    );

    solum_dispatch(
        &mut BufferHost::default(),
        "copy",
        &[
            Value::Str(src.to_string_lossy().into_owned()),
            Value::Str(dst_text.clone()),
        ],
    );

    assert_eq!(
        std::fs::read_to_string(&dst).expect("read copied file"),
        "content"
    );
}

#[test]
fn kernel_solum_rename_moves_file() {
    let (_root, nested) = solum_fixture("rename");
    let src = nested.join("old.txt");
    let dst = nested.join("new.txt");
    let src_text = src.to_string_lossy().into_owned();
    let dst_text = dst.to_string_lossy().into_owned();
    solum_dispatch(
        &mut BufferHost::default(),
        "write_file",
        &[Value::Str(src_text), Value::Str("movable".to_owned())],
    );

    solum_dispatch(
        &mut BufferHost::default(),
        "rename",
        &[
            Value::Str(src.to_string_lossy().into_owned()),
            Value::Str(dst_text),
        ],
    );

    assert_eq!(
        std::fs::read_to_string(&dst).expect("read moved file"),
        "movable"
    );
}

#[test]
fn kernel_solum_delete_removes_file() {
    let (_root, nested) = solum_fixture("delete");
    let path = nested.join("tobedeleted.txt");
    let text = path.to_string_lossy().into_owned();
    solum_dispatch(
        &mut BufferHost::default(),
        "write_file",
        &[Value::Str(text.clone()), Value::Str("delete me".to_owned())],
    );

    solum_dispatch(&mut BufferHost::default(), "delete", &[Value::Str(text)]);
    assert!(!path.exists());
}

#[test]
fn kernel_solum_touch_creates_file() {
    let (_root, nested) = solum_fixture("touch");
    let path = nested.join("touched.txt");
    let text = path.to_string_lossy().into_owned();

    solum_dispatch(
        &mut BufferHost::default(),
        "touch",
        &[Value::Str(text.clone())],
    );
    assert!(path.exists());
}

#[test]
fn kernel_solum_set_mode_mode_changes_and_reads_permissions() {
    let (_root, nested) = solum_fixture("chmod");
    let path = nested.join("permissions.txt");
    let text = path.to_string_lossy().into_owned();
    solum_dispatch(
        &mut BufferHost::default(),
        "write_file",
        &[Value::Str(text.clone()), Value::Str("data".to_owned())],
    );

    solum_dispatch(
        &mut BufferHost::default(),
        "set_mode",
        &[Value::Str(text.clone()), Value::Int(0o600)],
    );
    assert_eq!(
        solum_dispatch(&mut BufferHost::default(), "mode", &[Value::Str(text)]),
        Value::Int(0o600)
    );
}

#[test]
fn kernel_solum_read_link_reads_symlink_target() {
    let (_root, nested) = solum_fixture("read_link");
    let target = nested.join("target.txt");
    let link = nested.join("source-link");
    let target_text = target.to_string_lossy().into_owned();
    let link_text = link.to_string_lossy().into_owned();
    solum_dispatch(
        &mut BufferHost::default(),
        "write_file",
        &[
            Value::Str(target_text.clone()),
            Value::Str("data".to_owned()),
        ],
    );
    std::os::unix::fs::symlink(&target, &link).expect("create symlink");

    assert_eq!(
        solum_dispatch(
            &mut BufferHost::default(),
            "read_link",
            &[Value::Str(link_text)]
        ),
        Value::Str(target_text)
    );
}

#[test]
fn kernel_solum_create_link_is_symlink_creates_and_checks_hardlink() {
    let (_root, nested) = solum_fixture("hardlink");
    let src = nested.join("original.txt");
    let link = nested.join("hardlink.txt");
    let src_text = src.to_string_lossy().into_owned();
    let link_text = link.to_string_lossy().into_owned();
    solum_dispatch(
        &mut BufferHost::default(),
        "write_file",
        &[
            Value::Str(src_text.clone()),
            Value::Str("linked".to_owned()),
        ],
    );

    solum_dispatch(
        &mut BufferHost::default(),
        "create_link",
        &[Value::Str(src_text), Value::Str(link_text.clone())],
    );

    assert_eq!(
        solum_dispatch(
            &mut BufferHost::default(),
            "is_symlink",
            &[Value::Str(link_text)]
        ),
        Value::Bool(true)
    );
}

#[test]
fn kernel_solum_join_joins_path_parts() {
    let parts = Value::Array(Rc::new(RefCell::new(vec![
        Value::Str("alpha".to_owned()),
        Value::Str("beta.txt".to_owned()),
    ])));

    assert_eq!(
        solum_dispatch(&mut BufferHost::default(), "join", &[parts]),
        Value::Str(
            std::path::PathBuf::from("alpha")
                .join("beta.txt")
                .to_string_lossy()
                .into_owned()
        )
    );
}

#[test]
fn kernel_solum_home_returns_non_empty_string() {
    let home = solum_dispatch(&mut BufferHost::default(), "home", &[]);
    assert!(matches!(home, Value::Str(text) if !text.is_empty()));
}

#[test]
fn kernel_solum_temp_dir_returns_non_empty_string() {
    let temp = solum_dispatch(&mut BufferHost::default(), "temp_dir", &[]);
    assert!(matches!(temp, Value::Str(text) if !text.is_empty()));
}

#[test]
fn kernel_solum_remove_tree_removes_directory_tree() {
    let (root, _nested) = solum_fixture("remove_tree");
    let root_text = root.path().to_string_lossy().into_owned();

    solum_dispatch(
        &mut BufferHost::default(),
        "remove_tree",
        &[Value::Str(root_text)],
    );
    assert!(!root.path().exists());
}

#[test]
fn kernel_aleator_seed_is_deterministic_and_uuid_is_v4_shape() {
    let mut host = BufferHost::default();
    dispatch_kernel(
        &mut host,
        KernelModule::Aleator,
        "seed",
        &[Value::Int(42)],
        Span::default(),
    )
    .expect("seed");
    let first = dispatch_kernel(
        &mut host,
        KernelModule::Aleator,
        "random_int",
        &[Value::Int(1), Value::Int(100)],
        Span::default(),
    )
    .expect("random_int first");
    dispatch_kernel(
        &mut host,
        KernelModule::Aleator,
        "seed",
        &[Value::Int(42)],
        Span::default(),
    )
    .expect("seed");
    let second = dispatch_kernel(
        &mut host,
        KernelModule::Aleator,
        "random_int",
        &[Value::Int(1), Value::Int(100)],
        Span::default(),
    )
    .expect("random_int second");
    assert_eq!(first, second);
    let bytes = dispatch_kernel(
        &mut host,
        KernelModule::Aleator,
        "random_bytes",
        &[Value::Int(4)],
        Span::default(),
    )
    .expect("random_bytes");
    let Value::Octeti(bytes) = bytes else {
        panic!("expected octeti");
    };
    assert_eq!(bytes.len(), 4);
    let uuid = dispatch_kernel(
        &mut host,
        KernelModule::Aleator,
        "uuid",
        &[],
        Span::default(),
    )
    .expect("uuid");
    let Value::Str(uuid) = uuid else {
        panic!("expected uuid string");
    };
    assert_eq!(uuid.len(), 36);
    assert_eq!(uuid.as_bytes()[14], b'4');
}

#[test]
fn consolum_write_partial_writes_stdout_without_newline() {
    let mut host = BufferHost::default();
    dispatch_kernel(
        &mut host,
        KernelModule::Consolum,
        "write_partial",
        &[Value::Str("hello".to_owned())],
        Span::default(),
    )
    .expect("consolum.write_partial");
    assert_eq!(host.pending_stdout, "hello");
    assert!(host.stdout_lines.is_empty());

    dispatch_kernel(
        &mut host,
        KernelModule::Consolum,
        "write_partial",
        &[Value::Str(" world".to_owned())],
        Span::default(),
    )
    .expect("consolum.write_partial second");
    assert_eq!(host.pending_stdout, "hello world");

    dispatch_kernel(
        &mut host,
        KernelModule::Consolum,
        "write_line",
        &[Value::Str("!".to_owned())],
        Span::default(),
    )
    .expect("consolum.write_line");
    assert_eq!(host.stdout_lines, vec!["hello world!".to_owned()]);
    assert!(host.pending_stdout.is_empty());
}

fn consolum(
    host: &mut BufferHost,
    verb: &str,
    args: &[Value],
) -> Result<Value, super::RunnerError> {
    dispatch_kernel(host, KernelModule::Consolum, verb, args, Span::default())
}

#[test]
fn consolum_write_bytes_writes_stdout_without_newline() {
    let mut host = BufferHost::default();
    consolum(&mut host, "write_bytes", &[Value::Octeti(b"abc".to_vec())])
        .expect("consolum.write_bytes");
    assert_eq!(host.pending_stdout, "abc");
    assert!(host.stdout_lines.is_empty());
    assert!(host.stderr_lines.is_empty());

    let err = consolum(&mut host, "write_bytes", &[]).expect_err("arity");
    assert!(err.issue.contains("write_bytes_provider_arity"));
    let err = consolum(&mut host, "write_bytes", &[Value::Str("x".to_owned())]).expect_err("type");
    assert!(err.issue.contains("expects_octeti"));
}

#[test]
fn consolum_write_bytes_accepts_a_b8_list_and_a_solum_read_bytes_result() {
    let mut host = BufferHost::default();
    let list = Value::Array(Rc::new(RefCell::new(vec![
        Value::UInt(0x68),
        Value::UInt(0x69),
        Value::UInt(0x0a),
    ])));
    consolum(&mut host, "write_bytes", &[list]).expect("consolum.write_bytes list<b8>");
    assert_eq!(host.pending_stdout, "hi\n");

    let (_root, nested) = solum_fixture("consolum_read_bytes");
    let path = nested.join("bytes.bin");
    let text = path.to_string_lossy().into_owned();
    std::fs::write(&path, b"abc").expect("write fixture");
    let read = solum_dispatch(
        &mut BufferHost::default(),
        "read_bytes",
        &[Value::Str(text)],
    );
    assert_eq!(read, Value::Octeti(b"abc".to_vec()));
    consolum(&mut host, "write_bytes", &[read]).expect("consolum.write_bytes read_bytes result");
    assert_eq!(host.pending_stdout, "hi\nabc");

    let out_of_range = Value::Array(Rc::new(RefCell::new(vec![Value::UInt(256)])));
    let err = consolum(&mut host, "write_bytes", &[out_of_range]).expect_err("range");
    assert!(err.issue.contains("expects_octeti"));
}

#[test]
fn consolum_debug_line_writes_stderr() {
    let mut host = BufferHost::default();
    consolum(&mut host, "debug_line", &[Value::Str("trace".to_owned())])
        .expect("consolum.debug_line");
    assert_eq!(host.stderr_lines, vec!["trace".to_owned()]);
    assert!(host.stdout_lines.is_empty());
    assert!(host.pending_stdout.is_empty());

    let err = consolum(&mut host, "debug_line", &[]).expect_err("arity");
    assert!(err.issue.contains("debug_line_provider_arity"));
}

#[test]
fn consolum_read_line_reads_host_stdin() {
    let mut host = BufferHost::default().with_stdin_lines(["first", "second"]);
    let value = consolum(&mut host, "read_line", &[]).expect("consolum.read_line");
    assert_eq!(value, Value::Str("first".to_owned()));
    let value = consolum(&mut host, "read_line", &[]).expect("consolum.read_line second");
    assert_eq!(value, Value::Str("second".to_owned()));
    let eof = consolum(&mut host, "read_line", &[]).expect("consolum.read_line eof");
    assert_eq!(eof, Value::Str(String::new()));

    let err =
        consolum(&mut host, "read_line", &[Value::Str("extra".to_owned())]).expect_err("arity");
    assert!(err.issue.contains("read_line_provider_arity"));
}

#[test]
fn consolum_read_bytes_reads_bounded_stdin() {
    let mut host = BufferHost::default().with_stdin_bytes(b"abcdef".to_vec());
    let value = consolum(&mut host, "read_bytes", &[Value::Int(3)]).expect("read_bytes");
    assert_eq!(value, Value::Octeti(b"abc".to_vec()));
    let rest = consolum(&mut host, "read_bytes", &[Value::Int(8)]).expect("short read");
    assert_eq!(rest, Value::Octeti(b"def".to_vec()));
    let empty = consolum(&mut host, "read_bytes", &[Value::Int(4)]).expect("eof");
    assert_eq!(empty, Value::Octeti(Vec::new()));
    let zero = consolum(&mut host, "read_bytes", &[Value::Int(0)]).expect("zero");
    assert_eq!(zero, Value::Octeti(Vec::new()));
    let negative = consolum(&mut host, "read_bytes", &[Value::Int(-1)]).expect("negative");
    assert_eq!(negative, Value::Octeti(Vec::new()));

    let err = consolum(&mut host, "read_bytes", &[]).expect_err("arity");
    assert!(err.issue.contains("read_bytes_provider_arity"));
    let err = consolum(&mut host, "read_bytes", &[Value::Str("x".to_owned())]).expect_err("type");
    assert!(err.issue.contains("expects_numerus"));
    let err = consolum(&mut host, "read_bytes", &[Value::Int(1024 * 1024 + 1)]).expect_err("bound");
    assert!(err.issue.contains("must_be_at_most"));
}

#[test]
fn consolum_tty_probes_return_bool_and_reject_arity() {
    let mut host = BufferHost::default();
    assert_eq!(
        consolum(&mut host, "stdin_is_tty", &[]).expect("stdin_is_tty"),
        Value::Bool(false)
    );
    assert_eq!(
        consolum(&mut host, "stdout_is_tty", &[]).expect("stdout_is_tty"),
        Value::Bool(false)
    );
    assert_eq!(
        consolum(&mut host, "stderr_is_tty", &[]).expect("stderr_is_tty"),
        Value::Bool(false)
    );
    let err = consolum(&mut host, "stdin_is_tty", &[Value::Bool(true)]).expect_err("arity");
    assert!(err.issue.contains("stdin_is_tty_provider_arity"));
}

#[test]
fn consolum_unknown_verb_fails_closed() {
    let mut host = BufferHost::default();
    let err = consolum(&mut host, "write_bytes_async", &[]).expect_err("async");
    assert!(
        err.issue.contains("consolum_kernel_verb"),
        "unknown verb must stay fail-closed, got {}",
        err.issue
    );
    assert_eq!(
        err.issue,
        "runner_faberscript_unsupported_consolum_kernel_verb_write_bytes_async"
    );
    let err = consolum(&mut host, "not_a_verb", &[]).expect_err("unknown");
    assert_eq!(
        err.issue,
        "runner_faberscript_unsupported_consolum_kernel_verb_not_a_verb"
    );
}
