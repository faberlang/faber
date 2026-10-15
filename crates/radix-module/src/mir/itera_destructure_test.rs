//! DB-U4: itera destructuring follows binding-position MIR lowering.
//!
//! The semantic layer expands a pattern binder into ordinary locals before MIR
//! lowering. These tests pin that seam: the per-element projection shape is
//! the same as the corresponding binding-position shape, and representative
//! list, tuple, and record elements validate and execute in the runner.

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::mir::MirLoweringOptions;
use crate::mir::{
    BufferHost, MirConstant, MirOperand, MirPlace, MirPlaceBase, MirProjection, MirValue, MirValueKind, RunSourceError,
    lower_analyzed_unit, run_source, validate_program,
};
use std::fmt::Write;

fn session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    )
}

fn initializer_shape(source: &str, function_name: &str, names: &[&str]) -> Vec<String> {
    let unit = analyze_source(&session(), "itera-destructure.fab", source, AnalyzeRequest::default())
        .expect("fixture analyzes");
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("fixture lowers to MIR");
    let function = lowered
        .validated
        .program()
        .functions
        .iter()
        .find(|function| {
            function
                .name
                .is_some_and(|name| lowered.interner.resolve(name) == function_name)
        })
        .unwrap_or_else(|| panic!("function {function_name} is present"));

    names
        .iter()
        .map(|name| {
            let local = function
                .locals
                .iter()
                .find(|local| {
                    local
                        .name
                        .is_some_and(|symbol| lowered.interner.resolve(symbol) == *name)
                })
                .unwrap_or_else(|| panic!("local {name} is present"));
            let value = function
                .blocks
                .iter()
                .flat_map(|block| block.statements.iter())
                .find_map(|statement| {
                    let crate::mir::MirStatementKind::Assign { place, value } = &statement.kind else {
                        return None;
                    };
                    if place.base == MirPlaceBase::Local(local.id) && place.projections.is_empty() {
                        Some(value)
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| panic!("local {name} receives a MIR initializer"));
            value_shape(value, &lowered.interner)
        })
        .collect()
}

fn value_shape(value: &MirValue, interner: &crate::lexer::Interner) -> String {
    match &value.kind {
        MirValueKind::Operand(operand) => operand_shape(operand, interner),
        other => format!("value:{other:?}"),
    }
}

fn operand_shape(operand: &MirOperand, interner: &crate::lexer::Interner) -> String {
    match operand {
        MirOperand::Place(place) => place_shape(place, interner),
        MirOperand::Constant(MirConstant::Int(value)) => format!("const-int:{value}"),
        MirOperand::Constant(constant) => format!("const:{constant:?}"),
        MirOperand::Temp(_) => "temp".to_owned(),
        MirOperand::Value(_) => "value".to_owned(),
    }
}

fn place_shape(place: &MirPlace, interner: &crate::lexer::Interner) -> String {
    let mut shape = match place.base {
        MirPlaceBase::Local(_) => "local".to_owned(),
        MirPlaceBase::Temp(_) => "temp".to_owned(),
    };
    for projection in &place.projections {
        match projection {
            MirProjection::Field(field) => {
                let _ = write!(shape, ".field({})", interner.resolve(*field));
            }
            MirProjection::Index(index) => {
                let _ = write!(shape, ".index({})", operand_shape(index, interner));
            }
            MirProjection::VariantField { field, .. } => {
                let _ = write!(shape, ".variant-field({})", interner.resolve(*field));
            }
            other => {
                let _ = write!(shape, ".projection({other:?})");
            }
        }
    }
    shape
}

fn assert_lowered_and_valid(source: &str) {
    let unit = analyze_source(&session(), "itera-destructure.fab", source, AnalyzeRequest::default())
        .expect("fixture analyzes");
    let lowered =
        lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()).expect("fixture lowers to MIR");
    validate_program(lowered.validated.program(), lowered.validated.validation()).expect("fixture MIR validates");
}

fn run(source: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    match run_source(&session(), "itera-destructure.fab", source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Frontend(errors)) => panic!("fixture frontend errors: {errors:?}"),
        Err(RunSourceError::Mir(errors)) => panic!("fixture MIR errors: {errors:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("fixture runner errors: {errors:?}"),
    }
}

#[test]
fn loop_binder_mir_matches_binding_position_for_each_pattern_form() {
    let cases = vec![
        (
            "array positional",
            r"
functio bind(lista<numerus> row) → vacuum {
    fixum [a, b] ← row
    tacet
}
",
            r"
functio loopum(lista<lista<numerus>> rows) → vacuum {
    itera ex rows fixum [a, b] {
        tacet
    }
}
",
            "bind",
            "loopum",
            vec!["a", "b"],
        ),
        (
            "object by-label",
            r"
genus Point {
    varia numerus x = 0
    varia numerus y = 0
}

functio bind(Point point) → vacuum {
    fixum {x ut left, y} ← point
    tacet
}
",
            r"
genus Point {
    varia numerus x = 0
    varia numerus y = 0
}

functio loopum(lista<Point> points) → vacuum {
    itera ex points fixum {x ut left, y} {
        tacet
    }
}
",
            "bind",
            "loopum",
            vec!["left", "y"],
        ),
        (
            "array rest",
            r"
functio bind(lista<numerus> row) → vacuum {
    fixum [head, ceteri tail] ← row
    tacet
}
",
            r"
functio loopum(lista<lista<numerus>> rows) → vacuum {
    itera ex rows fixum [head, ceteri tail] {
        tacet
    }
}
",
            "bind",
            "loopum",
            vec!["head", "tail"],
        ),
        (
            "array underscore",
            r"
functio bind(lista<numerus> row) → vacuum {
    fixum [_, value] ← row
    tacet
}
",
            r"
functio loopum(lista<lista<numerus>> rows) → vacuum {
    itera ex rows fixum [_, value] {
        tacet
    }
}
",
            "bind",
            "loopum",
            vec!["value"],
        ),
    ];

    for (label, binding_source, loop_source, binding_name, loop_name, names) in cases {
        let binding = initializer_shape(binding_source, binding_name, &names);
        let looping = initializer_shape(loop_source, loop_name, &names);
        assert_eq!(looping, binding, "{label} loop and binding MIR projection shapes differ");
    }
}

#[test]
fn itera_ex_rest_over_lista_of_listax_validates_and_runs() {
    let source = r"
functio sum_rest(lista<lista<numerus>> rows) → numerus {
    varia numerus total ← 0
    itera ex rows fixum [head, ceteri tail] {
        total ← total + head
        total ← total + tail[0]
    }
    redde total
}

incipit {
    sit lista<lista<numerus>> rows ← [[1, 2, 3], [4, 5, 6]]
    nota sum_rest(rows)
}
";
    assert_lowered_and_valid(source);
    // D14.1: `tail` is the tail after `head`, so this is (1 + 2) + (4 + 5).
    assert_eq!(run(source), ["12"]);
}

#[test]
fn itera_ex_over_lista_of_tuples_validates_and_runs() {
    let source = r"
functio sum_pairs(lista<iuncta<numerus, numerus>> pairs) → numerus {
    varia numerus total ← 0
    itera ex pairs fixum [left, right] {
        total ← total + left + right
    }
    redde total
}

incipit {
    sit lista<iuncta<numerus, numerus>> pairs ← [
        iuncta<numerus, numerus> [1, 2],
        iuncta<numerus, numerus> [3, 4]
    ]
    nota sum_pairs(pairs)
}
";
    assert_lowered_and_valid(source);
    assert_eq!(run(source), ["10"]);
}

#[test]
fn itera_ex_over_lista_of_records_validates_and_runs() {
    let source = r"
genus Point {
    varia numerus x = 0
    varia numerus y = 0
}

functio sum_points(lista<Point> points) → numerus {
    varia numerus total ← 0
    itera ex points fixum {x, y} {
        total ← total + x + y
    }
    redde total
}

incipit {
    sit lista<Point> points ← [
        Point { x = 1, y = 2 },
        Point { x = 3, y = 4 }
    ]
    nota sum_points(points)
}
";
    assert_lowered_and_valid(source);
    assert_eq!(run(source), ["10"]);
}
