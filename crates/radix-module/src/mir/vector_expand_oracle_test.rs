//! `radix_mir::vector_expand` against the lowerer's own vector lanes.
//!
//! The lowerer still builds vector lanes inline (VG-3 removes that). Until
//! then it is the oracle: for every op and width 2, 3, 4, lower a one-method
//! function, rebuild the same function with the method deferred as a single
//! `VectorOp`, expand it, and require the two to match lane for lane. The
//! comparison is the dump text with temps renumbered by first appearance, so
//! the allocation order of temps is free but every statement, operand, block
//! edge and type is pinned.

use super::lower::{MirLoweringOptions, dump_analyzed_unit, lower_analyzed_unit};
use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, AnalyzedModule, Config, Session};
use radix_lexer::Span;
use radix_mir::vector_expand::expand_vector_ops;
use radix_mir::{
    MirBinOp, MirBlock, MirBlockId, MirFunction, MirIntrinsic, MirLocalId, MirOperand, MirPlace, MirProgram,
    MirRuntimeCall, MirStatement, MirStatementKind, MirTemp, MirTempId, MirValueKind, MirVectorOp, dump::dump_program,
};

fn analyze(source: &str) -> AnalyzedModule {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()).expect("analysis succeeds")
}

fn source(width: usize, result: &str, call: &str) -> String {
    format!(
        "functio f(vf32[{width}] a, vf32[{width}] b) → {result} {{\n    fixum {result} r ← {call}\n    redde r\n}}\n"
    )
}

/// The oracle function the lowerer builds today.
fn old_function(unit: &AnalyzedModule) -> MirFunction {
    let lowered = lower_analyzed_unit(unit, MirLoweringOptions::default()).expect("lowers");
    let functions = &lowered.validated.program().functions;
    assert_eq!(functions.len(), 1);
    functions[0].clone()
}

/// Rebuild `old` with its inline vector lanes replaced by one deferred call.
///
/// The lanes end where `_2 = %T` (the `fixum r` binding) begins; the call
/// writes `%T`, exactly as the lowerer's runtime-call path will in VG-3.
fn deferred_function(old: &MirFunction, op: MirVectorOp, binary: bool) -> MirFunction {
    let last = old.blocks.last().expect("a block");
    let (index, result_temp) = last
        .statements
        .iter()
        .enumerate()
        .find_map(|(index, statement)| match &statement.kind {
            MirStatementKind::Assign { place, value } if *place == MirPlace::local(MirLocalId(2)) => {
                match &value.kind {
                    MirValueKind::Operand(MirOperand::Temp(temp)) => Some((index, *temp)),
                    _ => None,
                }
            }
            _ => None,
        })
        .expect("the `r` binding assigns a result temp");
    let return_ty = old.temp_ty(result_temp).expect("result temp type");

    let mut args = vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))];
    if binary {
        args.push(MirOperand::Place(MirPlace::local(MirLocalId(1))));
    }
    let mut statements = vec![MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: Some(MirPlace::temp(MirTempId(0))),
            call: MirRuntimeCall { intrinsic: MirIntrinsic::VectorOp(op), args, return_ty },
        },
        span: Span::default(),
    }];
    statements.extend(
        last.statements[index..]
            .iter()
            .cloned()
            .map(|mut statement| {
                retarget_temp(&mut statement, result_temp, MirTempId(0));
                statement
            }),
    );

    let mut terminator = last.terminator.clone();
    if let radix_mir::MirTerminatorKind::Return(Some(MirOperand::Temp(temp))) = &mut terminator.kind
        && *temp == result_temp
    {
        *temp = MirTempId(0);
    }
    let mut function = old.clone();
    function.temps = vec![MirTemp { id: MirTempId(0), ty: return_ty, span: Span::default() }];
    function.blocks = vec![MirBlock { id: MirBlockId(0), statements, terminator, span: Span::default() }];
    function
}

fn retarget_temp(statement: &mut MirStatement, from: MirTempId, to: MirTempId) {
    if let MirStatementKind::Assign { value, .. } = &mut statement.kind
        && let MirValueKind::Operand(MirOperand::Temp(temp)) = &mut value.kind
        && *temp == from
    {
        *temp = to;
    }
}

/// Dump text of one function: header, params and locals as printed, then the
/// block bodies with temps renumbered by first appearance, plus each canonical
/// temp's type.
fn canonical(function: &MirFunction) -> (String, Vec<String>) {
    let dump = dump_program(&MirProgram { functions: vec![function.clone()] });
    let mut temp_types = std::collections::BTreeMap::new();
    let mut in_temps = false;
    let mut header = Vec::new();
    let mut body = Vec::new();
    for line in dump.lines() {
        if line.starts_with("  bb") || !body.is_empty() {
            in_temps = false;
            body.push(line.to_owned());
        } else if line == "  temps:" {
            in_temps = true;
        } else if in_temps {
            let (id, ty) = line.trim().split_once(": ").expect("temp row");
            temp_types.insert(id.to_owned(), ty.to_owned());
        } else {
            header.push(line.to_owned());
        }
    }

    let mut order: Vec<String> = Vec::new();
    let renamed: Vec<String> = body
        .iter()
        .map(|line| {
            let mut out = String::new();
            let mut rest = line.as_str();
            while let Some(at) = rest.find('%') {
                out.push_str(&rest[..at]);
                let digits: String = rest[at + 1..]
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect();
                let name = format!("%{digits}");
                let position = order
                    .iter()
                    .position(|seen| *seen == name)
                    .unwrap_or_else(|| {
                        order.push(name.clone());
                        order.len() - 1
                    });
                out.push_str(&format!("%c{position}"));
                rest = &rest[at + 1 + digits.len()..];
            }
            out.push_str(rest);
            out
        })
        .collect();
    let types = order
        .iter()
        .enumerate()
        .map(|(position, name)| format!("%c{position}: {}", temp_types[name]))
        .collect();
    (format!("{}\n{}", header.join("\n"), renamed.join("\n")), types)
}

