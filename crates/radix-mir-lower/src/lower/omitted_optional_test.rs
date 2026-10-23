//! Omitted `sponte` argument lowering pins.
//!
//! A bare `optional` parameter (no `vel` default) omitted at the call site is
//! `nihil`, but the argument still has to validate as the parameter's declared
//! slot. A callee the package linker resolves by MIR function id carries no
//! optional flags — the validator reads that signature from the MIR parameter
//! list alone — so a bare `nil` operand is a call-argument type mismatch. The
//! placeholder therefore lands in a slot of the declared parameter type.

use super::*;

const BARE_OPTIONAL: DefId = DefId(23);

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

    fn builder(&self) -> FunctionBuilder<'_> {
        let mut function_params = FxHashMap::default();
        function_params.insert(
            BARE_OPTIONAL,
            vec![context::CallableParamMeta { ty: Some(self.numerus), optional: true, default: None }],
        );
        let mut builder = FunctionBuilder::new(&self.types);
        builder.context = FunctionBuilderContext {
            interner: Some(&self.interner),
            function_params: Arc::new(function_params),
            ..FunctionBuilderContext::empty()
        };
        builder
    }
}

#[test]
fn omitted_optional_argument_keeps_the_parameter_type() {
    let fixture = Fixture::new();
    let mut builder = fixture.builder();

    let lowered = builder
        .lower_direct_call_args(BARE_OPTIONAL, &[], Span::default())
        .expect("an omitted bare-optional argument lowers");

    assert_eq!(lowered.len(), 1, "the omitted position still fills one operand");
    let MirOperand::Temp(temp) = lowered[0] else {
        panic!("the omitted argument is materialized in a typed slot, got {:?}", lowered[0]);
    };
    let slot = builder
        .temps
        .iter()
        .find(|candidate| candidate.id == temp)
        .unwrap_or_else(|| panic!("the omitted argument temp {temp:?} is declared"));
    assert_eq!(
        slot.ty.semantic_id(),
        fixture.numerus,
        "the omitted argument validates as the parameter's declared type",
    );
}
