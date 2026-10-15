//! Raw entry argument bindings must be initialized by the host before any
//! source read. Ordinary deferred-init locals retain definite-assignment checks.

use super::*;
use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};

const SOURCE: &str = r#"+++
locale = "en"
+++
main args argv {
    print argv.length()
    for from argv let arg {
        print arg
    }
}
"#;

fn session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_dev_locale_root(),
    )
}

#[test]
fn raw_entry_args_are_initialized_once_before_the_body() {
    let unit = analyze_source(&session(), "raw-entry-args.fab", SOURCE, AnalyzeRequest::default())
        .expect("raw entry args analyze as runtime-provided");
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("raw entry args lower");
    let entry = lowered
        .validated
        .program()
        .functions
        .last()
        .expect("entry function");
    let argv = entry
        .locals
        .iter()
        .find(|local| {
            local
                .name
                .is_some_and(|name| lowered.interner.resolve(name) == "argv")
        })
        .expect("argv local");
    let is_argv_init = |statement: &MirStatement| {
        matches!(
            &statement.kind,
            MirStatementKind::RuntimeCall { destination: Some(destination), call }
                if *destination == MirPlace::local(argv.id)
                    && call.args.is_empty()
                    && call.return_ty == argv.ty
                    && matches!(&call.intrinsic, MirIntrinsic::Provider(provider)
                        if provider.kind == MirProviderKind::Kernel(radix_mir::KernelModule::Processus)
                            && lowered.interner.resolve(provider.name) == "argv")
        )
    };
    assert!(
        is_argv_init(&entry.blocks[0].statements[0]),
        "host binding must precede the first source read"
    );
    assert_eq!(
        entry
            .blocks
            .iter()
            .flat_map(|block| &block.statements)
            .filter(|statement| is_argv_init(statement))
            .count(),
        1
    );
}

#[cfg(feature = "runner")]
#[test]
fn raw_entry_args_execute_populated_and_empty_host_vectors() {
    let unit = analyze_source(&session(), "raw-entry-args.fab", SOURCE, AnalyzeRequest::default())
        .expect("raw entry args analyze");
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("raw entry args lower");
    for (args, expected) in [
        (
            vec!["inferentia", "serve", "--prompt", "X"],
            vec!["4", "inferentia", "serve", "--prompt", "X"],
        ),
        (vec![], vec!["0"]),
    ] {
        let mut host = crate::mir::BufferHost::with_argumenta(args.into_iter().map(str::to_owned).collect());
        crate::mir::run_entry(&lowered.validated, &mut host).expect("host argv must initialize the entry local");
        assert_eq!(host.stdout_lines, expected);
    }
}

#[test]
fn raw_entry_args_do_not_exempt_user_locals_from_initialization() {
    let source = r#"+++
locale = "en"
+++
main args argv {
    let int uninitialized
    print uninitialized
}
"#;
    let errors = analyze_source(&session(), "raw-entry-uninitialized.fab", source, AnalyzeRequest::default())
        .err()
        .expect("a user local still requires initialization");
    assert!(
        errors
            .iter()
            .any(|diagnostic| diagnostic.issue() == Some("possibly_uninitialized"))
    );
}
