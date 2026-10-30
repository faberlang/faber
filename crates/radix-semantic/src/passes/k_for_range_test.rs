//! KFOR: a counted range loop (`itera ab a‥b fixum j`) inside a `filum` body.
//!
//! The independence pass proves the range loop like the `while` loop: its
//! bounds are evaluated once per lane and read nothing a lane writes, its
//! binder is iteration-local, and its body meets the same rules as the lane
//! body. The lockstep form keeps its decline. Every assertion is on the
//! diagnostic code, issue and named reason arguments, never message text.

use crate::test_support::analyze_source_la;

const ISSUE: &str = "filum_independence_unproven";
const CODE: &str = "SEM010";

fn accepted(source: &str) {
    let result = analyze_source_la(source);
    assert!(result.success(), "{:?}", result.diagnostics);
}

fn rejected(source: &str, reason: &str) {
    let result = analyze_source_la(source);
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(!errors.is_empty(), "body unexpectedly certified");
    assert!(
        errors
            .iter()
            .all(|d| d.issue() == Some(ISSUE) && crate::semantic_spec(d.kind).code == CODE),
        "not independence errors: {errors:?}"
    );
    assert!(
        errors.iter().any(|d| d
            .args
            .iter()
            .any(|arg| arg.name == "reason" && arg.value == reason)),
        "missing reason {reason}: {errors:?}"
    );
}

/// A generic row kernel: lane `i` owns `out[i]`, `w` and `x` are read-only,
/// `n` is a lane-invariant scalar, `tmp` is a second written tensor and
/// `lens` is a written integer tensor a bound may not read.
fn kernel(body: &str) -> String {
    format!(
        r"@ nucleum
@ publica
functio gemv<magnitudo N, magnitudo K>(
    de tensor<f32, [N]> dom,
    de tensor<f32, [N, K]> w,
    de tensor<f32, [K]> x,
    varia tensor<f32, [N]> out,
    varia tensor<f32, [N, K]> scratch,
    varia tensor<f32, [K]> tmp,
    varia tensor<i32, [N]> lens,
    i32 n
) → vacuum {{
    itera ex dom apud [i] filum lane sit e {{
        {body}
    }}
}}
"
    )
}

/// The same lane body with an outer scalar the lane may not carry state in.
fn kernel_outer(body: &str) -> String {
    format!(
        r"@ nucleum
@ publica
functio gemv<magnitudo N, magnitudo K>(
    de tensor<f32, [N]> dom,
    de tensor<f32, [N, K]> w,
    de tensor<f32, [K]> x,
    varia tensor<f32, [N]> out
) → vacuum {{
    varia f32 total ← 0.0
    itera ex dom apud [i] filum lane sit e {{
        {body}
    }}
}}
"
    )
}

#[test]
fn serial_row_dot_with_a_range_loop_accepted() {
    accepted(&kernel(
        "varia f32 acc ← 0.0
        itera ab 0‥n sit j {
            acc ← acc + w[[i, j]] * x[[j]]
        }
        out[[i]] ← acc",
    ));
}

#[test]
fn nested_range_inside_a_range_accepted() {
    accepted(&kernel(
        "varia f32 acc ← 0.0
        itera ab 0‥n sit j {
            itera ab 0‥n sit k {
                acc ← acc + w[[i, j]] * x[[k]]
            }
        }
        out[[i]] ← acc",
    ));
}

#[test]
fn literal_generic_and_scalar_parameter_bounds_accepted() {
    accepted(&kernel(
        "varia f32 acc ← 0.0
        itera ab 0‥3 sit a {
            acc ← acc + w[[i, a]]
        }
        itera ab 1‥n sit b {
            acc ← acc + x[[b]]
        }
        out[[i]] ← acc",
    ));
}

#[test]
fn range_product_inside_a_lane_accepted() {
    accepted(&kernel(
        "varia f32 acc ← 0.0
        itera ab 0‥n, 0‥n sit a, b {
            acc ← acc + w[[i, a]] * x[[b]]
        }
        out[[i]] ← acc",
    ));
}

#[test]
fn iteration_local_scratch_inside_the_range_accepted() {
    accepted(&kernel(
        "varia f32 acc ← 0.0
        itera ab 0‥n sit j {
            sit f32 term ← w[[i, j]] * x[[j]]
            acc ← acc + term
        }
        out[[i]] ← acc",
    ));
}

#[test]
fn lane_owned_scratch_row_with_inner_range_accepted() {
    accepted(&kernel(
        "itera ab 0‥n sit j {
            scratch[[i, j]] ← w[[i, j]]
        }",
    ));
}

