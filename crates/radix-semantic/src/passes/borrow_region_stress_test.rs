//! BRI-U4 stress corpus: `dum`/`loop` back-edges, condition borrows, and
//! nested-loop release points through the production region path.
//!
//! Every accepted fixture must clear the full semantic pipeline (`rumpe`
//! fixtures keep their exits); every rejected fixture must still fire the
//! SEM052 family when the region is live at the write. The differential
//! parity obligation over these programs plus the exempla corpus lives in
//! `borrow_region_parity_test.rs`.

use crate::SemanticErrorKind;
use crate::test_support::{analyze_source_la, issue};

fn assert_accepted(source: &str, label: &str) {
    let result = analyze_source_la(source);
    assert!(
        !result.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SemanticErrorKind::MutableBorrowConflict
                && matches!(
                    issue(diagnostic),
                    Some("write_while_borrowed" | "use_while_mutably_borrowed")
                )
        }),
        "{label} must be region-accepted; diagnostics: {:?}",
        result.diagnostics
    );
}

fn assert_write_conflict(source: &str, label: &str) {
    let result = analyze_source_la(source);
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SemanticErrorKind::MutableBorrowConflict
                && issue(diagnostic) == Some("write_while_borrowed")
        }),
        "{label} must keep write_while_borrowed; diagnostics: {:?}",
        result.diagnostics
    );
}

#[test]
fn condition_borrow_stays_live_across_back_edge() {
    // The marked borrow's use is the loop condition itself. Every iteration
    // re-enters that use across the back edge, so the region is live at the
    // body write even though the write follows the condition in source
    // order. This is the cyclic case where block-scope approximation and
    // true regions agree: the conflict must fire.
    assert_write_conflict(
        r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    varia in lista<numerus> view ← xs
    dum view.longitudo() ≺ 5 {
        xs.appende(4)
    }
}
",
        "condition borrow live around the back edge",
    );
}

#[test]
fn condition_borrow_write_before_condition_use_conflicts() {
    // Same shape, but the body write precedes the borrow's last use in
    // source order — the use follows the write inside the body. The region
    // is live at the write and SEM052 must fire.
    assert_write_conflict(
        r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    varia in lista<numerus> view ← xs
    dum xs.longitudo() ≺ 5 {
        xs.appende(4)
        nota view.longitudo()
    }
}
",
        "body write before the condition-established borrow's later use",
    );
}

#[test]
fn nested_loop_releases_inner_borrow_before_outer_write() {
    // Inner-loop borrow whose last use is inside the inner body; the outer
    // write runs after the inner loop exits, so the region is dead.
    assert_accepted(
        r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    varia numerus i ← 0
    dum i ≺ 2 {
        varia in lista<numerus> view ← xs
        varia numerus j ← 0
        dum j ≺ 2 {
            nota view.longitudo()
            j ← j + 1
        }
        xs.appende(4)
        i ← i + 1
    }
}
",
        "nested inner borrow released before outer write",
    );
}

#[test]
fn nested_loop_write_while_inner_borrow_live_conflicts() {
    // The outer write lands inside the inner loop before the borrow's last
    // use: the region is live at the write and SEM052 must fire.
    assert_write_conflict(
        r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    varia numerus i ← 0
    dum i ≺ 2 {
        varia in lista<numerus> view ← xs
        varia numerus j ← 0
        dum j ≺ 2 {
            xs.appende(4)
            nota view.longitudo()
            j ← j + 1
        }
        i ← i + 1
    }
}
",
        "outer write inside inner loop while borrow live",
    );
}

#[test]
fn perge_back_edge_does_not_resurrect_released_borrow() {
    // The `perge` continue edge returns to the header; the source write sits
    // before it but after the borrow's last use, so the region stays dead.
    assert_accepted(
        r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    varia numerus i ← 0
    dum i ≺ 3 {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
        i ← i + 1
        perge
    }
}
",
        "perge back-edge after last use",
    );
}

#[test]
fn body_reestablish_after_release_is_not_a_self_conflict() {
    // The approximation rejected a borrow re-established later in the same
    // block against its own earlier shadow. Regions must accept it.
    assert_accepted(
        r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    varia in lista<numerus> first ← xs
    nota first.longitudo()
    varia in lista<numerus> second ← xs
    nota second.longitudo()
    xs.appende(4)
}
",
        "re-established borrow after earlier shadow release",
    );
}

#[test]
fn shared_condition_borrow_without_body_write_stays_clean() {
    // A shared (`de`) condition borrow with no body write: nothing conflicts,
    // and the loop still exits through the explicit `rumpe`.
    assert_accepted(
        r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    varia de lista<numerus> view ← xs
    dum view.longitudo() ≺ 5 {
        rumpe
    }
}
",
        "shared condition borrow, no body write",
    );
}

#[test]
fn loop_body_borrow_live_at_later_write_conflicts() {
    // Straight-line divergence inside one loop body: the write precedes the
    // last use, so the region is live at the write.
    assert_write_conflict(
        r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    dum xs.longitudo() ≺ 8 {
        varia in lista<numerus> view ← xs
        xs.appende(4)
        nota view.longitudo()
    }
}
",
        "loop-body write before last use",
    );
}
