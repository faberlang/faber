//! A reduction with a `thread` clause is the plain reduction, sequentially.
//!
//! `thread` (HIR `filum`) only permits parallel evaluation; its sequential
//! meaning is the plain fold in iteration order, so every operator and the
//! grouped form lower through the one reduction lowering. Only the ungrouped
//! `sum` keeps its declared device recipe (`FilumSum`).
//!
//! The semantic pass admits `thread` only inside an `@ kernel` function, and
//! the runner refuses to execute those by policy. Each test therefore analyzes
//! the source with its kernels intact (so admission is the real one), then
//! clears the `@ kernel` flag on the analyzed HIR to execute the lowered fold on
//! the runner and compares it against the same program without `thread`.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunnerError, run_analyzed};
use crate::program::analyze_prepared_program_name;
use radix_hir::HirItemKind;

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirTypeScript))
}

/// Corpus-style English source: the TOML head selects the code locale.
fn english(source: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\n{source}")
}

fn try_run(source: &str, clear_kernels: bool) -> Result<Vec<String>, Vec<RunnerError>> {
    let mut unit = analyze_prepared_program_name(&session(), "reducta-filum.fab", &english(source))
        .unwrap_or_else(|errors| panic!("source must analyze with its kernels intact: {errors:?}"));
    if clear_kernels {
        for item in &mut unit.hir.items {
            if let HirItemKind::Function(function) = &mut item.kind {
                function.is_kernel = false;
            }
        }
    }
    let mut host = BufferHost::default();
    run_analyzed(&mut unit, &mut host)?;
    Ok(host.stdout_lines)
}

/// Run with the kernels cleared; the output must equal the plain twin's.
fn run(source: &str) -> Vec<String> {
    try_run(source, true).unwrap_or_else(|errors| panic!("thread reduction must lower and run: {errors:?}"))
}

fn assert_thread_equals_plain(source: &str, expected: &[&str]) {
    let output = run(source);
    assert_eq!(output, expected);
    let half = output.len() / 2;
    assert_eq!(output[..half], output[half..], "thread and plain forms must print the same");
}

#[test]
fn thread_max_min_and_product_fold_like_the_plain_reduction() {
    let source = r"
@ kernel
@ public
fn t_max<size N>(tf32[N] a, f32 floor) → f32 {
    return reduce via max from a at [i] thread lane let s { return s } coalesce floor
}
@ kernel
@ public
fn t_min<size N>(tf32[N] a, f32 floor) → f32 {
    return reduce via min from a at [i] thread lane let s { return s } coalesce floor
}
@ kernel
@ public
fn t_prod<size N>(tf32[N] a) → f32 {
    return reduce via product from a at [i] thread lane let s { return s }
}
fn p_max<size N>(tf32[N] a, f32 floor) → f32 {
    return reduce via max from a at [i] let s { return s } coalesce floor
}
fn p_min<size N>(tf32[N] a, f32 floor) → f32 {
    return reduce via min from a at [i] let s { return s } coalesce floor
}
fn p_prod<size N>(tf32[N] a) → f32 {
    return reduce via product from a at [i] let s { return s }
}
main {
    let tf32[4] a ← [3.0, 1.0, 4.0, 2.0] ↦ tf32[4]
    print t_max(a, 0.0)
    print t_min(a, 9.0)
    print t_prod(a)
    print p_max(a, 0.0)
    print p_min(a, 9.0)
    print p_prod(a)
}
";
    assert_thread_equals_plain(source, &["4.0", "1.0", "24.0", "4.0", "1.0", "24.0"]);
}

#[test]
fn thread_argmax_and_argmin_ties_go_to_the_lowest_coordinate() {
    let source = r"
@ kernel
@ public
fn t_argmax<size N>(tf32[N] a) → i32 {
    return (reduce via argmax from a at [i] thread lane let s { return s }) coalesce -1
}
@ kernel
@ public
fn t_argmin<size N>(tf32[N] a) → i32 {
    return (reduce via argmin from a at [i] thread lane let s { return s }) coalesce -1
}
fn p_argmax<size N>(tf32[N] a) → int {
    return (reduce via argmax from a at [i] let s { return s }) coalesce -1
}
fn p_argmin<size N>(tf32[N] a) → int {
    return (reduce via argmin from a at [i] let s { return s }) coalesce -1
}
main {
    let tf32[5] hi ← [3.0, 1.0, 3.0, 2.0, 3.0] ↦ tf32[5]
    let tf32[5] lo ← [4.0, 2.0, 5.0, 2.0, 9.0] ↦ tf32[5]
    print t_argmax(hi)
    print t_argmin(lo)
    print p_argmax(hi)
    print p_argmin(lo)
}
";
    assert_thread_equals_plain(source, &["0", "1", "0", "1"]);
}

