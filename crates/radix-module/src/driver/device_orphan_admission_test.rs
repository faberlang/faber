//! DFV2-2 orphaned-annotation admission tests (`device-fragments-v2` DFV2-2).
//!
//! An annotated `@ nucleum` function that yields zero concrete device entries
//! in its unit — a size-generic with no same-unit call site — fails closed at
//! check stage with the named `nucleum_orphaned_annotation` issue at the
//! annotation span, carrying the definition span as the secondary span. The
//! old generic zero-kernel codegen message must not be the first diagnostic.

use super::compile;
use crate::codegen::Target;
use crate::diagnostics::DiagnosticArg;
use crate::driver::{analyze_source, AnalyzeRequest, ModuleCompileRequest};
use crate::test_support::test_session;

/// The spike's uninstantiated generic shape: annotated, public, never called.
const ORPHAN_GENERIC: &str = r#"+++
locale = "en"
+++

@ kernel
@ public
fn kadd<size M, size N>(tensor<f32, [M, N]> a, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {
    return a + b
}
"#;

/// The spike's green control: the same generic entry monomorphized by a
/// same-unit host caller.
const CONCRETE_LOCAL_GENERIC: &str = r#"+++
locale = "en"
+++

@ kernel
@ public
fn kadd<size M, size N>(tensor<f32, [M, N]> a, tensor<f32, [M, N]> b) → tensor<f32, [M, N]> {
    return a + b
}

fn main() → void {
    const tensor<f32, []> seed ← empty
    const _ a ← seed.from_flat([1.0, 2.0, 3.0, 4.0], [2, 2])
    const _ b ← seed.from_flat([10.0, 20.0, 30.0, 40.0], [2, 2])
    const _ r ← kadd(a, b)
    print "ok"
}
"#;

const ORPHAN_ISSUE: &str = "nucleum_orphaned_annotation";

fn arg_value<'a>(args: &'a [DiagnosticArg], name: &str) -> Option<&'a str> {
    args.iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

/// Driver spans are relative to the peeled body (frontmatter removed).
fn peeled_body(source: &str) -> &str {
    let after_open = source.split_once("+++\n").expect("frontmatter open");
    after_open
        .1
        .split_once("+++\n")
        .expect("frontmatter close")
        .1
}

#[test]
fn uninstantiated_generic_annotation_fails_check_with_named_orphan_issue() {
    let result = analyze_source(
        &test_session(Target::MirWgsl),
        "orphan-generic.fab",
        ORPHAN_GENERIC,
        AnalyzeRequest::default(),
    );
    let Err(diagnostics) = result else {
        panic!("orphaned generic annotation must fail at check stage");
    };
    let first = diagnostics.first().expect("at least one diagnostic");
    assert!(
        first.is_error(),
        "orphan admission must be an error, never a warning: {first:?}"
    );
    assert_eq!(arg_value(&first.args, "issue"), Some(ORPHAN_ISSUE), "{first:?}");
    assert!(
        !first
            .message
            .contains("requires at least one @ nucleum function"),
        "the generic zero-kernel codegen message must not be first: {first:?}"
    );
}

#[test]
fn orphan_diagnostic_points_at_the_annotation_with_definition_secondary_span() {
    let result = analyze_source(
        &test_session(Target::MirWgsl),
        "orphan-generic.fab",
        ORPHAN_GENERIC,
        AnalyzeRequest::default(),
    );
    let Err(diagnostics) = result else {
        panic!("orphaned generic annotation must fail at check stage");
    };
    let first = diagnostics.first().expect("at least one diagnostic");
    let span = first.span.expect("primary span at the annotation");
    let body = peeled_body(ORPHAN_GENERIC);
    let primary = &body[span.start as usize..span.end as usize];
    assert!(primary.contains("kernel"), "primary span covers the annotation: {primary:?}");

    let definition_start = arg_value(&first.args, "definition_start").expect("definition_start");
    let start: usize = definition_start.parse().expect("definition_start numeric");
    assert!(
        body[start..].starts_with("fn kadd"),
        "secondary span points at the function definition"
    );
    assert!(start >= span.end as usize, "secondary span is distinct from the primary");
    assert!(arg_value(&first.args, "definition_end").is_some(), "definition_end present");
}

#[test]
fn concrete_local_generic_stays_green_at_check() {
    let result = analyze_source(
        &test_session(Target::MirWgsl),
        "concrete-generic.fab",
        CONCRETE_LOCAL_GENERIC,
        AnalyzeRequest::default(),
    );
    match result {
        Ok(unit) => {
            assert!(
                unit.diagnostics
                    .iter()
                    .all(|diag| arg_value(&diag.args, "issue") != Some(ORPHAN_ISSUE)),
                "no orphan diagnostic for the concrete local generic: {:?}",
                unit.diagnostics
            );
        }
        Err(diagnostics) => {
            panic!("a locally instantiated generic entry is admissible: {diagnostics:?}");
        }
    }
}

#[cfg(feature = "mir-metal")]
#[test]
fn orphan_is_not_reported_as_the_generic_zero_kernel_codegen_message_first() {
    let session = crate::driver::Session::new(
        crate::driver::Config::default()
            .with_target(Target::MirMetal)
            .with_dev_stdlib(),
    );
    let result = compile(&session, "orphan-generic.fab", ORPHAN_GENERIC, ModuleCompileRequest::default());
    assert!(!result.success(), "orphan must fail closed");
    let first = result.diagnostics.first().expect("at least one diagnostic");
    assert_eq!(arg_value(&first.args, "issue"), Some(ORPHAN_ISSUE), "{first:?}");
}
