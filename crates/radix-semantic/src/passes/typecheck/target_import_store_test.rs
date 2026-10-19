//! Target-native import stores: the SCALAR DOOR law says only values that
//! arrive through a target import (`importa python "math" m`) may be stored
//! into a scalar cell. Without an import, an `ignotum` value stays refused.
//!
//! Tests assert error presence, never a specific issue or message text: the
//! explicit `ignotum` declaration itself is a lint, not an error.

use crate::test_support;
use radix_target::Target;

#[test]
fn no_import_scalar_store_stays_refused() {
    let result = test_support::analyze_source_for_target_la(
        r#"
incipit {
    sit ignotum x ← 1
    sit f64 y ← x
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
