//! Runner coverage for bounded `bytes<N>` carriers: the store-time capacity
//! check, `⊥` recovery, and the bridge back to unbounded `bytes`.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(body: &str) -> Result<Vec<String>, RunSourceError> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let source = format!("+++\nlocale = \"en\"\n+++\nmain {{\n{body}\n}}\n");
    let mut host = BufferHost::default();
    run_source(&session, "bounded.fab", &source, &mut host).map(|()| host.stdout_lines)
}

#[test]
fn bounded_octeti_const_within_bound_reports_length_and_emptiness() {
    let lines =
        run("const bytes raw ← |61 62|\nconst bytes<8> a ← raw ↦ bytes<8>\nprint a.length()\nprint a.is_empty()")
            .expect("runs");
    assert_eq!(lines, ["2", "false"]);
}

#[test]
fn bounded_octeti_store_over_the_bound_traps_with_the_rust_text() {
    let error = run("const bytes raw ← |61 62 63|\nconst bytes<2> a ← raw ↦ bytes<2>\nprint a.length()")
        .expect_err("overflow must fail");
    assert!(
        format!("{error:?}").contains("octeti to bounded octeti conversion failed"),
        "got: {error:?}"
    );
}

#[test]
fn bounded_octeti_recovery_supplies_the_fallback_on_overflow() {
    let lines =
        run("const bytes raw ← |61 62 63|\nconst bytes<2> a ← raw ↦ bytes<2> ⊥ (|7a| ↦ bytes<2>)\nprint a.length()")
            .expect("runs");
    assert_eq!(lines, ["1"]);
}

#[test]
fn bounded_octeti_widens_back_to_unbounded_bytes() {
    let lines = run(
        "const bytes raw ← |61 62|\nconst bytes<2> a ← raw ↦ bytes<2>\nconst bytes back ← a ↦ bytes\nprint back.length()\nprint a.length()",
    )
    .expect("runs");
    assert_eq!(lines, ["2", "2"]);
}
