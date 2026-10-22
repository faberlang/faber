//! Imported callable default-argument lowering pins.
//!
//! Package-linked callees have synthetic definition ids that are not present
//! in the caller unit's parameter map. These tests ensure the imported metadata
//! supplies omitted defaults and bare optional values while preserving explicit
//! call-site arguments.

use super::*;
use radix_hir::HirId;

const WITH_DEFAULT: DefId = DefId(21);
const BARE_OPTIONAL: DefId = DefId(22);

struct Fixture {
    types: TypeTable,
    interner: Interner,
    numerus: TypeId,
}

impl Fixture {
    fn new() -> Self {
        let types = TypeTable::new();
        let numerus = types.primitive(Primitive::Numerus);
        let interner = Interner::new();
        Self { types, interner, numerus }
    }

    fn literal(&self, value: u64, id: u32) -> HirExpression {
        HirExpression {
            id: HirId(id),
            kind: HirExpressionKind::Literal(HirLiteral::Int(value)),
            ty: Some(self.numerus),
            span: Span::default(),
        }
    }

    fn builder(&self) -> FunctionBuilder<'_> {
        let mut imported_function_params = FxHashMap::default();
        imported_function_params.insert(
            WITH_DEFAULT,
            vec![ImportedCallableParam { optional: true, default: Some(self.literal(5, 1)) }],
        );
        imported_function_params.insert(BARE_OPTIONAL, vec![ImportedCallableParam { optional: true, default: None }]);
        let mut builder = FunctionBuilder::new(&self.types);
        builder.context = FunctionBuilderContext {
            interner: Some(&self.interner),
            imported_function_params: Arc::new(imported_function_params),
            ..FunctionBuilderContext::empty()
        };
        builder
    }
}

#[test]
fn imported_default_fills_the_omitted_argument() {
    let fixture = Fixture::new();
    let mut builder = fixture.builder();

    let lowered = builder.lower_direct_call_args(WITH_DEFAULT, &[], Span::default());

    assert_eq!(
        lowered,
        Some(vec![MirOperand::Constant(MirConstant::Int(5))]),
        "the imported default expression fills the omitted position",
    );
}

#[test]
fn imported_default_nil_for_bare_optional() {
    let fixture = Fixture::new();
    let mut builder = fixture.builder();

    let lowered = builder.lower_direct_call_args(BARE_OPTIONAL, &[], Span::default());

    assert_eq!(
        lowered,
        Some(vec![MirOperand::Constant(MirConstant::Nil)]),
        "an imported optional parameter without a default lowers to nil",
    );
}

#[test]
fn imported_default_explicit_argument_wins() {
    let fixture = Fixture::new();
    let mut builder = fixture.builder();
    let span = Span::default();
    let args = [HirCallArg { name: None, spread: false, marker: None, expr: fixture.literal(1, 2), span }];

    let lowered = builder.lower_direct_call_args(WITH_DEFAULT, &args, span);

    assert_eq!(
        lowered,
        Some(vec![MirOperand::Constant(MirConstant::Int(1))]),
        "an explicit call argument takes precedence over the imported default",
    );
}