fn assert_matches_lowerer(width: usize, result: &str, call: &str, op: MirVectorOp, binary: bool) {
    let unit = analyze(&source(width, result, call));
    let old = old_function(&unit);
    let mut new = deferred_function(&old, op.clone(), binary);
    let errors = expand_vector_ops(&unit.types, &mut new);
    assert!(errors.is_empty(), "w{width} {op:?}: {errors:?}");
    assert_eq!(canonical(&new), canonical(&old), "w{width} {op:?}");
}

#[test]
fn elementwise_matches_the_lowerer_lane_for_lane() {
    for width in 2..=4 {
        let vector = format!("vf32[{width}]");
        for (method, op) in [
            ("addita", MirBinOp::Add),
            ("subtrahe", MirBinOp::Sub),
            ("multiplica", MirBinOp::Mul),
            ("divida", MirBinOp::Div),
        ] {
            assert_matches_lowerer(width, &vector, &format!("a.{method}(b)"), MirVectorOp::Elementwise(op), true);
        }
    }
}

#[test]
fn dot_matches_the_lowerer_lane_for_lane() {
    for width in 2..=4 {
        assert_matches_lowerer(width, "f32", "a.productum(b)", MirVectorOp::Dot, true);
    }
}

#[test]
fn length_matches_the_lowerer_lane_for_lane() {
    for width in 2..=4 {
        assert_matches_lowerer(width, "f32", "a.longitudo()", MirVectorOp::Length, false);
    }
}

#[test]
fn normalized_matches_the_lowerer_lane_for_lane() {
    for width in 2..=4 {
        let vector = format!("vf32[{width}]");
        assert_matches_lowerer(width, &vector, "a.normalizata()", MirVectorOp::Normalized, false);
    }
}

#[test]
fn crea_matches_the_lowerer_lane_for_lane() {
    for width in 2..=4 {
        let vector = format!("vf32[{width}]");
        assert_matches_lowerer(width, &vector, "a.crea()", MirVectorOp::Crea, false);
    }
}

#[test]
fn swizzle_matches_the_lowerer_lane_for_lane() {
    for width in 2..=4usize {
        let vector = format!("vf32[{width}]");
        let forward: String = "xyzw".chars().take(width).collect();
        let reversed: String = forward.chars().rev().collect();
        let rotated: String = forward.chars().cycle().skip(1).take(width).collect();
        for pattern in [forward, reversed, rotated] {
            let lanes = pattern
                .chars()
                .map(|lane| u8::try_from("xyzw".find(lane).expect("lane")).expect("lane index"))
                .collect();
            assert_matches_lowerer(
                width,
                &vector,
                &format!("a.swizzle(\"{pattern}\")"),
                MirVectorOp::Swizzle(lanes),
                false,
            );
        }
    }
}

#[test]
fn cross_matches_the_lowerer_at_width_three_and_both_refuse_other_widths() {
    assert_matches_lowerer(3, "vf32[3]", "a.transversum(b)", MirVectorOp::Cross, true);

    for width in [2, 4] {
        let vector = format!("vf32[{width}]");
        let unit = analyze(&source(width, &vector, "a.transversum(b)"));
        assert!(dump_analyzed_unit(&unit).is_err(), "the lowerer refuses cross at width {width}");

        // The expander refuses the same shape: borrow a width-3 lowering for
        // the scaffold and retype the receiver to the other width.
        let unit3 = analyze(&source(3, "vf32[3]", "a.transversum(b)"));
        let scaffold = deferred_function(&old_function(&unit3), MirVectorOp::Cross, true);
        let unit_w = analyze(&source(width, &vector, "a.crea()"));
        let old_w = old_function(&unit_w);
        let mut function = scaffold;
        function.params = old_w.params.clone();
        function.locals = old_w.locals.clone();
        function.temps[0].ty = old_w.temps[old_w.temps.len() - 1].ty;
        if let MirStatementKind::RuntimeCall { call, .. } = &mut function.blocks[0].statements[0].kind {
            call.return_ty = function.temps[0].ty;
        }
        let errors = expand_vector_ops(&unit_w.types, &mut function);
        assert_eq!(errors.len(), 1, "w{width}");
        assert_eq!(errors[0].issue, radix_mir::vector_expand::VECTOR_CROSS_WIDTH);
    }
}
