use super::{
    SwitchArmLines, SwitchArmStore, body_indent, if_else_lines, if_else_store, indexed_switch,
    loop_control_body_lines, loop_local_assign, nested_branch_body_lines,
    nested_control_body_lines, scalar_loop_name, scalar_switch, value_join, value_join_name,
    while_loop,
};

/// MSL statement indent (Metal emitter profile).
const MSL: &str = "    ";
/// WGSL statement indent (WGSL emitter profile).
const WGSL: &str = "  ";

/// The output-store line both device backends render for a frame store; the
/// callback seam exists so a backend may lawfully diverge (WGSL f16 stores).
fn plain_store(expr: &str) -> String {
    format!("out[id] = {expr};")
}

#[test]
fn value_join_name_binds_the_shared_claim_law() {
    assert_eq!(value_join_name("x", "output", "id"), "faber_joined");
    // A claimed first candidate skips to the next, and a fully claimed set
    // falls back — the binding lives here, once, for both backends.
    assert_eq!(
        value_join_name("faber_joined", "output", "id"),
        "faber_joined_value"
    );
    assert_eq!(
        value_join_name("x", "faber_joined", "faber_joined_value"),
        "faber_joined_scalar"
    );
    // Full claim: all three candidates shadowed by (param, buffer, index).
    assert_eq!(
        value_join_name("faber_joined", "faber_joined_value", "faber_joined_scalar"),
        "faber_joined_result"
    );
}

#[test]
fn scalar_loop_name_binds_the_shared_claim_law() {
    assert_eq!(scalar_loop_name("x", "output", "id"), "faber_loop");
    assert_eq!(
        scalar_loop_name("faber_loop", "output", "id"),
        "faber_loop_value"
    );
    assert_eq!(
        scalar_loop_name("faber_loop", "faber_loop_value", "faber_loop_scalar"),
        "faber_loop_result"
    );
}

#[test]
fn if_else_store_frames_match_both_indent_profiles() {
    assert_eq!(
        if_else_store("ready", "a", "b", MSL, plain_store),
        "    if (ready) {\n        out[id] = a;\n    } else {\n        out[id] = b;\n    }"
    );
    assert_eq!(
        if_else_store("ready", "a", "b", WGSL, plain_store),
        "  if (ready) {\n    out[id] = a;\n  } else {\n    out[id] = b;\n  }"
    );
}

#[test]
fn if_else_lines_frames_match_both_indent_profiles() {
    let then = vec!["        out[id] = a;".to_owned()];
    let else_lines = vec!["        out[id] = b;".to_owned()];
    assert_eq!(
        if_else_lines("ready", &then, &else_lines, MSL),
        "    if (ready) {\n        out[id] = a;\n    } else {\n        out[id] = b;\n    }"
    );
    let then_w = vec!["    out[id] = a;".to_owned()];
    let else_w = vec!["    out[id] = b;".to_owned()];
    assert_eq!(
        if_else_lines("ready", &then_w, &else_w, WGSL),
        "  if (ready) {\n    out[id] = a;\n  } else {\n    out[id] = b;\n  }"
    );
}

#[test]
fn indexed_switch_chains_arms_with_single_space_separators() {
    let cases = vec![
        SwitchArmLines {
            condition: "(x == 1)".to_owned(),
            lines: vec!["        out[id] = a;".to_owned()],
        },
        SwitchArmLines {
            condition: "(x == 2)".to_owned(),
            lines: vec!["        out[id] = b;".to_owned()],
        },
    ];
    let default_lines = vec!["        out[id] = d;".to_owned()];
    // The original separator law: a pushed space before each non-first arm,
    // whose text itself carries the full indent — so `}` + 5 spaces in MSL.
    assert_eq!(
        indexed_switch(&cases, &default_lines, MSL),
        "    if ((x == 1)) {\n        out[id] = a;\n    }     else if ((x == 2)) {\n        out[id] = b;\n    } else {\n        out[id] = d;\n    }"
    );

    let cases_w = vec![SwitchArmLines {
        condition: "(x == 1)".to_owned(),
        lines: vec!["    out[id] = a;".to_owned()],
    }];
    let default_w = vec!["    out[id] = d;".to_owned()];
    assert_eq!(
        indexed_switch(&cases_w, &default_w, WGSL),
        "  if ((x == 1)) {\n    out[id] = a;\n  } else {\n    out[id] = d;\n  }"
    );
}

#[test]
fn scalar_switch_stores_each_arm_expression() {
    let cases = vec![
        SwitchArmStore {
            condition: "(x == 1)".to_owned(),
            expr: "a".to_owned(),
        },
        SwitchArmStore {
            condition: "(x == 2)".to_owned(),
            expr: "b".to_owned(),
        },
    ];
    // Same separator law as the indexed switch: `}` + 5 spaces in MSL.
    assert_eq!(
        scalar_switch(&cases, "d", MSL, plain_store),
        "    if ((x == 1)) {\n        out[id] = a;\n    }     else if ((x == 2)) {\n        out[id] = b;\n    } else {\n        out[id] = d;\n    }"
    );
    assert_eq!(
        scalar_switch(&cases[..1], "d", WGSL, plain_store),
        "  if ((x == 1)) {\n    out[id] = a;\n  } else {\n    out[id] = d;\n  }"
    );
}

