use crate::decode::{decode, encode};
use crate::test_support::stub_artifact;
use radix_hir::{
    HirBlock, HirExpression, HirExpressionKind, HirId, HirItemKind, HirLiteral, HirParam,
    HirParamMode, HirRangeKind,
};
use radix_lexer::{Span, Symbol};
use radix_types::{DefId, Type, TypeTable, USER_DEF_ID_BASE};

fn lit(id: u32, value: u64) -> HirExpression {
    HirExpression {
        id: HirId(id),
        kind: HirExpressionKind::Literal(HirLiteral::Int(value)),
        ty: None,
        span: Span::default(),
    }
}

fn stub_function(artifact: &mut crate::artifact::HirArtifact) -> &mut radix_hir::HirFunction {
    match &mut artifact.hir.items[0].kind {
        HirItemKind::Function(func) => func,
        _ => panic!("stub artifact item must be a function"),
    }
}

#[test]
fn decode_accepts_intervallum_child_hir_ids() {
    let mut artifact = stub_artifact();
    let body = HirBlock::simple(
        Vec::new(),
        Some(Box::new(HirExpression {
            id: HirId(1),
            kind: HirExpressionKind::Intervallum {
                start: Box::new(lit(2, 0)),
                end: Box::new(lit(3, 10)),
                step: Some(Box::new(lit(4, 1))),
                kind: HirRangeKind::Exclusive,
            },
            ty: None,
            span: Span::default(),
        })),
        Span::default(),
    );
    stub_function(&mut artifact).body = Some(body);

    let bytes = encode(&artifact).expect("encode artifact");
    decode(&bytes).expect("intervallum child HirIds must be in the registry");
}

#[test]
fn decode_accepts_def_sites_inside_parameter_default() {
    let mut artifact = stub_artifact();
    let mut types = TypeTable::new();
    let ty = types.intern(Type::Error);
    artifact.types = types.snapshot();

    let closure = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::Clausura(
            vec![HirParam {
                def_id: DefId(USER_DEF_ID_BASE + 2),
                name: Symbol(1),
                ty,
                mode: HirParamMode::Owned,
                optional: false,
                sponte: false,
                default: None,
                span: Span::default(),
            }],
            None,
            None,
            Box::new(lit(2, 0)),
        ),
        ty: None,
        span: Span::default(),
    };
    stub_function(&mut artifact).params.push(HirParam {
        def_id: DefId(USER_DEF_ID_BASE + 1),
        name: Symbol(0),
        ty,
        mode: HirParamMode::Owned,
        optional: true,
        sponte: false,
        default: Some(closure),
        span: Span::default(),
    });

    let bytes = encode(&artifact).expect("encode artifact");
    decode(&bytes).expect("closure params in a default must register as def sites");
}
