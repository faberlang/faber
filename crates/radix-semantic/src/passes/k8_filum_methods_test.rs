//! K8 look-through of method kernels and public kernels (gpu-reset rule 12.7,
//! extended 2026-10-04).
//!
//! Anything that can be a kernel callee is analysed as if inlined, whether it
//! is a private helper, a public kernel, or a method called through a receiver.
//! `ego` maps to the receiver's storage, a tensor-typed field to the storage the
//! receiver holds in it, and every other rule of the enclosing body applies to
//! the method body unchanged. A call the analysis cannot bind to one body, or
//! whose arguments may overlap, keeps a named decline.

use crate::test_support::{analyze_source_la, assert_accepts_la, assert_rejects_with_issue_la};

const ISSUE: &str = "filum_independence_unproven";
const GRID: &str = "tensor<f32, [4]>";

fn accepted(source: &str) {
    let result = analyze_source_la(source);
    assert!(result.success(), "{:?}", result.diagnostics);
}

fn has_reason(error: &crate::SemanticError, reason: &str) -> bool {
    error
        .args
        .iter()
        .any(|arg| arg.name == "reason" && arg.value == reason)
}

fn rejected(source: &str, reason: &str) {
    let result = analyze_source_la(source);
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(!errors.is_empty(), "body unexpectedly certified");
    assert!(
        errors.iter().all(|d| d.issue() == Some(ISSUE)),
        "not independence errors: {errors:?}"
    );
    assert!(
        errors.iter().any(|d| has_reason(d, reason)),
        "missing reason {reason}: {errors:?}"
    );
}

/// A genus whose methods are kernels over a tensor field and a scalar field.
const STENCIL: &str = r"
genus Stencil {
    varia tensor<f32, [4]> data
    varia f32 gain

    @ nucleum
    functio put(numerus at, f32 v) → vacuum {
        ego.data[at] ← v * ego.gain
    }

    @ nucleum
    functio peek(numerus at) → f32 {
        redde ego.data[at + 1]
    }

    @ nucleum
    functio here(numerus at) → f32 {
        redde ego.data[at]
    }

    @ nucleum
    functio put_into(in tensor<f32, [4]> target, numerus at, f32 v) → vacuum {
        target[at] ← v
    }

    @ nucleum
    functio read_from(de tensor<f32, [4]> source, numerus at) → f32 {
        redde source[at + 1]
    }

    @ nucleum
    functio rebind(de tensor<f32, [4]> other) → vacuum {
        ego.data ← other
    }

    @ nucleum
    functio bump() → vacuum {
        ego.gain ← ego.gain + 1.0
    }

    @ nucleum
    functio put_twice(numerus at, f32 v) → vacuum {
        ego.put(at, v)
        ego.put(at, v + 1.0)
    }

    @ nucleum
    functio spin(numerus at) → f32 {
        redde ego.spin(at)
    }

    @ nucleum
    functio ping(numerus at) → f32 {
        redde ego.pong(at)
    }

    @ nucleum
    functio pong(numerus at) → f32 {
        redde ego.ping(at)
    }

    @ nucleum
    functio both(in tensor<f32, [4]> left, in tensor<f32, [4]> right, numerus at) → vacuum {
        left[at] ← 1.0
        right[at] ← 2.0
    }

    @ nucleum
    functio put_via_function(numerus at, f32 v) → vacuum {
        field_store(in ego.data, at, v)
    }

    functio host_method(f32 x) → f32 {
        redde x
    }
}

@ nucleum
functio field_store(in tensor<f32, [4]> target, numerus at, f32 v) → vacuum {
    target[at] ← v
}
";

/// A genus whose method kernel calls a host function. Appended only where the
/// test expects the analysis (which runs before kernel legality) to decline.
const LEAKY: &str = r"
functio host(f32 x) → f32 { redde x }

genus Leaky {
    varia tensor<f32, [4]> data

    @ nucleum
    functio calls_host(f32 x) → f32 {
        redde host(x)
    }
}
";

/// A public kernel `k` whose loop body is `statement`; `setup` runs before the
/// loop. `s` holds `out` and `t` holds `src` unless the setup says otherwise.
fn kernel(setup: &str, statement: &str) -> String {
    kernel_with("", setup, statement)
}

/// [`kernel`] with `extra` declarations after the stencil genus.
fn kernel_with(extra: &str, setup: &str, statement: &str) -> String {
    format!(
        r"{STENCIL}{extra}
@ nucleum
@ publica
functio k(in {GRID} out, de {GRID} src) → vacuum {{
    {setup}
    itera ex src apud [i] filum lane fixum value {{
        {statement}
    }}
}}
"
    )
}

const OVER_OUT: &str = "varia Stencil s ← Stencil { data = out, gain = 2.0 }";
const OVER_SRC: &str = "varia Stencil t ← Stencil { data = src, gain = 2.0 }";