#[test]
fn while_loop_frames_match_both_indent_profiles() {
    let body = vec!["        faber_loop = (faber_loop - 1.0f);".to_owned()];
    assert_eq!(
        while_loop(
            "float faber_loop = x;",
            "(faber_loop > 0.0f)",
            &body,
            Some("faber_loop"),
            MSL,
            plain_store
        ),
        "    float faber_loop = x;\n    while ((faber_loop > 0.0f)) {\n        faber_loop = (faber_loop - 1.0f);\n    }\n    out[id] = faber_loop;"
    );

    let body_w = vec!["    faber_loop = (faber_loop - 1.0f);".to_owned()];
    assert_eq!(
        while_loop(
            "var faber_loop: f32 = x;",
            "(faber_loop > 0.0f)",
            &body_w,
            None,
            WGSL,
            plain_store
        ),
        "  var faber_loop: f32 = x;\n  while ((faber_loop > 0.0f)) {\n    faber_loop = (faber_loop - 1.0f);\n  }"
    );
}

#[test]
fn value_join_frames_match_both_indent_profiles() {
    assert_eq!(
        value_join(
            "float faber_joined;",
            "faber_joined",
            "ready",
            "a",
            "b",
            MSL,
            plain_store
        ),
        "    float faber_joined;\n    if (ready) {\n        faber_joined = a;\n    } else {\n        faber_joined = b;\n    }\n    out[id] = faber_joined;"
    );
    assert_eq!(
        value_join(
            "var faber_joined: f32;",
            "faber_joined",
            "ready",
            "a",
            "b",
            WGSL,
            plain_store
        ),
        "  var faber_joined: f32;\n  if (ready) {\n    faber_joined = a;\n  } else {\n    faber_joined = b;\n  }\n  out[id] = faber_joined;"
    );
}

#[test]
fn loop_control_body_lines_match_both_indent_profiles() {
    assert_eq!(
        loop_control_body_lines("(faber_loop < 2.0f)", "break", &[], MSL),
        vec![
            "        if ((faber_loop < 2.0f)) {".to_owned(),
            "            break;".to_owned(),
            "        }".to_owned(),
        ]
    );
    let continuation = vec!["    faber_loop = (faber_loop - 1.0);".to_owned()];
    assert_eq!(
        loop_control_body_lines("(faber_loop < 2.0)", "continue", &continuation, WGSL),
        vec![
            "    if ((faber_loop < 2.0)) {".to_owned(),
            "      continue;".to_owned(),
            "    }".to_owned(),
            "    faber_loop = (faber_loop - 1.0);".to_owned(),
        ]
    );
}

#[test]
fn nested_branch_body_lines_reindent_then_lines_one_unit() {
    // The original re-indent law: a then-line produced by `loop_body_lines`
    // at loop-body level (two units) gains one more unit inside the nested
    // branch — it is a prefix, not a re-indent to a fixed level.
    let then = vec!["        faber_loop = a;".to_owned()];
    let continuation = vec!["        faber_loop = b;".to_owned()];
    assert_eq!(
        nested_branch_body_lines("(faber_loop > 2.0)", &then, &continuation, MSL),
        vec![
            "        if ((faber_loop > 2.0)) {".to_owned(),
            "            faber_loop = a;".to_owned(),
            "        }".to_owned(),
            "        faber_loop = b;".to_owned(),
        ]
    );
    let then_w = vec!["    faber_loop = a;".to_owned()];
    assert_eq!(
        nested_branch_body_lines("(faber_loop > 2.0)", &then_w, &[], WGSL),
        vec![
            "    if ((faber_loop > 2.0)) {".to_owned(),
            "      faber_loop = a;".to_owned(),
            "    }".to_owned(),
        ]
    );
}

#[test]
fn nested_control_body_lines_match_both_indent_profiles() {
    let update = vec!["        faber_loop = u;".to_owned()];
    let continuation = vec!["        faber_loop = c;".to_owned()];
    assert_eq!(
        nested_control_body_lines("(o)", "(i)", "break", &update, &continuation, MSL),
        vec![
            "        if ((o)) {".to_owned(),
            "            if ((i)) {".to_owned(),
            "                break;".to_owned(),
            "            }".to_owned(),
            "            faber_loop = u;".to_owned(),
            "        }".to_owned(),
            "        faber_loop = c;".to_owned(),
        ]
    );
    let update_w = vec!["    faber_loop = u;".to_owned()];
    let continuation_w = vec!["    faber_loop = c;".to_owned()];
    assert_eq!(
        nested_control_body_lines("(o)", "(i)", "continue", &update_w, &continuation_w, WGSL),
        vec![
            "    if ((o)) {".to_owned(),
            "      if ((i)) {".to_owned(),
            "        continue;".to_owned(),
            "      }".to_owned(),
            "      faber_loop = u;".to_owned(),
            "    }".to_owned(),
            "    faber_loop = c;".to_owned(),
        ]
    );
}

#[test]
fn loop_local_assign_and_body_indent_match_both_indent_profiles() {
    assert_eq!(
        loop_local_assign("faber_loop", "x", MSL),
        "        faber_loop = x;"
    );
    assert_eq!(
        loop_local_assign("faber_loop", "x", WGSL),
        "    faber_loop = x;"
    );
    assert_eq!(body_indent(MSL), "        ");
    assert_eq!(body_indent(WGSL), "    ");
}