#[test]
fn scratch_write_with_non_lane_leading_coordinate_rejected() {
    rejected(
        &kernel(
            "itera ab 0‥n sit j {
                scratch[[j, 0]] ← w[[i, j]]
            }",
        ),
        "write_not_own_coordinate",
    );
}

#[test]
fn owned_row_may_not_reuse_the_lane_coordinate_inside() {
    // The lane's own coordinate owns the row only when it appears once:
    // reusing it in an inner slot is not row ownership.
    rejected(
        &kernel(
            "itera ab 0‥n sit j {
                scratch[[i, i]] ← w[[i, j]]
            }",
        ),
        "write_not_own_coordinate",
    );
    rejected(
        &kernel(
            "itera ab 0‥n sit j {
                scratch[[i, i + 1]] ← w[[i, j]]
            }",
        ),
        "write_not_own_coordinate",
    );
}

#[test]
fn owned_row_inner_coordinates_may_be_anything() {
    // Inner coordinates only pick cells inside the lane's own row: a
    // lane-local def names a cell as well as a loop binder does.
    accepted(&kernel(
        "itera ab 0‥n sit j {
            sit numerus q ← j + 1
            scratch[[i, q]] ← w[[i, j]]
        }",
    ));
}

#[test]
fn bound_reading_a_lane_written_tensor_rejected() {
    // `lens` is written by the lane (at a constant cell), so its value is not a
    // lane-invariant bound.
    rejected(
        &kernel(
            "lens[[0]] ← 2
        itera ab 0‥lens[[0]] sit j {
            out[[i]] ← x[[j]]
        }",
        ),
        "written_storage_neighbor_read",
    );
}

#[test]
fn bound_reading_the_lanes_own_written_cell_rejected() {
    rejected(
        &kernel(
            "lens[[i]] ← 2
        itera ab 0‥lens[[i]] sit j {
            out[[i]] ← x[[j]]
        }",
        ),
        "written_storage_neighbor_read",
    );
}

#[test]
fn range_product_bound_reading_a_lane_written_tensor_rejected() {
    rejected(
        &kernel(
            "lens[[i]] ← 2
        itera ab 0‥n, 0‥lens[[i]] sit a, b {
            tmp[[a]] ← x[[b]]
        }",
        ),
        "written_storage_neighbor_read",
    );
}

#[test]
fn body_storing_outer_storage_at_a_non_lane_coordinate_rejected() {
    rejected(
        &kernel(
            "itera ab 0‥n sit j {
            out[[j]] ← x[[j]]
        }",
        ),
        "write_not_own_coordinate",
    );
}

#[test]
fn loop_carried_write_to_outer_state_rejected() {
    rejected(
        &kernel_outer(
            "itera ab 0‥3 sit j {
            total ← total + x[[j]]
        }
        out[[i]] ← total",
        ),
        "outer_local_mutation",
    );
}

#[test]
fn lockstep_loop_still_declined() {
    rejected(
        r"incipit {
    varia tensor<f32, [4]> rows ← vacua
    varia tensor<f32, [4]> out ← vacua
    sit lista<f32> xs ← [1.0, 2.0]
    sit lista<f32> ys ← [3.0, 4.0]
    itera ex rows apud [i] filum lane sit e {
        itera ex xs, ys sit a, b {
            out[[i]] ← a + b
        }
    }
}",
        "unproved_loop_form",
    );
}

/// A kernel range joins its endpoints by the same rule as the host (ruling 12),
/// so an integer literal beside an `i32`/`u32` scalar parameter takes the
/// parameter's width in both places. A float endpoint cannot join, so the
/// exact-type fallback still rejects it.
#[test]
fn sized_int_range_endpoints_join_like_host() {
    accepted(
        r"@ nucleum
@ publica
functio range_loop(tensor<f32, [8]> x, i32 n) → f32 {
    varia f32 acc ← 0.0
    itera ab 0‥n sit k {
        acc ← acc + x[[k]]
    }
    redde acc
}",
    );
    accepted(
        r"@ nucleum
@ publica
functio range_loop(tensor<f32, [8]> x, u32 n) → f32 {
    varia f32 acc ← 0.0
    si n ≤ 2147483647 {
        itera ab 0‥n sit k {
            acc ← acc + x[[k]]
        }
    }
    redde acc
}",
    );
    crate::test_support::assert_analysis_rejects_with_issue_la(
        r"@ nucleum
functio bad(i32 x, f32 f) → vacuum {
    varia f32 acc ← 0.0
    itera ab 0‥f sit i {
        acc ← acc
    }
}",
        "equality_exact_types_required",
    );
}
