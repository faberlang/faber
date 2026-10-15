//! Byte-value iteration over `octeti` (`lista<b8>`): the binding is a `b8`, the
//! number comes from an explicit `↦ u8`.

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session};
use crate::mir::{BufferHost, MirLoweringOptions, lower_analyzed_unit, run_source};

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirTypeScript))
}

#[test]
fn octeti_itera_ex_validates_byte_binding_and_runtime_bound() {
    let source = r"functio byte_list(octeti input) → lista<u8> {
    varia lista<u8> out ← vacua
    itera ex input sit byte {
        out.appende(byte ↦ u8)
    }
    redde out
}";
    let unit = crate::driver::analyze_source(&session(), "byte-list.fab", source, AnalyzeRequest::default())
        .expect("D10.4/D19.4 admit bytes iteration with a u8 binding");
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect("byte iteration must lower through the existing byte index row");
    crate::mir::validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("the byte carrier and binding types must pass MIR validation");
}

#[test]
fn octeti_itera_ex_runs_decoder_byte_list_in_order_and_handles_empty() {
    let source = r"functio byte_list(octeti input) → lista<u8> {
    varia lista<u8> out ← vacua
    itera ex input sit byte {
        out.appende(byte ↦ u8)
    }
    redde out
}

incipit {
    sit octeti input ← |00 7f 80 ff 00|
    sit lista<u8> copied ← byte_list(input)
    nota copied.longitudo()
    itera ex copied sit byte {
        nota byte
    }
    nota input[3] ↦ u8
    nota byte_list(||).longitudo()
}";
    let mut host = BufferHost::default();
    run_source(&session(), "byte-list-run.fab", source, &mut host)
        .unwrap_or_else(|error| panic!("decoder byte iteration must execute: {error:?}"));
    assert_eq!(host.stdout_lines, ["5", "0", "127", "128", "255", "0", "255", "0"]);
}

#[test]
fn octeti_itera_ex_preserves_continue_and_break() {
    let source = r"incipit {
    sit octeti input ← |00 7f 80 ff|
    itera ex input sit byte {
        si byte ≡ 0 {
            perge
        }
        si byte ≡ 255 {
            rumpe
        }
        nota byte ↦ u8
    }
}";
    let mut host = BufferHost::default();
    run_source(&session(), "byte-loop-flow.fab", source, &mut host)
        .unwrap_or_else(|error| panic!("byte iteration control flow must execute: {error:?}"));
    assert_eq!(host.stdout_lines, ["127", "128"]);
}

#[test]
fn octeti_itera_de_keeps_existing_fail_closed_admission() {
    let source = r"functio indices(octeti input) → vacuum {
    itera de input sit index {
        nota index
    }
}";
    let unit = crate::driver::analyze_source(&session(), "byte-indices.fab", source, AnalyzeRequest::default())
        .expect("the checker admits index iteration over a list of bytes");
    // Need: `octeti` is `lista<b8>`, an ordinary list, so `itera de`
    // is the ordinary list index iteration. MIR no longer declines it (the
    // retired opaque carrier only offered value iteration).
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .expect("index iteration over lista<b8> lowers like every list");
    crate::mir::validate_program(lowered.validated.program(), lowered.validated.validation())
        .expect("the index iteration of a byte list must pass MIR validation");
}
