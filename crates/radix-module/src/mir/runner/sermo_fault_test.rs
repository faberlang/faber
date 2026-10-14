//! 341d49e1-5: a bare `sermo ↦ T` failure in a `⇥ E` function crosses as the
//! leaf's `{kind, path}` record; the registered `valor → E` row registered by
//! `@ conversion` converts it on the failure edge, so the caller's `catch`
//! binds the declared fault type — never the raw record.

use super::{BufferHost, RunSourceError, RunnerError, run_source};
use crate::codegen::Target;
use crate::driver::{Config, Session};

fn run_host(name: &str, source: &str, host: &mut BufferHost) -> Result<(), Vec<RunnerError>> {
    match run_source(
        &Session::new(Config::default().with_target(Target::HirRust)),
        name,
        source,
        host,
    ) {
        Ok(()) => Ok(()),
        Err(RunSourceError::Stepper(errors)) => Err(errors),
        Err(error) => panic!("{name} should reach the MIR runner: {error:?}"),
    }
}

fn program(body: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\n{body}\n")
}

#[test]
fn sermo_failure_is_converted_to_the_declared_fault() {
    const MISSING: &str = "/nonexistent/341d49e1-5/sermo-fault.txt";
    let source = program(&format!(
        r#"
union Fault {{
    NotFound {{
        string path
    }},
    Other
}}

@ conversion
fn fault_from(value raw) → Fault {{
    do {{
        const map<string, value> record ← raw ↦ map<string, value>
        const string kind ← record["kind"] ↦ string
        const string path ← record["path"] ↦ string
        if kind ≡ "NotFound" {{
            return variant NotFound {{ path = path }}
        }}
        return variant Other
    }} catch err {{
        return variant Other
    }}
}}

fn read(string p) → string ⇥ Fault {{
    return call 'solum:lege' (p) ↦ string
}}

main {{
    do {{
        const string text ← read("{MISSING}")
        print text
    }} catch err {{
        match err {{
            case NotFound const path {{
                print path
            }}
            case Other {{
                print "other"
            }}
        }}
    }}
}}
"#
    ));
    let mut host = BufferHost::default();
    run_host("sermo-fault.fab", &source, &mut host)
        .unwrap_or_else(|errors| panic!("sermo fault program should run: {errors:?}"));
    assert_eq!(host.stdout_lines, [MISSING.to_owned()]);
}

#[test]
fn generic_sermo_failure_is_converted_to_the_declared_fault() {
    const MISSING: &str = "/nonexistent/341d49e1-5/sermo-fault-generic.txt";
    let source = program(&format!(
        r#"
union Fault {{
    NotFound {{
        string path
    }},
    Other
}}

@ conversion
fn fault_from(value raw) → Fault {{
    do {{
        const map<string, value> record ← raw ↦ map<string, value>
        const string kind ← record["kind"] ↦ string
        const string path ← record["path"] ↦ string
        if kind ≡ "NotFound" {{
            return variant NotFound {{ path = path }}
        }}
        return variant Other
    }} catch err {{
        return variant Other
    }}
}}

@ radix type T mut string list<string> bytes
fn read<T>(string p) → T ⇥ Fault {{
    return call 'solum:lege' (p) ↦ T
}}

main {{
    do {{
        const string text ← read<string>("{MISSING}")
        print text
    }} catch err {{
        match err {{
            case NotFound const path {{
                print path
            }}
            case Other {{
                print "other"
            }}
        }}
    }}
}}
"#
    ));
    let mut host = BufferHost::default();
    run_host("sermo-fault-generic.fab", &source, &mut host)
        .unwrap_or_else(|errors| panic!("sermo fault program should run: {errors:?}"));
    assert_eq!(host.stdout_lines, [MISSING.to_owned()]);
}

#[test]
fn generic_sermo_success_returns_the_value_through_the_failable_channel() {
    let path = std::env::temp_dir().join("pumpfix-g77c071f7-generic-read.txt");
    std::fs::write(&path, "bridge-ok").expect("write generic read fixture");
    let source = program(&format!(
        r#"
union Fault {{
    NotFound {{
        string path
    }},
    Other
}}

@ conversion
fn fault_from(value raw) → Fault {{
    do {{
        const map<string, value> record ← raw ↦ map<string, value>
        const string kind ← record["kind"] ↦ string
        const string path ← record["path"] ↦ string
        if kind ≡ "NotFound" {{
            return variant NotFound {{ path = path }}
        }}
        return variant Other
    }} catch err {{
        return variant Other
    }}
}}

@ radix type T mut string list<string> bytes
fn read<T>(string p) → T ⇥ Fault {{
    return call 'solum:lege' (p) ↦ T
}}

main {{
    const string text ← read<string>("{}")
    print text
}}
"#,
        path.display()
    ));
    let mut host = BufferHost::default();
    run_host("sermo-fault-generic-success.fab", &source, &mut host)
        .unwrap_or_else(|errors| panic!("sermo fault program should run: {errors:?}"));
    assert_eq!(host.stdout_lines, ["bridge-ok".to_owned()]);
}

/// e71f077f-4: a registered conversion row that is itself failable
/// (`⇥ E`) must be applied on the propagation edge with `try_call`, so the
/// row's own failure reaches the same alternate exit. `matched_kind` is the
/// raw `kind` the row throws `Perit` for; any other kind returns `Absens`.
fn failable_row_source(matched_kind: &str) -> String {
    program(&format!(
        r#"
union Fault {{ Absens, Perit }}

fn kind_of(value raw) → string {{
    do {{
        const map<string, value> record ← raw ↦ map<string, value>
        const string kind ← record["kind"] ↦ string
        return kind
    }} catch err {{
        return ""
    }}
}}

@ conversion
fn convert_error(value raw) → Fault ⇥ Fault {{
    if kind_of(raw) ≡ "{matched_kind}" {{
        throw variant Perit
    }}
    return variant Absens
}}

fn read(string p) → string ⇥ Fault {{
    return call 'solum:lege' (p) ↦ string
}}

main {{
    do {{
        const string text ← read("/nonexistent/e71f077f/failable-row.txt")
        print text
    }} catch err {{
        match err {{
            case Absens {{
                print "absens"
            }}
            case Perit {{
                print "perit"
            }}
        }}
    }}
}}
"#
    ))
}

#[test]
fn failable_row_error_is_the_propagated_fault() {
    let source = failable_row_source("NotFound");
    let mut host = BufferHost::default();
    run_host("sermo-fault-failable-row-error.fab", &source, &mut host)
        .unwrap_or_else(|errors| panic!("failable conversion row should run: {errors:?}"));
    assert_eq!(host.stdout_lines, ["perit".to_owned()]);
}

#[test]
fn failable_row_success_value_is_the_propagated_fault() {
    let source = failable_row_source("Never");
    let mut host = BufferHost::default();
    run_host("sermo-fault-failable-row-success.fab", &source, &mut host)
        .unwrap_or_else(|errors| panic!("failable conversion row should run: {errors:?}"));
    assert_eq!(host.stdout_lines, ["absens".to_owned()]);
}
