//! Need: a one-letter ascii literal in a `littera` slot is the
//! `littera` scalar (D10.5). Semantic analysis types it as `littera`, but a
//! MIR ascii constant is typed bare `ascii`, so the literal must ride a temp
//! typed by its own semantic cell — both as a call argument against a
//! `littera` parameter and as a literal case pattern over a `littera`
//! scrutinee.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(source: &str) -> (Vec<String>, Vec<String>) {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let mut host = BufferHost::default();
    let errors = match run_source(&session, "littera-slot.fab", source, &mut host) {
        Ok(()) => Vec::new(),
        Err(RunSourceError::Stepper(errors)) => errors.into_iter().map(|e| e.issue).collect(),
        Err(other) => panic!("{source}: {other:?}"),
    };
    (host.stdout_lines, errors)
}

fn ok(source: &str) -> Vec<String> {
    let (out, errors) = run(source);
    assert!(errors.is_empty(), "{source}: {errors:?}");
    out
}

#[test]
fn littera_slot_call_argument_is_the_littera_scalar() {
    let source = "\
functio identitas(littera c) → littera {
  redde c
}

incipit {
  nota identitas('a')
}
";
    assert_eq!(ok(source), ["a"]);
}

#[test]
fn littera_slot_switch_case_pattern_takes_the_matching_arm() {
    let source = "\
incipit {
  sit littera c ← 'a'
  elige c {
    casu 'a' { nota \"hit\" }
    ceterum { nota \"miss\" }
  }
}
";
    assert_eq!(ok(source), ["hit"]);
}