// =============================================================================
// Method kernels: writes and reads through `ego`, fields and parameters
// =============================================================================

#[test]
fn method_writing_a_mut_tensor_param_at_the_loop_coordinate_is_accepted() {
    accepted(&kernel(OVER_SRC, "t.put_into(in out, i, value)"));
}

#[test]
fn method_writing_its_tensor_field_at_the_loop_coordinate_is_accepted() {
    accepted(&kernel(OVER_OUT, "s.put(i, value)"));
}

#[test]
fn method_writing_its_tensor_field_at_another_coordinate_is_rejected() {
    rejected(
        &kernel(OVER_OUT, "s.put(0, value)"),
        "write_not_own_coordinate",
    );
    rejected(
        &kernel(OVER_OUT, "s.put(i + 1, value)"),
        "write_not_own_coordinate",
    );
}

#[test]
fn method_reading_a_neighbour_of_the_written_tensor_is_rejected() {
    // Through the field: the receiver holds the tensor the loop writes.
    rejected(
        &kernel(OVER_OUT, "out[i] ← s.peek(i)"),
        "written_storage_neighbor_read",
    );
    // Through the receiver's own write and a second call's neighbour read.
    rejected(
        &kernel(
            OVER_OUT,
            "s.put(i, value)\n        fixum f32 next ← s.peek(i)",
        ),
        "written_storage_neighbor_read",
    );
    // Through a parameter the call passes the written tensor to.
    rejected(
        &kernel(OVER_SRC, "out[i] ← t.read_from(out, i)"),
        "written_storage_neighbor_read",
    );
}

#[test]
fn method_reading_a_neighbour_of_an_unwritten_tensor_is_accepted() {
    accepted(&kernel(OVER_SRC, "out[i] ← t.peek(i)"));
    accepted(&kernel(OVER_OUT, "out[i] ← s.read_from(src, i)"));
}

#[test]
fn method_reading_the_written_tensor_at_the_own_coordinate_is_accepted() {
    accepted(&kernel(OVER_OUT, "out[i] ← s.here(i) + value"));
}

#[test]
fn method_calling_a_sibling_method_through_ego_is_accepted() {
    accepted(&kernel(OVER_OUT, "s.put_twice(i, value)"));
}

#[test]
fn method_handing_its_field_to_a_kernel_function_keeps_the_mapping() {
    accepted(&kernel(OVER_OUT, "s.put_via_function(i, value)"));
    rejected(
        &kernel(OVER_OUT, "s.put_via_function(0, value)"),
        "write_not_own_coordinate",
    );
}

#[test]
fn method_through_an_iteration_local_receiver_is_accepted() {
    // The receiver is built inside the iteration, so its field write is not
    // loop-carried state.
    accepted(&kernel(
        "",
        "varia tensor<f32, [4]> scratch ← vacua\n        varia Stencil local ← Stencil { data = scratch, gain = 2.0 }\n        local.put(i, value)\n        out[i] ← local.here(i)",
    ));
}

#[test]
fn method_through_a_receiver_that_holds_no_known_storage_is_declined() {
    // A genus parameter has no roots, so its field is not storage the
    // analysis can place: the write declines instead of being assumed own.
    rejected(
        &format!(
            r"{STENCIL}
@ nucleum
@ publica
functio k(in Stencil s, de {GRID} src) → vacuum {{
    itera ex src apud [i] filum lane fixum value {{
        s.put(i, value)
    }}
}}
"
        ),
        "unknown_storage_identity",
    );
}

// =============================================================================
// `ego` fields mutated by a method
// =============================================================================

#[test]
fn method_mutating_a_scalar_field_of_an_outer_receiver_is_rejected() {
    // A loop-carried scalar: every iteration bumps the one receiver.
    rejected(&kernel(OVER_OUT, "s.bump()"), "outer_storage_mutation");
}

#[test]
fn method_mutating_a_scalar_field_of_an_iteration_local_receiver_is_accepted() {
    accepted(&kernel(
        "",
        "varia tensor<f32, [4]> scratch ← vacua\n        varia Stencil local ← Stencil { data = scratch, gain = 2.0 }\n        local.bump()\n        local.put(i, value)\n        out[i] ← local.here(i)",
    ));
}

#[test]
fn method_rebinding_a_tensor_field_of_an_outer_receiver_is_rejected() {
    rejected(&kernel(OVER_OUT, "s.rebind(src)"), "outer_storage_mutation");
}

// =============================================================================
// Generic genus
// =============================================================================

