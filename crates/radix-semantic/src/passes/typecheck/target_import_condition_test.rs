//! Target-native imports: the CONDITION DOOR law says an `ignotum` value
//! observed at a control-flow condition (`si`/`dum`) converts with implicit
//! syntax but forced semantics, so the target's own truthiness judges it.
//!
//! Without an import, a plainly non-boolean condition stays refused as a mask:
//! the acceptance above must not weaken the truth-value requirement.
//!
//! Tests assert error presence, never a specific issue or message text.

use crate::test_support;
use radix_target::Target;

#[test]
fn si_observes_an_import_value() {
    test_support::assert_accepts_for_target_la(
        r#"
importa python "math" m

incipit {
    si m.flag {
        scribe 1
    }
}
"#,
        Target::HirPython,
    );
}

#[test]
fn dum_observes_an_import_value() {
    test_support::assert_accepts_for_target_la(
        r#"
importa python "math" m

incipit {
    dum m.flag {
        rumpe
    }
}
"#,
        Target::HirPython,
    );
}

#[test]
fn ordering_condition_observes_an_import_value() {
    test_support::assert_accepts_for_target_la(
        r#"
importa python "math" m

incipit {
    si m.x ≻ 3 {
        scribe 1
    }
}
"#,
        Target::HirPython,
    );
}

#[test]
fn a_local_int_condition_stays_refused() {
    let result = test_support::analyze_source_for_target_la(
        r#"
incipit {
    sit numerus n ← 1
    si n {
        scribe 1
    }
}
"#,
        Target::HirPython,
    );
    assert!(
        result.diagnostics.iter().any(|err| err.is_error()),
        "{:?}",
        result.diagnostics
    );
}
