//! Need, unit 7: the rank-1 tensor bracket range (`t[1 ‥ 4]`)
//! lowers to the `TensorSliceNd` collection op and runs on the MIR runner.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

/// Corpus-style English source: the TOML head selects the code locale.
fn english(source: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\n{source}")
}

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirTypeScript))
}

fn ok(body: &str) -> Vec<String> {
    let source = english(&format!("main {{\n{body}\n}}\n"));
    let mut host = BufferHost::default();
    match run_source(&session(), "tensor-bracket-range.fab", &source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(other) => panic!("{body}: {other:?}"),
    }
}

const T: &str = "mut tf32[6] t ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0] ↦ tf32[6]";

#[test]
fn tensor_bracket_range_rank1_slices_with_step() {
    let body = format!(
        "{T}\n\
         let tf32[3] s ← t[1 ‥ 4]\n\
         let tf32[3] q ← t[0 ‥ 6 step 2]\n\
         print s\n\
         print q"
    );
    assert_eq!(ok(&body), ["[2.0, 3.0, 4.0]", "[1.0, 3.0, 5.0]"]);
}
