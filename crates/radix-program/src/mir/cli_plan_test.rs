#[test]
fn generic_applied_arg_preserves_index_kind() {
    let mut types = radix_types::TypeTable::new();
    let base = types.intern(radix_types::Type::Param(radix_module::lexer::Symbol(10)));
    let size = types.intern_index(radix_types::IndexExpr::Literal(3));
    let applied = types.intern(radix_types::Type::Applied(
        base,
        vec![radix_types::AppliedArg::Size(size)],
    ));
    assert!(matches!(
        types.get(applied),
        radix_types::Type::Applied(_, args)
            if matches!(args.as_slice(), [radix_types::AppliedArg::Size(id)] if *id == size)
    ));
}

#[test]
fn import_semantic_type_preserves_nested_compound_shape() {
    let mut source = radix_types::TypeTable::new();
    let numerus = source.primitive(radix_types::Primitive::Numerus);
    let array = source.array(numerus);
    let tuple = source.tuple(vec![array, numerus]);
    let input = source.intervallum(tuple);

    let mut target = radix_types::TypeTable::new();
    let mut imported = std::collections::HashMap::new();
    let output =
        super::import_semantic_type_with_nominal(&source, &mut target, input, &mut imported, None);

    let numerus_target = target.primitive(radix_types::Primitive::Numerus);
    let array_target = target.ensure_array(numerus_target);
    let tuple_target = target.tuple(vec![array_target, numerus_target]);
    let intervallum_target = target.intervallum(tuple_target);
    assert_eq!(output, intervallum_target);

    // The second import of the same source id short-circuits through the cache.
    let again =
        super::import_semantic_type_with_nominal(&source, &mut target, input, &mut imported, None);
    assert_eq!(output, again);
}

// ── `--` ends option parsing (A-169) ───────────────────────────────────────
//
// The generated Rust parser and `runtime/llvm/src/cli.rs` both treat a bare
// `--` as end-of-options: it is dropped and everything after it is a
// positional. `faber run . -- ...` strips the first `--` itself, so the
// planner sees the second one.

fn run_cli_package(
    tag: &str,
    files: &[(&str, &str)],
    argumenta: &[&str],
) -> Result<Vec<String>, Vec<radix_module::diagnostics::Diagnostic>> {
    let dir = crate::test_support::test_temp_dir(tag);
    for (name, source) in files {
        std::fs::write(dir.join(name), source).expect("write package file");
    }
    let mut host = radix_mir_runner::BufferHost::with_argumenta(
        argumenta
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect(),
    );
    crate::mir::run_package_mir(
        &radix_module::driver::Config::default(),
        &dir,
        &crate::test_support::test_mir_seams(),
        &mut host,
    )?;
    Ok(host.stdout_lines)
}

