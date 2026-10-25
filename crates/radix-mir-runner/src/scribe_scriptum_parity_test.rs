//! Parity between the Scribe diagnostic node and the scriptum (`§`) text path.
//!
//! Operator ruling 2026-10-17: `print x` rewrites to
//! `console.write_line("§"(x))`, so the two renderings of one value must be the
//! same text. The Scribe node renders through `Value::display_typed`
//! (`runtime.rs`, `MirIntrinsic::Diagnostic`); the scriptum lowering emits
//! `MirIntrinsic::FormatString` and renders through
//! `value::format_template_shaped`. Scribe output is the pinned expectation: a
//! mismatch is a defect on the scriptum side.
//!
//! Every case renders one runner [`Value`] twice — once through the Scribe node
//! (captured from the buffer host) and once through a single-hole `"§"`
//! template— and asserts the two strings agree with each other and, where the
//! language fixes the text, with the pinned literal.
//!
//! Locale words are baked at lowering time from the source locale. They reach
//! the runner as the printing function's `DisplayTokens` entry, which both
//! paths read through `field_order()`, so the matrix runs under both word sets:
//! the Latin constructor the runtime contract ships and the English words
//! (`true`/`false`/`none`/`tuple`) lowering bakes for an `en` function.
//!
//! Not covered: a dynamic `valor`/`json` carrier, and the reason is a real gap
//! rather than an untested family. `lower_scriptum` forwards its arguments
//! unchanged, so a `valor`-typed argument does reach the `FormatString` node as
//! the dynamic carrier; but only the Scribe node sets `FieldOrder::dynamic`
//! (`operand_is_dynamic`), the scriptum path never does, and
//! `format_template_shaped` takes no `dynamic` term at all. Threading it is a
//! `value.rs` signature change, outside this unit's write scope, so this matrix
//! pins the families the ruling names and the `dynamic` row is left as declared
//! debt: adding it here would fail until the flag is threaded on the scriptum
//! side.

use super::{MapKey, Runner, Value};
use crate::host::BufferHost;
use handroll::FxHashMap;
use radix_lexer::{Interner, Span};
use radix_mir::{
    MirBlockId, MirDiagnosticKind, MirFunction, MirFunctionId, MirIntrinsic, MirLocal, MirLocalId,
    MirOperand, MirPlace, MirProgram, MirRuntimeCall, MirType, MirValidationContext,
};
use radix_runtime_contract::display_tokens::DisplayTokens;
use radix_types::{Primitive, TypeId, TypeTable};
use std::cell::RefCell;
use std::rc::Rc;

/// One row of the display matrix.
struct Case {
    /// Name the failure message uses.
    what: &'static str,
    /// Declared type of the operand's local slot, which is what both paths read
    /// the display width and shape from.
    ty: TypeId,
    /// The runner value the slot holds.
    value: Value,
    /// Exact text both paths must render, as `(Latin words, English words)`.
    /// `None` where the language leaves the text open (a `tabula` has no
    /// defined order).
    expected: Option<(&'static str, &'static str)>,
}

/// The value families the ruling has to cover. Type ids come from `types`
/// because a declared slot type is what shapes the display.
fn cases(types: &mut TypeTable) -> Vec<Case> {
    let numerus = types.primitive(Primitive::Numerus);
    let fractus = types.primitive(Primitive::Fractus);
    let bivalens = types.primitive(Primitive::Bivalens);
    let textus = types.primitive(Primitive::Textus);
    let optio = types.option(numerus);
    let lista = types.array(numerus);
    let tabula = types.map(textus, numerus);
    let iuncta = types.tuple(vec![numerus, fractus]);
    let nidus = types.array(iuncta);

    let tuple_value = || Value::Tuple(vec![Value::Int(7), Value::Float(0.5)]);
    let nested = Value::Array(Rc::new(RefCell::new(vec![
        Value::Tuple(vec![Value::Int(7), Value::Float(0.5)]),
        Value::Tuple(vec![Value::Int(8), Value::Float(1.0)]),
    ])));

    vec![
        Case {
            what: "numerus",
            ty: numerus,
            value: Value::Int(42),
            expected: Some(("42", "42")),
        },
        Case {
            what: "fractus with a fraction",
            ty: fractus,
            value: Value::Float(1.5),
            expected: Some(("1.5", "1.5")),
        },
        Case {
            what: "fractus with a zero fraction",
            ty: fractus,
            value: Value::Float(2.0),
            expected: Some(("2.0", "2.0")),
        },
        Case {
            what: "infinite fractus",
            ty: fractus,
            value: Value::Float(f64::INFINITY),
            expected: Some(("inf", "inf")),
        },
        Case {
            what: "negatively infinite fractus",
            ty: fractus,
            value: Value::Float(f64::NEG_INFINITY),
            expected: Some(("-inf", "-inf")),
        },
        Case {
            what: "bivalens",
            ty: bivalens,
            value: Value::Bool(true),
            expected: Some(("verum", "true")),
        },
        Case {
            what: "textus",
            ty: textus,
            value: Value::Str("Aurelia".to_owned()),
            expected: Some(("Aurelia", "Aurelia")),
        },
        Case {
            what: "nihil (null option)",
            ty: optio,
            value: Value::Nil,
            expected: Some(("nihil", "none")),
        },
        Case {
            what: "lista",
            ty: lista,
            value: Value::Array(Rc::new(RefCell::new(vec![
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
            ]))),
            expected: Some(("[1, 2, 3]", "[1, 2, 3]")),
        },
        Case {
            // A `tabula` iterates in an unspecified order, so only the two
            // paths can be compared, never a literal.
            what: "tabula",
            ty: tabula,
            value: Value::Map(Rc::new(RefCell::new(FxHashMap::from_iter([
                (MapKey::Str("a".to_owned()), Value::Int(1)),
                (MapKey::Str("b".to_owned()), Value::Int(2)),
            ])))),
            expected: None,
        },
        Case {
            what: "iuncta (tuple)",
            ty: iuncta,
            value: tuple_value(),
            expected: Some(("iuncta [7, 0.5]", "tuple [7, 0.5]")),
        },
        Case {
            what: "lista of iuncta",
            ty: nidus,
            value: nested,
            expected: Some((
                "[iuncta [7, 0.5], iuncta [8, 1.0]]",
                "[tuple [7, 0.5], tuple [8, 1.0]]",
            )),
        },
    ]
}

