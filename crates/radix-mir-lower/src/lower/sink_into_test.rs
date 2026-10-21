//! MIR lowering pins for `sink ⇜ item` (need-5).
//!
//! `⇜` is the operator form of `sink.sink(item)` that dies on a failed write,
//! while the method form reports. The call therefore lowers to the sinking
//! contract's `sink` method inside one trap whose fail block panics instead of
//! recovering, and a channel's send view lowers to the sermo send intrinsic
//! the method twin (`m.da(x)`) already uses.
//!
//! The fixtures build the shape the analyzer hands lowering — a typed receiver
//! place, the item, and the method's declared error channel — and assert the
//! emitted MIR structure, never prose.

use super::*;
use radix_hir::HirId;
use radix_semantic::radix_contract::SINKING_METHOD;

const SINKER: DefId = DefId(11);
const SINKER_SINK: DefId = DefId(12);
const MEUS: DefId = DefId(13);
const OOPS: DefId = DefId(14);
const RECEIVER: DefId = DefId(15);
const RECEIVER_NAME: Symbol = Symbol(3);
const ITEM_TEXT: Symbol = Symbol(4);

struct Fixture {
    types: TypeTable,
    interner: Interner,
    sinker: TypeId,
    view: TypeId,
    oops: TypeId,
    textus: TypeId,
    vacuum: TypeId,
    sink: Symbol,
}

impl Fixture {
    fn new() -> Self {
        let mut types = TypeTable::new();
        let sinker = types.intern(Type::Struct(SINKER));
        let view = types.intern(Type::Struct(MEUS));
        let oops = types.intern(Type::Struct(OOPS));
        let textus = types.primitive(Primitive::Textus);
        let vacuum = types.primitive(Primitive::Vacuum);
        let mut interner = Interner::new();
        let sink = interner.intern(SINKING_METHOD);
        Self { types, interner, sinker, view, oops, textus, vacuum, sink }
    }

    /// A builder that knows the sink's `sink` method and its declared error
    /// channel, with one receiver parameter bound in the entry block.
    fn builder(&self, receiver_ty: TypeId, send_view: bool) -> FunctionBuilder<'_> {
        let mut function_errors = FxHashMap::default();
        function_errors.insert(SINKER_SINK, MirType::semantic(self.oops));
        let mut method_targets = FxHashMap::default();
        method_targets.insert((SINKER, self.sink), MethodTarget { def_id: SINKER_SINK });
        let mut builder = FunctionBuilder::new(&self.types);
        builder.context = FunctionBuilderContext {
            interner: Some(&self.interner),
            function_errors: Arc::new(function_errors),
            method_targets: Arc::new(method_targets),
            meus_def: send_view.then_some(MEUS),
            ..FunctionBuilderContext::empty()
        };
        builder.add_param(RECEIVER, RECEIVER_NAME, receiver_ty, HirParamMode::Owned, Span::default());
        let entry = builder.fresh_block(Span::default());
        builder.switch_to(entry);
        builder
    }

    fn receiver(&self, ty: TypeId) -> HirExpression {
        HirExpression { id: HirId(1), kind: HirExpressionKind::Path(RECEIVER), ty: Some(ty), span: Span::default() }
    }

    fn item(&self) -> HirExpression {
        HirExpression {
            id: HirId(2),
            kind: HirExpressionKind::Literal(HirLiteral::String(ITEM_TEXT)),
            ty: Some(self.textus),
            span: Span::default(),
        }
    }

    /// The lowered `sink ⇜ item` statement, `vacuum`-typed.
    fn statement(&self, sink: HirExpression, item: HirExpression) -> HirExpression {
        HirExpression {
            id: HirId(3),
            kind: HirExpressionKind::SinkInto { sink: Box::new(sink), item: Box::new(item) },
            ty: Some(self.vacuum),
            span: Span::default(),
        }
    }
}