/// Diagnostic identity for failure context: code, issue and named args.
fn diagnostic_identities(
    diagnostics: &[radix_module::diagnostics::Diagnostic],
) -> Vec<(
    Option<&'static str>,
    Option<&str>,
    Vec<(&'static str, &str)>,
)> {
    diagnostics
        .iter()
        .map(|diag| {
            let args = diag
                .args
                .iter()
                .map(|arg| (arg.name, arg.value.as_str()))
                .collect();
            (diag.code, diag.issue(), args)
        })
        .collect()
}

const OPTION_AND_OPERAND_MAIN: &str = r#"
@ cli "tool"
@ option verbose short "v" long "verbose" type bool
@ operand string name
main args args {
  print args.verbose
  print args.name
}
"#;

const OPERAND_ONLY_MAIN: &str = r#"
@ cli "tool"
@ operand string name
main args args {
  print args.name
}
"#;

#[test]
fn package_mir_cli_double_dash_ends_options_for_root_operand() {
    for (argumenta, expected) in [
        (vec!["--", "bob"], vec!["false", "bob"]),
        (vec!["--", "-x"], vec!["false", "-x"]),
        (vec!["-v", "--", "--verbose"], vec!["true", "--verbose"]),
    ] {
        let lines = run_cli_package(
            "cli-double-dash-root",
            &[("main.fab", OPTION_AND_OPERAND_MAIN)],
            &argumenta,
        )
        .unwrap_or_else(|diagnostics| {
            panic!(
                "`--` must end option parsing for {argumenta:?}, got {:?}",
                diagnostic_identities(&diagnostics)
            )
        });
        assert_eq!(lines, expected, "arguments {argumenta:?}");
    }
}

#[test]
fn package_mir_cli_double_dash_is_dropped_when_no_options_are_declared() {
    let lines = run_cli_package(
        "cli-double-dash-no-options",
        &[("main.fab", OPERAND_ONLY_MAIN)],
        &["--", "bob"],
    )
    .unwrap_or_else(|diagnostics| panic!("operand-only package must run, got {diagnostics:?}"));
    assert_eq!(lines, vec!["bob".to_owned()]);
}

#[test]
fn package_mir_cli_double_dash_ends_global_options_before_a_command() {
    let main = r#"
import from "./jobs" * as jobs

@ cli "tool"
@ option verbose long "verbose" type bool global
@ imperia "jobs" ex jobs
main args args {}
"#;
    let jobs = r#"
@ command "run"
@ operand string name
fn run() args args {
  print args.name
}
"#;
    let lines = run_cli_package(
        "cli-double-dash-command",
        &[("main.fab", main), ("jobs.fab", jobs)],
        &["--verbose", "--", "jobs", "run", "Ian"],
    )
    .unwrap_or_else(|diagnostics| {
        panic!(
            "`--` must end global option parsing, got {:?}",
            diagnostic_identities(&diagnostics)
        )
    });
    assert_eq!(lines, vec!["Ian".to_owned()]);
}

#[test]
fn package_mir_cli_double_dash_passes_dash_operand_to_a_mounted_command() {
    let main = r#"
import from "./jobs" * as jobs

@ cli "tool"
@ imperia "jobs" ex jobs
main args args {}
"#;
    let jobs = r#"
@ command "run"
@ operand string name
fn run() args args {
  print args.name
}
"#;
    let lines = run_cli_package(
        "cli-double-dash-command-operand",
        &[("main.fab", main), ("jobs.fab", jobs)],
        &["jobs", "run", "--", "-x"],
    )
    .unwrap_or_else(|diagnostics| panic!("mounted command must run, got {diagnostics:?}"));
    assert_eq!(lines, vec!["-x".to_owned()]);
}

// ── octeti/bytes CLI values ────────────────────────────────
//
// An `octeti` (`bytes` on the English surface) option or rest operand arrives
// on the package-MIR runner as the argument's UTF-8 byte list, not as text:
// the option field is `lista<b8>` and the rest field is a row list of those.
// The Rust emitter side of the same need is pinned in
// `radix-hir-rust::cli`.

const OCTETI_OPTION_MAIN: &str = r#"
@ cli "tool"
@ option payload long "payload" type bytes
main args args {
  let bytes payload ← args.payload coalesce |21|
  print payload.length()
}
"#;

#[test]
fn package_mir_cli_octeti_option_value_is_the_arguments_utf8_bytes() {
    let lines = run_cli_package(
        "cli-octeti-option",
        &[("main.fab", OCTETI_OPTION_MAIN)],
        &["--payload", "Az"],
    )
    .unwrap_or_else(|diagnostics| {
        panic!(
            "octeti option must parse on the runner, got {:?}",
            diagnostic_identities(&diagnostics)
        )
    });
    assert_eq!(lines, vec!["2".to_owned()]);

    let lines = run_cli_package(
        "cli-octeti-option-default",
        &[("main.fab", OCTETI_OPTION_MAIN)],
        &[],
    )
    .unwrap_or_else(|diagnostics| {
        panic!(
            "an omitted octeti option must take its default, got {:?}",
            diagnostic_identities(&diagnostics)
        )
    });
    assert_eq!(lines, vec!["1".to_owned()]);
}

const OCTETI_REST_OPERAND_MAIN: &str = r#"
@ cli "tool"
@ operand rest bytes data
main args args {
  print args.data.length()
  print args.data[0].length()
  print args.data[1].length()
}
"#;

#[test]
fn package_mir_cli_octeti_rest_operand_value_is_rows_of_utf8_bytes() {
    let lines = run_cli_package(
        "cli-octeti-rest",
        &[("main.fab", OCTETI_REST_OPERAND_MAIN)],
        &["Az", "Q"],
    )
    .unwrap_or_else(|diagnostics| {
        panic!(
            "octeti rest operand must parse on the runner, got {:?}",
            diagnostic_identities(&diagnostics)
        )
    });
    assert_eq!(lines, vec!["2".to_owned(), "2".to_owned(), "1".to_owned()]);
}

#[test]
fn package_mir_cli_entry_and_command_may_share_a_binding_name() {
    let main = r#"
@ cli "tool"
@ option verbose long "verbose" type bool global
main args args {}

@ command "greet"
@ operand string name
fn greet() args args {
  print args.name
}
"#;
    let lines = run_cli_package(
        "cli-shared-binding-name",
        &[("main.fab", main)],
        &["greet", "Ian"],
    )
    .unwrap_or_else(|diagnostics| {
        panic!(
            "entry and command records must each take their own fields, got {:?}",
            diagnostic_identities(&diagnostics)
        )
    });
    assert_eq!(lines, vec!["Ian".to_owned()]);
}

#[test]
fn package_mir_cli_entry_and_command_sharing_a_binding_name_keep_their_own_fields() {
    let main = r#"
@ cli "tool"
@ option verbose long "verbose" type bool global
main args args {
}

@ command "greet"
@ operand string nomen
fn greet() args args {
  print args.verbose
  print args.nomen
}
"#;
    let lines = run_cli_package(
        "cli-shared-args-binding",
        &[("main.fab", main)],
        &["--verbose", "greet", "Ian"],
    )
    .unwrap_or_else(|diagnostics| {
        panic!(
            "entry and command sharing the `args` binding must keep their own record fields, got {:?}",
            diagnostic_identities(&diagnostics)
        )
    });
    assert_eq!(lines, vec!["true".to_owned(), "Ian".to_owned()]);
}

// ── dynamic CLI exit expressions ───────────────────────────
//
// Ruling Q-T8: `package_mir_cli_surface_unsupported` never fires for an exit
// expression on the runner. The entry tail lowered by the MIR entry pass
// evaluates the expression, so `exit code() + 1` reaches `Host::exit` with the
// evaluated value instead of planning refusing the surface.

const DYNAMIC_EXIT_EXPRESSION_MAIN: &str = r#"
fn code() → int {
  return 5
}

@ cli "x"
main args a exit code() + 1 {
  print "cli"
}
"#;

#[test]
fn package_mir_cli_dynamic_exit_expression_runs() {
    let dir = crate::test_support::test_temp_dir("cli-dynamic-exit-expression");
    std::fs::write(dir.join("main.fab"), DYNAMIC_EXIT_EXPRESSION_MAIN).expect("write package file");
    let mut host = radix_mir_runner::BufferHost::default();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        crate::mir::run_package_mir(
            &radix_module::driver::Config::default(),
            &dir,
            &crate::test_support::test_mir_seams(),
            &mut host,
        )
    }));
    match outcome {
        Err(panic) => {
            // The BufferHost exit panic payload carries the status; downcasting
            // it is the assertion, so the test never matches a rendered
            // diagnostic message.
            assert_eq!(
                panic.downcast_ref::<String>().map(String::as_str),
                Some("runner exit: 6")
            );
        }
        Ok(result) => panic!(
            "a dynamic CLI exit expression must reach host.exit, got {:?}",
            result.map_err(|diagnostics| diagnostics
                .iter()
                .map(|diag| format!("{:?}:{:?}", diag.code, diag.issue()))
                .collect::<Vec<_>>())
        ),
    }
    assert_eq!(host.stdout_lines, vec!["cli".to_owned()]);
}