#[test]
fn thread_rank2_argmax_tie_returns_the_lowest_row_major_coordinate() {
    let source = r"
@ kernel
@ public
fn t_hi<size H, size W>(tf32[H, W] m) → tuple<i32, i32> ∪ none {
    return reduce via argmax from m at [i, j] thread lane let s { return s }
}
@ kernel
@ public
fn t_lo<size H, size W>(tf32[H, W] m) → tuple<i32, i32> ∪ none {
    return reduce via argmin from m at [i, j] thread lane let s { return s }
}
fn p_hi<size H, size W>(tf32[H, W] m) → tuple<int, int> ∪ none {
    return reduce via argmax from m at [i, j] let s { return s }
}
fn p_lo<size H, size W>(tf32[H, W] m) → tuple<int, int> ∪ none {
    return reduce via argmin from m at [i, j] let s { return s }
}
main {
    let tf32[2, 3] m ← [[1.0, 5.0, 2.0], [5.0, 0.0, 1.0]] ↦ tf32[2, 3]
    let tuple<i32, i32> hi ← t_hi(m) coalesce tuple<i32, i32> [9, 9]
    let tuple<i32, i32> lo ← t_lo(m) coalesce tuple<i32, i32> [9, 9]
    let tuple<int, int> phi ← p_hi(m) coalesce tuple<int, int> [9, 9]
    let tuple<int, int> plo ← p_lo(m) coalesce tuple<int, int> [9, 9]
    print hi[0]
    print hi[1]
    print lo[0]
    print lo[1]
    print phi[0]
    print phi[1]
    print plo[0]
    print plo[1]
}
";
    // Row-major order: the 5.0 tie resolves to (0, 1), the lone minimum is (1, 1).
    assert_thread_equals_plain(source, &["0", "1", "1", "1", "0", "1", "1", "1"]);
}

#[test]
fn thread_grouped_reductions_bucket_like_the_plain_reduction() {
    let source = r"
@ kernel
@ public
fn t_sum<size N>(tf32[N] a, tensor<i32, [N]> g) → tf32[3] {
    return reduce via sum from a at [i] per [g[i]] thread lane let s { return s }
}
@ kernel
@ public
fn t_count<size N>(tf32[N] a, tensor<i32, [N]> g) → tensor<i32, [3]> {
    return reduce via count from a at [i] per [g[i]] thread lane let s { return s ≻ 2.5 }
}
fn p_sum<size N>(tf32[N] a, tensor<i32, [N]> g) → tf32[3] {
    return reduce via sum from a at [i] per [g[i]] let s { return s }
}
fn p_count<size N>(tf32[N] a, tensor<i32, [N]> g) → tensor<i32, [3]> {
    return reduce via count from a at [i] per [g[i]] let s { return s ≻ 2.5 }
}
main {
    let tf32[6] a ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0] ↦ tf32[6]
    let tensor<i32, [6]> g ← [0, 1, 0, 1, 0, 1] ↦ tensor<i32, [6]>
    print t_sum(a, g)
    print t_count(a, g)
    print p_sum(a, g)
    print p_count(a, g)
}
";
    // Bucket 2 never receives an element: sum 0, count 0. Grouped max/min need a
    // body-less `coalesce` identity, which a `thread` reduction (body required)
    // cannot carry, so they have no thread form.
    assert_thread_equals_plain(source, &["[9.0, 12.0, 0.0]", "[2, 2, 0]", "[9.0, 12.0, 0.0]", "[2, 2, 0]"]);
}

#[test]
fn thread_empty_source_yields_the_coalesce_default_or_none() {
    let source = r"
@ kernel
@ public
fn t_max(tf32[0] a) → f32 {
    return reduce via max from a at [i] thread lane let s { return s } coalesce 7.5
}
@ kernel
@ public
fn t_argmax(tf32[0] a) → i32 ∪ none {
    return reduce via argmax from a at [i] thread lane let s { return s }
}
@ kernel
@ public
fn t_count(tf32[0] a) → i32 {
    return reduce via count from a at [i] thread lane let s { return s ≻ 0.0 }
}
fn p_max(tf32[0] a) → f32 {
    return reduce via max from a at [i] let s { return s } coalesce 7.5
}
fn p_argmax(tf32[0] a) → int ∪ none {
    return reduce via argmax from a at [i] let s { return s }
}
fn p_count(tf32[0] a) → int {
    return reduce via count from a at [i] let s { return s ≻ 0.0 }
}
main {
    let tf32[0] a ← empty
    print t_max(a)
    print t_argmax(a)
    print t_count(a)
    print p_max(a)
    print p_argmax(a)
    print p_count(a)
}
";
    assert_thread_equals_plain(source, &["7.5", "none", "0", "7.5", "none", "0"]);
}

#[test]
fn thread_count_any_and_all_match_the_plain_reduction() {
    let source = r"
@ kernel
@ public
fn t_count<size N>(tf32[N] a) → i32 {
    return reduce via count from a at [i] thread lane let s { return s ≻ 2.0 }
}
@ kernel
@ public
fn t_any<size N>(tf32[N] a) → bool {
    return reduce via any from a at [i] thread lane let s { return s ≻ 3.5 }
}
@ kernel
@ public
fn t_all<size N>(tf32[N] a) → bool {
    return reduce via all from a at [i] thread lane let s { return s ≻ 0.5 }
}
fn p_count<size N>(tf32[N] a) → int {
    return reduce via count from a at [i] let s { return s ≻ 2.0 }
}
fn p_any<size N>(tf32[N] a) → bool {
    return reduce via any from a at [i] let s { return s ≻ 3.5 }
}
fn p_all<size N>(tf32[N] a) → bool {
    return reduce via all from a at [i] let s { return s ≻ 0.5 }
}
main {
    let tf32[4] a ← [3.0, 1.0, 3.0, 2.0] ↦ tf32[4]
    print t_count(a)
    print t_any(a)
    print t_all(a)
    print p_count(a)
    print p_any(a)
    print p_all(a)
}
";
    assert_thread_equals_plain(source, &["2", "false", "true", "2", "false", "true"]);
}
