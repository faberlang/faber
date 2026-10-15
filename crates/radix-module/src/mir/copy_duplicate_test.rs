//! `copy` marker semantics on the MIR path (mutability-engine D-6; operator
//! ruling 2026-09-24): a `copy` bind or parameter is a deep duplicate at
//! acquisition and the source stays live. Unmarked binds alias (D-1).

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(name: &str, source: &str) -> Vec<String> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let mut host = BufferHost::default();
    match run_source(&session, name, source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Frontend(errors)) => panic!("fixture frontend errors: {errors:?}"),
        Err(RunSourceError::Mir(errors)) => panic!("fixture MIR errors: {errors:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("fixture runner errors: {errors:?}"),
    }
}

fn dump(source: &str) -> String {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let unit = crate::driver::analyze_source(&session, "copy.fab", source, crate::driver::AnalyzeRequest::default())
        .unwrap_or_else(|diagnostics| panic!("analysis succeeds: {diagnostics:?}"));
    crate::mir::dump_analyzed_unit(unit.application_view())
        .unwrap_or_else(|errors| panic!("MIR lowering succeeds: {errors:?}"))
}

const COPY_PARAM: &str = r#"+++
locale = "en"
+++
fn take(copy list<int> xs) → int {
    mut list<int> fresh ← xs
    fresh.append(5)
    return fresh.length()
}
main {
    let list<int> a ← [1, 2]
    print take(copy a)
    print a.length()
}
"#;

#[test]
fn copy_param_duplicates_at_entry() {
    assert!(dump(COPY_PARAM).contains("runtime duplicate(_0)"));
    assert_eq!(run("copy-param.fab", COPY_PARAM), vec!["3", "2"]);
}

const COPY_BIND_DEEP: &str = r#"+++
locale = "en"
+++
class V3 {
    mut f64 x = 0.0
    mut list<int> ys = []
}
main {
    mut V3 a ← V3 { x = 1.0 }
    mut V3 b ← copy a
    b.x ← 5.0
    b.ys.append(1)
    print a.x
    print a.ys.length()
    print b.x
    print b.ys.length()
}
"#;

#[test]
fn copy_bind_duplicates_deeply() {
    assert!(dump(COPY_BIND_DEEP).contains("runtime duplicate(_0)"));
    assert_eq!(run("copy-bind.fab", COPY_BIND_DEEP), vec!["1.0", "0", "5.0", "1"]);
}

const COPY_GENUS_PARAM: &str = r#"+++
locale = "en"
+++
class C {
    mut int n = 0
    fn inc() → void {
        self.n ← self.n + 1
    }
}
fn bumpcopy(copy C c) → void {
    mut C mine ← c
    mine.inc()
    print mine.n
}
main {
    mut C c ← C {}
    c.inc()
    bumpcopy(copy c)
    print c.n
}
"#;

#[test]
fn copy_genus_param_leaves_caller_untouched() {
    assert_eq!(run("copy-genus-param.fab", COPY_GENUS_PARAM), vec!["2", "1"]);
}

/// `copy` marks how a binding is initialized, never the binding afterwards:
/// a later `b ← c` of a class value is by reference even though `b` was
/// declared `copy` (operator ruling 2026-10-02). `a` stays untouched, and the
/// write through `b` lands on `c`.
const COPY_BIND_THEN_ASSIGN: &str = r#"+++
locale = "en"
+++
class Point {
    mut int x
}
main {
    let Point a ← Point { x = 1 }
    let Point c ← Point { x = 2 }
    mut Point b ← copy a
    b ← c
    b.x ← 5
    print a.x
    print b.x
    print c.x
}
"#;

#[test]
fn copy_applies_at_the_initializer_only_and_later_assignment_aliases() {
    assert_eq!(run("copy-assign.fab", COPY_BIND_THEN_ASSIGN), vec!["1", "5", "5"]);
}

