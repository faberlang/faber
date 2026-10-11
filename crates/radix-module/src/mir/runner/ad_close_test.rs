//! A7 proof (DECISIONS D8.3): conversation lifecycle on the MIR runner.
//!
//! A conversation holds a router slot from open to its release point.
//! Release runs when the conversation completes (the handler returns or
//! fails, or the consumer reaches a terminal), on explicit `close()`, and on
//! the last reference as a safety net. `fini` reports the real terminal.
//! The run ledger counts slots: `live_peak` shows whether finished
//! conversations released while their handles were still held, and `live`
//! (read after every frame closed) shows what the last-reference net missed.

use super::{BufferHost, RunnerError};
use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::lower_analyzed_unit;
use crate::program::analyze_prepared_program_name;
use radix_mir_runner::sermo::ConversationLedger;

fn run(name: &str, source: &str) -> (Vec<String>, ConversationLedger) {
    let session = Session::new(Config::default().with_target(Target::HirRust));
    let analysis = analyze_prepared_program_name(&session, name, source)
        .unwrap_or_else(|errors| panic!("{name} should analyze: {errors:?}"));
    let lowered = lower_analyzed_unit(analysis.application_view(), crate::mir::MirLoweringOptions::default())
        .unwrap_or_else(|errors| panic!("{name} should lower: {errors:?}"));
    let mut host = BufferHost::default();
    let outcome = radix_mir_runner::run_entry_with_stats(&lowered.validated, &mut host)
        .unwrap_or_else(|errors: Vec<RunnerError>| panic!("{name} should start: {errors:?}"));
    if let Err(errors) = outcome.result {
        panic!("{name} should run: {errors:?}");
    }
    (host.stdout_lines, outcome.conversations)
}

const HANDLERS: &str = r#"+++
locale = "la"
+++
@ ad 'numeri:bis'
functio bis(numerus n) → numerus {
    redde n * 2
}

@ ad 'numeri:defice'
functio defice(numerus n) → numerus ⇥ textus {
    iace "defecit " + (n ↦ textus)
}

@ ad 'numeri:cuncti'
functio cuncti() fiunt → numerus {
    varia numerus i ← 0
    dum verum {
        nota "produce §"(i)
        cede i
        i ← i + 1
    }
}
"#;

fn program(body: &str) -> String {
    format!("{HANDLERS}\n{body}")
}

#[test]
fn finished_conversations_release_while_their_handles_stay_in_a_map() {
    let source = program(
        r"incipit {
    varia tabula<numerus, sermo> canales ← vacua
    itera ab 0‥4 fixum i {
        canales[i] ← ad 'numeri:bis' (i)
    }
    itera ab 0‥4 fixum i {
        nota canales[i] ↦ numerus
    }
}
",
    );
    let (lines, ledger) = run("close-map.fab", &source);
    assert_eq!(lines, ["0", "2", "4", "6"]);
    assert_eq!(ledger.opened, 4);
    assert_eq!(
        ledger.live_peak, 1,
        "each plain handler completes at open, so its slot is back before the next opens"
    );
    assert_eq!(ledger.live, 0);
}

#[test]
fn live_cursor_conversations_hold_slots_until_closed() {
    let source = program(
        r"incipit {
    varia tabula<numerus, sermo> canales ← vacua
    itera ab 0‥3 fixum i {
        canales[i] ← ad 'numeri:cuncti'
    }
    itera ab 0‥3 fixum i {
        nota canales[i].close() ⊥ falsum
    }
    nota canales[0].close() ⊥ falsum
}
",
    );
    let (lines, ledger) = run("close-live.fab", &source);
    assert_eq!(
        lines,
        ["verum", "verum", "verum", "falsum"],
        "close cancels an unread cursor handler before it produces, and a second close reports falsum"
    );
    assert_eq!(ledger.opened, 3);
    assert_eq!(ledger.live_peak, 3);
    assert_eq!(ledger.live, 0);
}

#[test]
fn close_reports_the_error_terminal() {
    let source = program(
        r#"functio claude(sermo s) → textus ⇥ textus {
    fixum bivalens clausa ← s.close()
    redde "clausa §"(clausa)
}

incipit {
    fixum sermo f ← ad 'numeri:defice' (7)
    nota f.close() ⊥ verum
    fixum sermo g ← ad 'numeri:defice' (8)
    nota claude(g) ⊥ "error"
    fixum sermo h ← ad 'numeri:bis' (1)
    nota claude(h) ⊥ "error"
    fac {
        fixum sermo k ← ad 'numeri:defice' (9)
        fixum bivalens nunquam ← k.close()
        nota nunquam
    } cape err {
        nota err
    }
}
"#,
    );
    let (lines, _) = run("close-error.fab", &source);
    assert_eq!(lines, ["verum", "error", "clausa verum", "defecit 9"]);
}

#[test]
fn abandoned_conversation_releases_on_last_reference() {
    let source = program(
        r#"functio desere() {
    fixum sermo s ← ad 'numeri:cuncti'
    nota "deserta"
}

incipit {
    desere()
    desere()
    fixum sermo tenta ← ad 'numeri:cuncti'
    nota "tenta"
}
"#,
    );
    let (lines, ledger) = run("close-drop.fab", &source);
    assert_eq!(
        lines,
        ["deserta", "deserta", "tenta"],
        "an abandoned cursor handler never resumes"
    );
    assert_eq!(ledger.opened, 3);
    assert_eq!(
        ledger.live_peak, 1,
        "each abandoned conversation released when its function returned"
    );
    assert_eq!(ledger.live, 0, "the held conversation released when incipit ended");
}

#[test]
fn fini_reports_the_real_terminal() {
    let source = program(
        r#"functio nomen(status t) → textus {
    discerne t {
        casu done { redde "done" }
        casu error { redde "error" }
        casu cancel { redde "cancel" }
        ceterum { redde "alia" }
    }
}

incipit {
    fixum sermo vivus ← ad 'numeri:cuncti'
    nota nomen(vivus.tuus<numerus>().fini())
    fixum sermo plenus ← ad 'numeri:bis' (2)
    nota nomen(plenus.tuus<numerus>().fini())
    fixum sermo fractus ← ad 'numeri:defice' (3)
    nota nomen(fractus.tuus<numerus>().fini())
}
"#,
    );
    let (lines, ledger) = run("close-fini.fab", &source);
    assert_eq!(lines, ["cancel", "done", "error"]);
    assert_eq!(ledger.live, 0);
}
