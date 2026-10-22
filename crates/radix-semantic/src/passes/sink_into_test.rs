//! `sink ⇜ item` sink-into: the lowering and typing rules 3, 5 and 8.
//!
//! `⇜` is the operator form of `sink.sink(item)`. It dies on a failed write
//! where the method call reports, and it writes exactly the item it is given.
//! Each rule below has a positive fixture and negative fixtures asserting the
//! diagnostic kind and issue (never prose).
//!
//! - R3 the item is exactly the sink's `T`, with no implicit conversion
//! - R5 the statement has no value: it cannot initialize or assign a binding
//! - R8 the left operand conforms to the sinking contract; lists, bounded lists
//!   and sets sink their elements, while tensors/vectors/matrices are not sinks
//!   (`⇇` stays the tensor write)
//!
//! Rules 6 (the access rules) and 7 (no chains) live with their own units.

use crate::SemanticErrorKind;
use crate::test_support::{AnalyzedFixture, analyze_source_with_config, issue};

/// The fixtures are written in the English surface.
fn analyze_en(source: &str) -> AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    analyze_source_with_config(source, config)
}

fn assert_accepts(source: &str) {
    let result = analyze_en(source);
    assert!(
        !result.diagnostics.iter().any(|d| d.is_error()),
        "expected no error, got: {:?}",
        result.diagnostics
    );
}

fn assert_rejects(source: &str, kind: SemanticErrorKind, expected: &str) {
    let result = analyze_en(source);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.kind == kind && issue(d) == Some(expected)),
        "expected {kind:?} issue {expected:?}, got: {:?}",
        result.diagnostics
    );
}

/// The sinking contract as norma declares it, plus one conforming class.
const SINKING: &str = r#"@ radix contract "sinking"
interface Sink<T> {
    fn sink(T item) → void ⇥ Oops
    fn flush() → void ⇥ Oops
}

union Oops {
    Broken { string path }
}

class Box implements Sink<string> {
    mut list<string> items = empty

    fn sink(string item) → void ⇥ Oops {
        self.items.append(item)
    }

    fn flush() → void ⇥ Oops {
    }
}

"#;

fn with_sinking(body: &str) -> String {
    format!("{SINKING}{body}")
}

// =============================================================================
// Accepted shapes
// =============================================================================

#[test]
fn a_conforming_class_takes_the_operator_form() {
    assert_accepts(&with_sinking(
        r#"fn write(mut Box sink) → void {
    sink ⇜ "x"
}
main {}"#,
    ));
}

#[test]
fn a_parameter_bounded_by_the_sinking_contract_takes_the_operator_form() {
    assert_accepts(&with_sinking(
        r"fn feed<T implements Sink<T>>(mut T sink, T item) → void {
    sink ⇜ item
}
main {}",
    ));
}

// =============================================================================
// R3 exact item type, no implicit conversion
// =============================================================================

#[test]
fn rule3_an_int_item_into_a_string_sink_is_rejected() {
    assert_rejects(
        &with_sinking(
            r"fn write(mut Box sink) → void {
    sink ⇜ 1
}
main {}",
        ),
        SemanticErrorKind::TypeMismatch,
        "sink_into_item_type_mismatch",
    );
}

#[test]
fn rule3_a_wider_item_is_not_converted() {
    assert_rejects(
        &with_sinking(
            r"fn write(mut Box sink, f64 n) → void {
    sink ⇜ n
}
main {}",
        ),
        SemanticErrorKind::TypeMismatch,
        "sink_into_item_type_mismatch",
    );
}

// =============================================================================
// R5 the statement has no value
// =============================================================================

#[test]
fn rule5_a_sink_into_is_not_a_value_to_initialize_with() {
    assert_rejects(
        &with_sinking(
            r#"fn write(mut Box sink) → void {
    let int v ← sink ⇜ "x"
}
main {}"#,
        ),
        SemanticErrorKind::TypeMismatch,
        "sink_into_value_used",
    );
}

// =============================================================================
// R8 the left operand is a Sink<T>
// =============================================================================

#[test]
fn rule8_a_tensor_receiver_is_not_a_sink() {
    assert_rejects(
        &with_sinking(
            r#"fn write(mut tf32[2, 2] grid) → void {
    grid ⇜ "x"
}
main {}"#,
        ),
        SemanticErrorKind::OperandTypesInvalid,
        "sink_into_receiver_not_sink",
    );
}

#[test]
fn rule8_an_int_receiver_is_not_a_sink() {
    assert_rejects(
        &with_sinking(
            r#"fn write(int x) → void {
    x ⇜ "x"
}
main {}"#,
        ),
        SemanticErrorKind::OperandTypesInvalid,
        "sink_into_receiver_not_sink",
    );
}

#[test]
fn rule8_a_list_receiver_sinks_its_element() {
    assert_accepts(&with_sinking(
        r#"fn write(mut list<string> xs) → void {
    xs ⇜ "x"
}
main {}"#,
    ));
}

#[test]
fn rule8_a_set_receiver_sinks_its_element() {
    assert_accepts(&with_sinking(
        r#"fn write(mut set<string> xs) → void {
    xs ⇜ "x"
}
main {}"#,
    ));
}

#[test]
fn rule8_a_bounded_list_receiver_sinks_its_element() {
    assert_accepts(&with_sinking(
        r#"fn write(mut list<string, 4> xs) → void {
    xs ⇜ "x"
}
main {}"#,
    ));
}

#[test]
fn rule8_a_list_receiver_rejects_a_wider_item() {
    assert_rejects(
        &with_sinking(
            r#"fn write(mut list<string> xs) → void {
    xs ⇜ 1
}
main {}"#,
        ),
        SemanticErrorKind::TypeMismatch,
        "sink_into_item_type_mismatch",
    );
}

// =============================================================================
// R6 `const` is deep: a sink reached through a `let` name is refused
// =============================================================================

#[test]
fn rule6_a_sink_reached_through_a_let_name_is_rejected() {
    assert_rejects(
        &with_sinking(
            r#"main {
    let Box sink ← Box { items = [] }
    sink ⇜ "x"
}"#,
        ),
        SemanticErrorKind::ImmutableAssignment,
        "mutating_receiver_through_const",
    );
}

#[test]
fn rule6_a_mut_local_is_writable() {
    assert_accepts(&with_sinking(
        r#"main {
    mut Box sink ← Box { items = [] }
    sink ⇜ "x"
}"#,
    ));
}

// =============================================================================
// R7 no `⇜` into a collection the enclosing loop iterates
// =============================================================================

#[test]
fn rule7_a_sink_into_inside_a_loop_over_the_same_name_is_rejected() {
    assert_rejects(
        &with_sinking(
            r#"main {
    mut list<string> xs ← ["a"]
    for from xs let x {
        xs ⇜ x
    }
}"#,
        ),
        SemanticErrorKind::OperandTypesInvalid,
        "loop_collection_shape_change",
    );
}