const GENUS_BIND_ALIAS: &str = r#"+++
locale = "en"
+++
class V3 {
    mut f64 x = 0.0
}
main {
    mut V3 a ← V3 { x = 1.0 }
    mut V3 b ← a
    b.x ← 5.0
    print a.x
}
"#;

#[test]
fn unmarked_genus_bind_aliases() {
    assert_eq!(run("genus-alias.fab", GENUS_BIND_ALIAS), vec!["5.0"]);
}

#[cfg(feature = "mir-llvm")]
fn llvm_text(name: &str, source: &str) -> String {
    let result = crate::program::compile_cli_source(name, source, crate::codegen::Target::MirLlvm);
    for diagnostic in &result.diagnostics {
        assert!(!diagnostic.is_error(), "fixture {name} failed: {diagnostic:?}");
    }
    match result.output {
        Some(crate::Output::LlvmText(out)) => out.code,
        _ => panic!("no LLVM text for {name}: {:?}", result.diagnostics),
    }
}

/// LLVM genus locals are inline aggregates; the alias bind must address the
/// source slot instead of copying the aggregate into a fresh alloca.
#[cfg(feature = "mir-llvm")]
#[test]
fn llvm_genus_bind_addresses_source_slot() {
    let code = llvm_text("genus-alias.fab", GENUS_BIND_ALIAS);
    let main = code
        .find("define")
        .map_or(code.as_str(), |start| &code[start..]);
    assert!(
        main.contains("%l1.addr = getelementptr inbounds { double }, ptr %l0.addr, i32 0"),
        "alias bind must address the source slot:\n{main}"
    );
    assert!(!main.contains("%l1.addr = alloca"), "an alias bind owns no storage:\n{main}");
}

#[cfg(feature = "mir-llvm")]
#[test]
fn llvm_copy_param_clones_the_lista() {
    let code = llvm_text("copy-param.fab", COPY_PARAM);
    assert!(
        code.contains("@__faber_rt_v1_array_clone("),
        "copy parameter entry must clone the lista:\n{code}"
    );
}

const COPY_TENSOR: &str = r#"+++
locale = "en"
+++
main {
    mut tf32[4] a ← tf32[4] {}.fill(1.5)
    mut tf32[4] w ← copy a
    w[0] ← 9.0
    let tf32[4] s ← w + a
    print s[0]
    print s[1]
    print a[0]
    print w[0]
}
"#;

/// A `copy`-marked tensor keeps element write and tensor arithmetic: the
/// marker is a binding fact, not a different carrier, and the copy stays
/// independent of its source.
#[test]
fn copy_tensor_keeps_element_write_and_arithmetic() {
    assert_eq!(run("copy-tensor.fab", COPY_TENSOR), vec!["10.5", "3.0", "1.5", "9.0"]);
}

const COPY_TENSOR_RANK2_READ: &str = r#"+++
locale = "en"
+++
main {
    mut tf32[2, 3] a ← tf32[2, 3] {}.fill(1.5)
    mut tf32[2, 3] w ← copy a
    a[[1, 2]] ← 7.0
    w[[0, 1]] ← 4.0
    let u32 r ← 1
    let u32 c ← 2
    let f32 lit ← w[[1, 2]]
    let f32 var_idx ← w[[r, c]]
    let f32 written ← w[[0, 1]]
    let f32 source ← a[[1, 2]]
    print lit
    print var_idx
    print written
    print source
}
"#;

/// A rank-2 `copy` tensor reads by `[[r, c]]` exactly like the unmarked
/// binding: the list-shaped index lowers to `TensorGet`, not to a device-view
/// index projection (which validate rejects for a non-integer index).
#[test]
fn copy_tensor_rank2_bracket_read_matches_unmarked() {
    assert_eq!(
        run("copy-tensor-rank2.fab", COPY_TENSOR_RANK2_READ),
        vec!["1.5", "1.5", "4.0", "7.0"]
    );
}