#[test]
fn generic_method_is_analysed_once_and_declines_what_it_cannot_place() {
    let generic = |statement: &str| {
        format!(
            r"
genus Arca<T> {{
    varia tensor<f32, [4]> data

    @ nucleum
    functio pone(numerus at, f32 v) → vacuum {{
        ego.data[at] ← v
    }}

    @ nucleum
    functio idem(T v) → T {{
        redde v
    }}
}}

@ nucleum
@ publica
functio k(in {GRID} out, de {GRID} src) → vacuum {{
    varia Arca<f32> s ← Arca<f32> {{ data = out }}
    itera ex src apud [i] filum lane fixum value {{
        {statement}
    }}
}}
"
        )
    };
    accepted(&generic("s.pone(i, value)"));
    // A value typed by the type parameter has no storage identity here.
    rejected(
        &generic("out[i] ← s.idem(value)"),
        "unknown_read_storage_identity",
    );
}

// =============================================================================
// Public kernels
// =============================================================================

const SHOWN: &str = r"
@ nucleum
@ publica
functio shown(f32 x) → f32 { redde x * 2.0 }

@ nucleum
@ publica
functio shown_store(in tensor<f32, [4]> target, numerus at, f32 v) → vacuum {
    target[at] ← v
}
";

fn function_kernel(helpers: &str, statement: &str) -> String {
    format!(
        r"{helpers}
@ nucleum
@ publica
functio k(in {GRID} out, de {GRID} src) → vacuum {{
    itera ex src apud [i] filum lane fixum value {{
        {statement}
    }}
}}
"
    )
}

#[test]
fn public_kernel_called_from_a_kernel_is_accepted() {
    accepted(&function_kernel(SHOWN, "out[i] ← shown(value)"));
    accepted(&function_kernel(SHOWN, "shown_store(in out, i, value)"));
}

#[test]
fn public_kernel_writing_elsewhere_is_rejected() {
    rejected(
        &function_kernel(SHOWN, "shown_store(in out, 0, value)"),
        "write_not_own_coordinate",
    );
}

// =============================================================================
// Declines that stay by name
// =============================================================================

#[test]
fn overlapping_mut_arguments_of_a_method_call_are_a_borrow_error() {
    // The call-site borrow rule checks a user-method call's arguments (need
    // 1ce9ea98 hole 1), so two `in` arguments naming one tensor are rejected
    // there first. The filum pass still declines the same call on its own, so
    // both diagnostics are present; the second owner is not leaned on.
    let result = analyze_source_la(&kernel(OVER_OUT, "s.both(in out, in out, i)"));
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors
            .iter()
            .any(|d| d.issue() == Some("mutable_borrow_conflict")),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .any(|d| has_reason(d, "overlapping_call_arguments")),
        "{errors:?}"
    );
}

#[test]
fn overlapping_mut_arguments_through_the_receiver_are_declined_by_the_pass() {
    // The borrow rule compares the argument places: `out` and `s.data` are
    // different places to it, though `s` holds `out`. Only the filum pass maps
    // the receiver's field to the storage it holds, so it is the sole owner of
    // this spelling.
    let source = kernel(OVER_OUT, "s.both(in out, in s.data, i)");
    let result = analyze_source_la(&source);
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.iter().all(|d| d
            .issue()
            .is_none_or(|issue| !issue.contains("borrow_conflict"))),
        "the borrow rule is silent here: {errors:?}"
    );
    rejected(&source, "overlapping_call_arguments");
    // Two distinct tensors are fine.
    accepted(&format!(
        r"{STENCIL}
@ nucleum
@ publica
functio k(in {GRID} out, in {GRID} aux, de {GRID} src) → vacuum {{
    varia Stencil s ← Stencil {{ data = src, gain = 2.0 }}
    itera ex src apud [i] filum lane fixum value {{
        s.both(in out, in aux, i)
    }}
}}
"
    ));
}

#[test]
fn receiver_field_overlapping_a_method_argument_is_declined() {
    rejected(
        &kernel(OVER_OUT, "s.put_into(in out, i, value)"),
        "overlapping_call_arguments",
    );
}

#[test]
fn function_call_with_aliased_mut_arguments_is_still_a_borrow_error() {
    let result = analyze_source_la(&function_kernel(
        r"
@ nucleum
functio both(in tensor<f32, [4]> left, in tensor<f32, [4]> right, numerus at) → vacuum {
    left[at] ← 1.0
    right[at] ← 2.0
}
",
        "both(in out, in out, i)",
    ));
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors.iter().any(|d| d
            .issue()
            .is_some_and(|issue| issue.contains("borrow_conflict"))),
        "{errors:?}"
    );
    assert!(
        errors.iter().all(|d| d.issue() != Some(ISSUE)),
        "{errors:?}"
    );
}

#[test]
fn method_recursion_is_declined_by_name() {
    rejected(&kernel(OVER_OUT, "out[i] ← s.spin(i)"), "recursive_call");
    rejected(&kernel(OVER_OUT, "out[i] ← s.ping(i)"), "recursive_call");
}