/// The `TryCall` edge whose definition callee is the sink method, the block it
/// wraps the payload into, and the fail block that trap joins.
fn sink_trap(builder: &FunctionBuilder<'_>) -> Option<(usize, usize, usize)> {
    for (index, block) in builder.blocks.iter().enumerate() {
        let Some(terminator) = block.terminator.as_ref() else {
            continue;
        };
        if let MirTerminatorKind::TryCall { callee: MirCallee::Definition { source, .. }, error_block, .. } =
            &terminator.kind
            && *source == SINKER_SINK
        {
            let error_block = usize::try_from(error_block.0).expect("a block id fits the index space");
            let MirTerminatorKind::Goto(fail_block) = &builder.blocks[error_block].terminator.as_ref()?.kind else {
                return None;
            };
            return Some((
                index,
                error_block,
                usize::try_from(fail_block.0).expect("a block id fits the index space"),
            ));
        }
    }
    None
}

/// Whether `block` panics and then becomes unreachable: the die terminal.
fn dies(block: &OpenBlock) -> bool {
    let panics = block.statements.iter().any(|statement| {
        matches!(
            &statement.kind,
            MirStatementKind::RuntimeCall { call, .. } if matches!(call.intrinsic, MirIntrinsic::Panic)
        )
    });
    let unreachable = block
        .terminator
        .as_ref()
        .is_some_and(|terminator| matches!(terminator.kind, MirTerminatorKind::Unreachable));
    panics && unreachable
}

#[test]
fn sink_into_lowers_to_die_call() {
    let fixture = Fixture::new();
    let mut builder = fixture.builder(fixture.sinker, false);
    let sink = fixture.receiver(fixture.sinker);
    let item = fixture.item();
    let expr = fixture.statement(sink.clone(), item.clone());

    let lowered = builder.lower_sink_into(&sink, &item, &expr);

    assert!(lowered.is_some(), "sink-into lowers a vacuum statement: {:?}", builder.errors);
    let (call_block, error_block, fail_block) = sink_trap(&builder).expect("the sink method lowers to a TryCall edge");
    let args = match &builder.blocks[call_block]
        .terminator
        .as_ref()
        .expect("call terminator")
        .kind
    {
        MirTerminatorKind::TryCall { args, .. } => args.len(),
        _ => unreachable!("sink_trap only returns TryCall blocks"),
    };
    assert_eq!(args, 2, "the receiver and the item are both passed to `sink`");
    let wraps_payload = builder.blocks[error_block]
        .statements
        .iter()
        .any(|statement| matches!(statement.kind, MirStatementKind::Assign { .. }));
    assert!(wraps_payload, "the trap fail edge carries the error payload out of the call");
    assert!(
        dies(&builder.blocks[fail_block]),
        "the trap fail block dies: it panics on the payload and never recovers the failure"
    );
}

#[test]
fn sink_into_send_view_lowers_to_sermo_send() {
    let fixture = Fixture::new();
    let mut builder = fixture.builder(fixture.view, true);
    let sink = fixture.receiver(fixture.view);
    let item = fixture.item();
    let expr = fixture.statement(sink.clone(), item.clone());

    let lowered = builder.lower_sink_into(&sink, &item, &expr);

    assert!(lowered.is_some(), "the send-view sink lowers: {:?}", builder.errors);
    let send = builder
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .find_map(|statement| match &statement.kind {
            MirStatementKind::RuntimeCall { call, .. }
                if matches!(call.intrinsic, MirIntrinsic::Sermo(radix_mir::MirSermoOp::Send)) =>
            {
                Some(call)
            }
            _ => None,
        });
    let send = send.expect("a send-view receiver lowers to the sermo send op");
    assert_eq!(send.args.len(), 2, "the view and the item are both passed to the send op");
    assert!(
        builder
            .blocks
            .iter()
            .all(|block| !block.statements.iter().any(|statement| matches!(
                &statement.kind,
                MirStatementKind::RuntimeCall { call, .. } if matches!(call.intrinsic, MirIntrinsic::Panic)
            ))),
        "an infallible send op needs no die trap"
    );
}
