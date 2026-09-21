//! Shared loop/branch frame construction for the structured-CFG device
//! emitters (register row S06-1, delivery unit B4B-8).
//!
//! One law where emission is identical, per-backend syntax where it differs:
//! this module owns the **shape** of structured-control frames — if/else
//! arms, while headers, switch chains, and the statement-line skeletons
//! inside loop bodies — and nothing else. Target syntax enters as
//! parameters: the indent unit (`"    "` for MSL, `"  "` for WGSL) and an
//! output-assign callback that renders the backend's own output-store line
//! for an expression. Declaration lines (`float n = init;` vs
//! `var n: f32 = init;`) are pre-rendered by each backend and passed in as
//! finished text, and MIR walking, expression rendering, and condition
//! computation stay in the backend crates — this crate must not depend on
//! `radix-mir`, so the walker law cannot leak in here.
//!
//! The preservation contract is byte-identical emitted text: the Metal and
//! WGSL emit suites (control and `kv_cache`) pass without golden edits. A
//! frame change that would move a golden is a stop condition, not an edit.

use std::fmt::Write as _;

use crate::ident::{
    SCALAR_LOOP_CANDIDATES, SCALAR_LOOP_FALLBACK, VALUE_JOIN_CANDIDATES, VALUE_JOIN_FALLBACK,
    unclaimed_name,
};

/// The value-join frame's local name: the first value-join candidate not
/// claimed by the kernel parameter, output buffer, or index expression
/// (register row S06-1 — one binding law for both device emitters).
#[must_use]
pub fn value_join_name(param_name: &str, output_buffer: &str, index_expr: &str) -> &'static str {
    unclaimed_name(
        VALUE_JOIN_CANDIDATES,
        param_name,
        output_buffer,
        index_expr,
        VALUE_JOIN_FALLBACK,
    )
}

/// The scalar-loop frame's local name: the first scalar-loop candidate not
/// claimed by the kernel parameter, output buffer, or index expression
/// (register row S06-1 — one binding law for both device emitters).
#[must_use]
pub fn scalar_loop_name(param_name: &str, output_buffer: &str, index_expr: &str) -> &'static str {
    unclaimed_name(
        SCALAR_LOOP_CANDIDATES,
        param_name,
        output_buffer,
        index_expr,
        SCALAR_LOOP_FALLBACK,
    )
}

/// One `if`/`else if` arm of a switch chain whose body is statement lines
/// (the indexed-switch frame).
pub struct SwitchArmLines {
    /// The arm condition, rendered and parenthesized by the backend.
    pub condition: String,
    /// The arm's statement lines, already indented for the arm body.
    pub lines: Vec<String>,
}

/// One `if`/`else if` arm of a switch chain whose body is a single output
/// store (the scalar-switch frame).
pub struct SwitchArmStore {
    /// The arm condition, rendered and parenthesized by the backend.
    pub condition: String,
    /// The expression stored by the arm.
    pub expr: String,
}

/// An if/else frame whose arms each store one expression into the output
/// buffer: the shape behind `emit_return_branch` and
/// `emit_indexed_value_join_branch`.
///
/// `output_assign` renders the backend's store line for an expression
/// (without indent); the frame indents it at two indent units inside each
/// arm.
#[must_use]
pub fn if_else_store(
    condition: &str,
    then_expr: &str,
    else_expr: &str,
    indent: &str,
    output_assign: impl Fn(&str) -> String,
) -> String {
    let then_line = output_assign(then_expr);
    let else_line = output_assign(else_expr);
    format!(
        "{indent}if ({condition}) {{\n{indent}{indent}{then_line}\n{indent}}} else {{\n{indent}{indent}{else_line}\n{indent}}}"
    )
}

/// An if/else frame whose arms each carry pre-rendered statement lines:
/// the shape behind `emit_indexed_branch`.
#[must_use]
pub fn if_else_lines(
    condition: &str,
    then_lines: &[String],
    else_lines: &[String],
    indent: &str,
) -> String {
    let then_text = then_lines.join("\n");
    let else_text = else_lines.join("\n");
    format!("{indent}if ({condition}) {{\n{then_text}\n{indent}}} else {{\n{else_text}\n{indent}}}")
}

/// An `if`/`else if`/`else` chain whose arms carry statement lines and whose
/// default arm closes the chain: the shape behind `emit_indexed_switch`.
#[must_use]
pub fn indexed_switch(cases: &[SwitchArmLines], default_lines: &[String], indent: &str) -> String {
    let mut rendered = String::new();
    for (index, case) in cases.iter().enumerate() {
        let prefix = if index == 0 { "if" } else { "else if" };
        if !rendered.is_empty() {
            rendered.push(' ');
        }
        write!(
            rendered,
            "{indent}{prefix} ({}) {{\n{}\n{indent}}}",
            case.condition,
            case.lines.join("\n")
        )
        .ok();
    }
    write!(
        rendered,
        " else {{\n{}\n{indent}}}",
        default_lines.join("\n")
    )
    .ok();
    rendered
}

