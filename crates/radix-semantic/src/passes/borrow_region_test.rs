//! BRI-1 red drivers for the HIR-hosted borrow-region contract.
//!
//! The delivery artifact settles the host as a semantic CFG derived from
//! typed HIR. Locations retain HIR identity (`HirId`/`DefId`/`Span`), and a
//! borrow region ends at the borrow's last use rather than at HIR block exit.
//! The parity contract is `AcceptedApproximation ⊆ AcceptedRegions`: a
//! region-based checker may accept these cases after last-use inference, but
//! it must not reject anything accepted by the current approximation.
//!
//! These fixtures intentionally pin the current scope checker's red result.
//! Once the region authority lands, both programs are expected to analyze
//! successfully because the marked mutable borrow is last used before the
//! source write. Do not weaken the red expectation in this unit.

use crate::test_support::{analyze_source, issue};
use crate::SemanticErrorKind;

fn assert_current_scope_red(source: &str, label: &str) {
    let result = analyze_source(source);
    assert!(
        result.is_err(),
        "{label} must remain a current-scope red driver; diagnostics: {:?}",
        result.diagnostics
    );
    assert!(
        result.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == SemanticErrorKind::MutableBorrowConflict
                && issue(diagnostic) == Some("write_while_borrowed")
        }),
        "{label} must expose MutableBorrowConflict/write_while_borrowed; diagnostics: {:?}",
        result.diagnostics
    );
}

#[test]
fn dum_back_edge_releases_mutable_borrow_before_source_write() {
    // The body write is after the borrow's last use. The back-edge then
    // re-enters the condition and must not make the released loan loop-wide.
    let source = r#"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    dum xs.longitudo() < 5 {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
    }
}
"#;

    // PRE-BRI-3: block-scope tracking keeps `view` live until the body exits,
    // so the current authority reports write_while_borrowed. POST-BRI-3, the
    // last-use region should accept this fixture.
    assert_current_scope_red(source, "dum back-edge release-before-write");
}

#[test]
fn loop_rumpe_releases_mutable_borrow_before_source_write() {
    // The explicit `rumpe` supplies the loop exit while the fall-through body
    // edge returns to the next iteration. The source write follows the borrow
    // last use in the body.
    let source = r#"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    fac {
        varia in lista<numerus> view ← xs
        nota view.longitudo()
        xs.appende(4)
        rumpe
    } dum verum
}
"#;

    // PRE-BRI-3: the current block-scope authority reports the same red.
    // POST-BRI-3: last-use region inference should accept the fixture while
    // preserving the explicit `rumpe` exit edge.
    assert_current_scope_red(source, "loop rumpe release-before-write");
}
