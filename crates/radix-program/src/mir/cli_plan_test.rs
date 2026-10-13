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
    let mut host = radix_module::mir::BufferHost::with_argumenta(
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
                diagnostics
                    .iter()
                    .map(|diag| (diag.code, diag.issue(), diag.message.clone()))
                    .collect::<Vec<_>>()
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
            diagnostics
                .iter()
                .map(|diag| (diag.code, diag.issue(), diag.message.clone()))
                .collect::<Vec<_>>()
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