#[test]
fn host_method_call_stays_declined() {
    rejected(
        &kernel(OVER_OUT, "out[i] ← s.host_method(value)"),
        "unproved_call_effect",
    );
}

#[test]
fn method_kernel_with_a_host_call_inside_is_declined_at_that_call() {
    rejected(
        &kernel_with(
            LEAKY,
            "",
            "varia Leaky bad ← Leaky { data = out }\n        out[i] ← bad.calls_host(value)",
        ),
        "unproved_call_effect",
    );
}

#[test]
fn contract_dispatch_is_declined_by_name() {
    rejected(
        r"
implendum Numerabile {
    functio valor() → f32
}

genus Unus implet Numerabile {
    functio valor() → f32 {
        redde 1.0
    }
}

@ nucleum
@ publica
functio k(in tensor<f32, [4]> out, de tensor<f32, [4]> src, Numerabile n) → vacuum {
    itera ex src apud [i] filum lane fixum value {
        out[i] ← n.valor()
    }
}
",
        "dynamic_dispatch",
    );
}

#[test]
fn host_receiver_method_that_writes_a_field_stays_declined() {
    // A user method that is not a kernel never gets looked through, whatever
    // its name or body.
    rejected(
        r"
genus Counter {
    varia numerus count
    functio accipe(numerus index) → numerus {
        ego.count ← ego.count + 1
        redde ego.count
    }
}

@ nucleum
@ publica
functio k(in tensor<numerus, [4]> grid) → vacuum {
    varia Counter counter ← Counter { count = 0 }
    itera ex grid apud [i] filum lane fixum value {
        grid[i] ← counter.accipe(i)
    }
}
",
        "unproved_call_effect",
    );
}

// =============================================================================
// Reduction terms
// =============================================================================

#[test]
fn reduction_term_may_call_a_method_kernel_that_reads() {
    assert_accepts_la(&format!(
        r"{STENCIL}
@ nucleum
functio k(tensor<f32, [4]> xs) → f32 {{
    varia Stencil s ← Stencil {{ data = xs, gain = 2.0 }}
    redde reducta via summa ex xs apud [i] fixum v {{ redde v + s.here(i) }}
}}
"
    ));
}

#[test]
fn reduction_term_may_call_a_public_kernel() {
    assert_accepts_la(
        r"
@ nucleum
@ publica
functio shown(f32 x) → f32 { redde x }
@ nucleum
functio k(tensor<f32, [32]> xs) → f32 {
    redde reducta via summa ex xs apud [i] fixum s { redde shown(s) }
}
",
    );
}

#[test]
fn reduction_term_calling_a_host_function_is_still_rejected() {
    assert_rejects_with_issue_la(
        &format!(
            r"{STENCIL}
functio host(f32 x) → f32 {{ redde x }}
@ nucleum
functio k(tensor<f32, [4]> xs) → f32 {{
    redde reducta via summa ex xs apud [i] fixum v {{ redde host(v) }}
}}
"
        ),
        "reducta_body_context",
    );
}

#[test]
fn reduction_term_calling_a_host_method_is_still_rejected() {
    assert_rejects_with_issue_la(
        &format!(
            r"{STENCIL}
@ nucleum
functio k(tensor<f32, [4]> xs) → f32 {{
    varia Stencil s ← Stencil {{ data = xs, gain = 2.0 }}
    redde reducta via summa ex xs apud [i] fixum v {{ redde s.host_method(v) }}
}}
"
        ),
        "reducta_body_context",
    );
}

#[test]
fn reduction_term_method_kernel_that_writes_is_rejected() {
    let result = analyze_source_la(&format!(
        r"{STENCIL}
@ nucleum
functio k(tensor<f32, [4]> xs) → f32 {{
    varia Stencil s ← Stencil {{ data = xs, gain = 2.0 }}
    redde reducta via summa ex xs apud [i] fixum v {{
        s.put(i, v)
        redde v
    }}
}}
"
    ));
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors
            .iter()
            .any(|d| d.issue() == Some("reducta_body_effect")),
        "{errors:?}"
    );
}

#[test]
fn reduction_term_method_kernel_with_a_host_call_inside_is_rejected() {
    let result = analyze_source_la(&format!(
        r"{LEAKY}
@ nucleum
functio k(tensor<f32, [4]> xs) → f32 {{
    varia Leaky s ← Leaky {{ data = xs }}
    redde reducta via summa ex xs apud [i] fixum v {{ redde s.calls_host(v) }}
}}
"
    ));
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors
            .iter()
            .any(|d| d.issue() == Some("reducta_body_effect")),
        "{errors:?}"
    );
}