/// Render one case through both paths: the Scribe node's line, captured from
/// the buffer host, and the scriptum template's rendered text.
fn render_both(case: &Case, types: &TypeTable, tokens: &DisplayTokens) -> (String, String) {
    let span = Span::default();
    let mut interner = Interner::new();
    let template = interner.intern("§");
    let vacuum = types.primitive(Primitive::Vacuum);
    let textus = types.primitive(Primitive::Textus);
    let program = MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: Vec::new(),
            locals: vec![MirLocal {
                id: MirLocalId(0),
                name: None,
                ty: MirType::semantic(case.ty),
                mutable: false,
                span,
            }],
            temps: Vec::new(),
            blocks: Vec::new(),
            return_ty: MirType::semantic(vacuum),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        }],
    };
    let mut context = MirValidationContext::new(types);
    context.display_tokens.assign(MirFunctionId(0), tokens);

    let mut host = BufferHost::default();
    let scriptum = {
        let mut runner = Runner::new(&program, &context, types, &interner, &mut host);
        runner.push_frame(MirFunctionId(0), MirBlockId(0));
        runner
            .frames
            .last_mut()
            .expect("frame")
            .seed_local(MirLocalId(0), case.value.clone());
        let operand = MirOperand::Place(MirPlace::local(MirLocalId(0)));
        runner
            .eval_runtime_call(
                &MirRuntimeCall {
                    intrinsic: MirIntrinsic::Diagnostic(MirDiagnosticKind::Scribe),
                    args: vec![operand.clone()],
                    return_ty: MirType::semantic(vacuum),
                },
                span,
            )
            .expect("the Scribe node renders the value");
        let rendered = runner
            .eval_runtime_call(
                &MirRuntimeCall {
                    intrinsic: MirIntrinsic::FormatString { template },
                    args: vec![operand],
                    return_ty: MirType::semantic(textus),
                },
                span,
            )
            .expect("the scriptum template renders the value");
        match rendered {
            Value::Str(text) => text,
            other => panic!("scriptum must render text, got {other:?}"),
        }
    };
    let scribed = host
        .stdout_lines
        .last()
        .cloned()
        .expect("the Scribe node wrote one stdout line");
    (scribed, scriptum)
}

#[test]
fn scribe_and_scriptum_render_every_family_alike() {
    let mut types = TypeTable::new();
    let cases = cases(&mut types);
    // The runtime contract ships the Latin constructor; lowering bakes the
    // English words into the same per-function table for an `en` function, so
    // both sets are reachable at this layer.
    let words = [
        ("la", DisplayTokens::latin()),
        (
            "en",
            DisplayTokens {
                true_: "true".to_owned(),
                false_: "false".to_owned(),
                none: "none".to_owned(),
                tuple: "tuple".to_owned(),
            },
        ),
    ];
    for case in &cases {
        for (index, (label, tokens)) in words.iter().enumerate() {
            let (scribed, scriptum) = render_both(case, &types, tokens);
            assert_eq!(
                scribed, scriptum,
                "{} rendered differently through the Scribe node and the scriptum template on the {label} words",
                case.what
            );
            if let Some((latin, english)) = case.expected {
                let want = if index == 0 { latin } else { english };
                assert_eq!(
                    scribed, want,
                    "{} scribe text on the {label} words",
                    case.what
                );
            }
        }
    }
}