/// An `if`/`else if`/`else` chain whose arms each store one expression into
/// the output buffer: the shape behind `emit_scalar_switch`.
#[must_use]
pub fn scalar_switch(
    cases: &[SwitchArmStore],
    default_expr: &str,
    indent: &str,
    output_assign: impl Fn(&str) -> String,
) -> String {
    let mut rendered = String::new();
    for (index, case) in cases.iter().enumerate() {
        let prefix = if index == 0 { "if" } else { "else if" };
        if !rendered.is_empty() {
            rendered.push(' ');
        }
        let store_line = output_assign(&case.expr);
        write!(
            rendered,
            "{indent}{prefix} ({}) {{\n{indent}{indent}{store_line}\n{indent}}}",
            case.condition
        )
        .ok();
    }
    let default_line = output_assign(default_expr);
    write!(
        rendered,
        " else {{\n{indent}{indent}{default_line}\n{indent}}}"
    )
    .ok();
    rendered
}

/// A while-loop frame with an optional trailing output store: the shape
/// behind the `emit_scalar_loop*` and `emit_indexed_write_loop*` families.
///
/// `declaration` is the loop local's pre-rendered declaration line
/// (`float n = init;` in MSL, `var n: f32 = init;` in WGSL); `exit_expr`,
/// when present, is stored into the output buffer after the loop.
#[must_use]
pub fn while_loop(
    declaration: &str,
    condition: &str,
    body_lines: &[String],
    exit_expr: Option<&str>,
    indent: &str,
    output_assign: impl Fn(&str) -> String,
) -> String {
    let body_text = body_lines.join("\n");
    let mut rendered =
        format!("{indent}{declaration}\n{indent}while ({condition}) {{\n{body_text}\n{indent}}}");
    if let Some(exit_expr) = exit_expr {
        let exit_line = output_assign(exit_expr);
        write!(rendered, "\n{indent}{exit_line}").ok();
    }
    rendered
}

/// A value-join frame: declare a join local, assign it in both arms, then
/// store it into the output buffer — the shape behind
/// `emit_value_join_branch`.
///
/// `declaration` is the join local's pre-rendered declaration (`float n;`
/// in MSL, `var n: f32;` in WGSL).
#[must_use]
pub fn value_join(
    declaration: &str,
    join_name: &str,
    condition: &str,
    then_expr: &str,
    else_expr: &str,
    indent: &str,
    output_assign: impl Fn(&str) -> String,
) -> String {
    let store_line = output_assign(join_name);
    format!(
        "{indent}{declaration}\n{indent}if ({condition}) {{\n{indent}{indent}{join_name} = {then_expr};\n{indent}}} else {{\n{indent}{indent}{join_name} = {else_expr};\n{indent}}}\n{indent}{store_line}"
    )
}

/// The statement-line skeleton of a loop body guarded by a break/continue
/// condition: the shape behind `scalar_loop_control_body_lines` and
/// `indexed_loop_control_body_lines`.
#[must_use]
pub fn loop_control_body_lines(
    condition: &str,
    keyword: &str,
    continuation_lines: &[String],
    indent: &str,
) -> Vec<String> {
    let mut lines = vec![
        format!("{indent}{indent}if ({condition}) {{"),
        format!("{indent}{indent}{indent}{keyword};"),
        format!("{indent}{indent}}}"),
    ];
    lines.extend_from_slice(continuation_lines);
    lines
}

/// The statement-line skeleton of a loop body with a nested branch arm: the
/// shape behind `nested_scalar_loop_branch_body_lines` and
/// `nested_indexed_loop_branch_body_lines`.
#[must_use]
pub fn nested_branch_body_lines(
    condition: &str,
    then_lines: &[String],
    continuation_lines: &[String],
    indent: &str,
) -> Vec<String> {
    let mut lines = vec![format!("{indent}{indent}if ({condition}) {{")];
    lines.extend(then_lines.iter().map(|line| format!("{indent}{line}")));
    lines.push(format!("{indent}{indent}}}"));
    lines.extend_from_slice(continuation_lines);
    lines
}

/// The statement-line skeleton of a loop body with a nested
/// break/continue-guarded branch: the shape behind
/// `nested_indexed_loop_control_body_lines`.
#[must_use]
pub fn nested_control_body_lines(
    outer_condition: &str,
    inner_condition: &str,
    keyword: &str,
    nested_update_lines: &[String],
    continuation_lines: &[String],
    indent: &str,
) -> Vec<String> {
    let mut lines = vec![
        format!("{indent}{indent}if ({outer_condition}) {{"),
        format!("{indent}{indent}{indent}if ({inner_condition}) {{"),
        format!("{indent}{indent}{indent}{indent}{keyword};"),
        format!("{indent}{indent}{indent}}}"),
    ];
    lines.extend(
        nested_update_lines
            .iter()
            .map(|line| format!("{indent}{line}")),
    );
    lines.push(format!("{indent}{indent}}}"));
    lines.extend_from_slice(continuation_lines);
    lines
}

/// One loop-local update statement inside a loop body —
/// `{name} = {expr};` at two indent units: the line shape behind
/// `loop_body_lines` and `indexed_loop_body_lines`.
#[must_use]
pub fn loop_local_assign(loop_name: &str, expr: &str, indent: &str) -> String {
    format!("{indent}{indent}{loop_name} = {expr};")
}

/// The indent of statements inside a loop or arm body (two indent units),
/// for backends rendering their own output-assign lines through the frame
/// builders' callback seam.
#[must_use]
pub fn body_indent(indent: &str) -> String {
    format!("{indent}{indent}")
}

#[cfg(test)]
#[path = "frame_test.rs"]
mod tests;
