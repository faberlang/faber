//! Conservative wrapping-store region analysis.
//!
//! The store rule (numeric model §1, items 2–3): arithmetic computes the
//! exact value and a sized cell applies its policy once, where the value
//! lands — nothing traps mid-expression. A chain of arithmetic temporaries
//! whose only observation is one explicit store into a plain `wrapping<W>`
//! cell may therefore be reduced in the ring ℤ/2^W throughout: the
//! reduction homomorphism ℤ → ℤ/2^W commutes with the admitted operations,
//! and the store applies it exactly once regardless. The MIR backends that
//! carry exact (wide) integer temporaries with per-operation carrier checks
//! used to trap on this shape (the swarm's G03, `(z ⊻ (z ⇒ 30)) * m` into a
//! `wrapping<u64>`); this analysis hands them the set of temporaries they
//! may compute at word width instead.
//!
//! WHY: per-function facts, not a rewrite. The analysis claims nothing about
//! the source language — no typing rule changes, no MIR is rewritten — it
//! only reports region membership, and every refusal simply leaves a
//! temporary exact.
//!
//! **The conservative stops** (each is a refusal: the temporary stays exact):
//!
//! - *Same-block chains only.* The store statement and the defining
//!   statement of every claimed temporary live in one basic block, def before
//!   use. A chain crossing a block edge is unsupported control flow.
//! - *Single consumer, one definition.* A claimed temporary is read exactly
//!   once in the whole function (the chain edge) and written exactly once.
//!   Any second read — a call argument, a diagnostic, an aggregate field, a
//!   terminator operand, an index projection, a `MirOperand::Value` alias of
//!   its defining value — is an observation boundary; a second write is a
//!   redefinition.
//! - *Projected writes stop the store.* Both store forms are refused when
//!   their destination place carries projections (a field or element write,
//!   not a whole-cell landing), and a projected read of a temporary is never
//!   a chain edge.
//! - *Only operations that commute with reduction.* `+ - *`, the bitwise
//!   pair operations on two's complement, `⇦` (multiplication by 2ⁿ), unary
//!   `-` and `¬` preserve the value mod 2^W. `/`, `%` (floor on the exact
//!   value) and `⇒` (`⌊x/2ⁿ⌋`) do not, so a temporary defined by one is a
//!   terminal leaf: it stays exact, and the chain does not continue through
//!   its operands (claiming them would change what the non-commuting
//!   operation sees).
//! - *Word-typed and non-integer temporaries are terminal.* A temporary
//!   typed `wrapping<W'>`, `saturating<W'>`, a ranged cell, raw bits, a
//!   decimal or a float carries its own policy or family semantics in its
//!   operation; reducing it under the destination would skip those. Only the
//!   exact integer family (`numerus`, sized `numerus<W>`, `inf`) joins.
//! - *Plain word destinations only.* A ranged wrapping cell
//!   (`wrapping<W, lo‥hi>`) reduces modulo `hi − lo + 1` offset by `lo`,
//!   which does not commute with arithmetic; `wrapping<inf>` has no width to
//!   reduce at. Both are refused as destinations.
//!
//! EDGE: `⇦`'s count operand stays exact. The shift count is a semantic
//! parameter (`x·2ⁿ` on the exact value), not a ring element: an exact count
//! of 2⁶⁴+5 reduces to 5 at word width and shifts differently, so the count
//! never joins the chain even when nothing else reads it.

use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirBinOp, MirBlock, MirBlockId, MirCallee,
    MirFunction, MirIntrinsic, MirOperand, MirOptionChainLink, MirOptionOp, MirPlace, MirPlaceBase,
    MirProjection, MirRuntimeCall, MirStatement, MirStatementKind, MirTempId, MirTerminatorKind,
    MirUnOp, MirValue, MirValueId, MirValueKind, local_ty,
};
use handroll::{FxHashMap, FxHashSet};
use radix_types::{NumericWidth, Primitive, Type, TypeId, TypeTable};

/// One explicit wrapping store and the arithmetic temporaries that may be
/// reduced under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappingStoreRegion {
    /// The block holding the store and every claimed chain member.
    pub block: MirBlockId,
    /// The statement index of the store within that block.
    pub statement: usize,
    /// Which MIR statement form the store takes.
    pub site: WrappingStoreSite,
    /// The destination word width `W`: every claimed temporary may be
    /// computed in the ring ℤ/2^W.
    pub width: NumericWidth,
    /// The claimed arithmetic temporaries, in walk order from the store.
    /// Each is defined in [`Self::block`] before its single consumer by an
    /// operation that commutes with reduction mod 2^W.
    pub temps: Vec<MirTempId>,
}

/// The MIR statement form of one explicit wrapping store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrappingStoreSite {
    /// A store-conversion node: [`MirIntrinsic::Convert`] carrying a
    /// `store` site whose target is a plain `wrapping<W>` cell (a
    /// declaration, assignment, argument, return or inc/dec store).
    Conversion,
    /// A plain assignment of one unconverted operand into an unprojected
    /// `wrapping<W>` slot.
    SlotAssign,
}

/// The per-function wrapping-store regions, in block and statement order.
///
/// A store with no reducible chain (a constant or copy landing, a
/// word-typed top operation, a refused chain) yields no region: the fact is
/// only useful when at least one temporary may be reduced.
#[must_use]
pub fn wrapping_store_regions(
    function: &MirFunction,
    types: &TypeTable,
) -> Vec<WrappingStoreRegion> {
    let uses = TempUses::collect(function);
    let mut regions = Vec::new();
    for block in &function.blocks {
        for (index, statement) in block.statements.iter().enumerate() {
            let Some((site, width, root)) = store_site(function, types, statement) else {
                continue;
            };
            let temps = walk_chain(types, &uses, block, index, root);
            if temps.is_empty() {
                continue;
            }
            regions.push(WrappingStoreRegion {
                block: block.id,
                statement: index,
                site,
                width,
                temps,
            });
        }
    }
    regions
}

/// The destination width a temporary may be reduced under, when any region
/// claims it. Claims are disjoint (a claimed temporary has exactly one
/// reader, the claiming chain), so the first claim answers.
#[must_use]
pub fn wrapping_reduce_width(
    regions: &[WrappingStoreRegion],
    temp: MirTempId,
) -> Option<NumericWidth> {
    regions
        .iter()
        .find(|region| region.temps.contains(&temp))
        .map(|region| region.width)
}

/// The store form of one statement: the site kind, the destination width,
/// and the operand whose chain may be reduced under it.
fn store_site<'a>(
    function: &MirFunction,
    types: &TypeTable,
    statement: &'a MirStatement,
) -> Option<(WrappingStoreSite, NumericWidth, &'a MirOperand)> {
    match &statement.kind {
        MirStatementKind::RuntimeCall { destination, call } => {
            conversion_store_site(types, destination.as_ref(), call)
        }
        MirStatementKind::Assign { place, value } => {
            assign_store_site(function, types, place, value)
        }
        MirStatementKind::Call { .. } | MirStatementKind::Construct { .. } => None,
    }
}

/// The store-conversion form: a `Convert` with a `store` site into a plain
/// `wrapping<W>` target, one argument, unprojected destination. The chain
/// root is borrowed from the call's argument list.
fn conversion_store_site<'a>(
    types: &TypeTable,
    destination: Option<&'a MirPlace>,
    call: &'a MirRuntimeCall,
) -> Option<(WrappingStoreSite, NumericWidth, &'a MirOperand)> {
    let MirIntrinsic::Convert(conversion) = &call.intrinsic else {
        return None;
    };
    // WHY: `store` names the slot whose policy applies here; without it the
    // node is an explicit `↦` or a proven-fit cast, not a store.
    conversion.store.as_ref()?;
    let width = modular_word_width(types, conversion.target_ty.semantic_id())?;
    let [value] = call.args.as_slice() else {
        return None;
    };
    // EDGE: a projected destination writes a field or element, not a whole
    // cell; a missing destination is not a landing at all.
    match destination {
        Some(place) if place.projections.is_empty() => {}
        _ => return None,
    }
    Some((WrappingStoreSite::Conversion, width, value))
}

/// The plain-assignment form: an unprojected `wrapping<W>` slot receiving
/// one unconverted operand. The chain root is borrowed from the value.
fn assign_store_site<'a>(
    function: &MirFunction,
    types: &TypeTable,
    place: &MirPlace,
    value: &'a MirValue,
) -> Option<(WrappingStoreSite, NumericWidth, &'a MirOperand)> {
    if !place.projections.is_empty() {
        return None;
    }
    let width = match place.base {
        MirPlaceBase::Local(local) => {
            modular_word_width(types, local_ty::local_ty(function, local)?.semantic_id())?
        }
        MirPlaceBase::Temp(temp) => {
            modular_word_width(types, function.temp_ty(temp)?.semantic_id())?
        }
    };
    let MirValueKind::Operand(operand) = &value.kind else {
        return None;
    };
    Some((WrappingStoreSite::SlotAssign, width, operand))
}

/// Walk the chain behind one store root and claim every temporary that may
/// be reduced under the destination. Refusals leave the temporary exact and
/// stop that branch of the walk.
fn walk_chain(
    types: &TypeTable,
    uses: &TempUses,
    block: &MirBlock,
    store_index: usize,
    root: &MirOperand,
) -> Vec<MirTempId> {
    let mut claimed = Vec::new();
    let mut visited = FxHashSet::default();
    // (operand, the index of the statement reading it)
    let mut pending: Vec<(&MirOperand, usize)> = vec![(root, store_index)];
    while let Some((operand, consumer)) = pending.pop() {
        let Some(temp) = chain_temp(operand) else {
            continue;
        };
        if !visited.insert(temp) {
            continue;
        }
        // Multiple consumers or a redefinition: the temporary's value is
        // observed or replaced elsewhere, so reduction is not equivalent.
        if uses.read_count(temp) != 1 || uses.write_count(temp) != 1 {
            continue;
        }
        // Unsupported control flow: the definition must be this block's,
        // before the reading statement.
        let Some((def_block, def_index)) = uses.definition(temp) else {
            continue;
        };
        if def_block != block.id || def_index >= consumer {
            continue;
        }
        let Some(MirStatementKind::Assign { value, .. }) = block
            .statements
            .get(def_index)
            .map(|statement| &statement.kind)
        else {
            continue;
        };
        // Word-typed, ranged, bits, decimal and float temporaries carry
        // their own operation semantics; only the exact integer family
        // reduces under the destination.
        if !is_exact_integer_ty(types, value.ty.semantic_id()) {
            continue;
        }
        match &value.kind {
            MirValueKind::Binary { op, lhs, rhs } if bin_op_reduces(*op) => {
                claimed.push(temp);
                // EDGE: `⇦`'s count is a semantic parameter, not a ring
                // element; it stays exact (see the module docs).
                if !matches!(op, MirBinOp::Shl) {
                    pending.push((rhs, def_index));
                }
                pending.push((lhs, def_index));
            }
            MirValueKind::Unary { op, operand } if un_op_reduces(*op) => {
                claimed.push(temp);
                pending.push((operand, def_index));
            }
            // A pure copy is transparent: nothing is reduced at the copy,
            // and the copied operand may still join the chain.
            MirValueKind::Operand(inner) => pending.push((inner, def_index)),
            // Not reducible arithmetic (a floor division or shift, a
            // comparison, a construct or call result): terminal leaf.
            MirValueKind::Binary { .. }
            | MirValueKind::Unary { .. }
            | MirValueKind::Closure(_)
            | MirValueKind::Option(_)
            | MirValueKind::TensorExtent { .. } => {}
        }
    }
    claimed
}

/// The temporary one chain edge reads, when the operand is a plain
/// unprojected temporary read. Constants and locals are leaves; a projected
/// place over a temporary is an observation in unsupported form; a value-id
/// operand never joins (its result is observed, not the chain's).
fn chain_temp(operand: &MirOperand) -> Option<MirTempId> {
    match operand {
        MirOperand::Temp(temp) => Some(*temp),
        MirOperand::Place(place) => match place.base {
            MirPlaceBase::Temp(temp) if place.projections.is_empty() => Some(temp),
            _ => None,
        },
        MirOperand::Value(_) | MirOperand::Constant(_) => None,
    }
}

/// The plain `wrapping<W>` word width of a type, following aliases.
/// `None` for ranged wrapping cells (their modulus is `hi − lo + 1`,
/// offset by `lo`), saturating words, `wrapping<inf>` and every non-word.
fn modular_word_width(types: &TypeTable, ty: TypeId) -> Option<NumericWidth> {
    let mut ty = ty;
    loop {
        match types.get(ty) {
            Type::Alias(_, inner) => ty = *inner,
            Type::ModularWord(width) => {
                return (width.is_signed_int() || width.is_unsigned_int()).then_some(*width);
            }
            _ => return None,
        }
    }
}

/// Whether a type is the exact integer family: bare `numerus`, a sized
/// `numerus<W>` (including `inf`), following aliases. Word-typed, ranged,
/// decimal, bits and float types carry other semantics and never join.
fn is_exact_integer_ty(types: &TypeTable, ty: TypeId) -> bool {
    let mut ty = ty;
    loop {
        match types.get(ty) {
            Type::Alias(_, inner) => ty = *inner,
            Type::Primitive(Primitive::Numerus) => return true,
            Type::SizedNumeric(Primitive::Numerus, width) => return !width.is_decimal(),
            _ => return false,
        }
    }
}

/// Whether a binary operation commutes with reduction mod 2^W.
/// `+ - *` are the ring operations; the bitwise pair operations act on the
/// low W bits of the two's-complement representation; `⇦` is multiplication
/// by 2ⁿ. `/` and `%` floor and `⇒` is `⌊x/2ⁿ⌋` on the exact value — none
/// of them commute, and the comparisons and boolean connectives are not
/// integer ring operations at all.
fn bin_op_reduces(op: MirBinOp) -> bool {
    matches!(
        op,
        MirBinOp::Add
            | MirBinOp::Sub
            | MirBinOp::Mul
            | MirBinOp::BitAnd
            | MirBinOp::BitOr
            | MirBinOp::BitXor
            | MirBinOp::Shl
    )
}

/// Whether a unary operation commutes with reduction mod 2^W: `-x` and
/// `¬x = -x - 1` are ring expressions. Every other unary operation is a
/// different domain (boolean, float).
fn un_op_reduces(op: MirUnOp) -> bool {
    matches!(op, MirUnOp::Neg | MirUnOp::BitNot)
}

/// Whole-function temp use counts, first definitions, and the value ids
/// whose defining statement stores into a temp (a `MirOperand::Value` read
/// observes that temp's result).
///
/// WHY: the shared read walkers in [`crate::read_analysis`] answer presence
/// (`bool`), not multiplicity, and do not follow value-id aliases or count
/// writes; the region analysis needs all three to prove a chain edge is the
/// single observation of a once-defined temporary.
struct TempUses {
    reads: FxHashMap<MirTempId, u32>,
    writes: FxHashMap<MirTempId, u32>,
    defs: FxHashMap<MirTempId, (MirBlockId, usize)>,
    value_temp: FxHashMap<MirValueId, MirTempId>,
}

impl TempUses {
    fn collect(function: &MirFunction) -> Self {
        let mut uses = Self {
            reads: FxHashMap::default(),
            writes: FxHashMap::default(),
            defs: FxHashMap::default(),
            value_temp: FxHashMap::default(),
        };
        for block in &function.blocks {
            for (index, statement) in block.statements.iter().enumerate() {
                uses.count_destination_writes(statement);
                uses.record_definition(block.id, index, statement);
            }
            uses.count_terminator_writes(&block.terminator.kind);
            uses.count_terminator_reads(&block.terminator.kind);
        }
        for block in &function.blocks {
            for statement in &block.statements {
                uses.count_statement_reads(statement);
            }
        }
        uses
    }

    fn read_count(&self, temp: MirTempId) -> u32 {
        self.reads.get(&temp).copied().unwrap_or(0)
    }

    fn write_count(&self, temp: MirTempId) -> u32 {
        self.writes.get(&temp).copied().unwrap_or(0)
    }

    fn definition(&self, temp: MirTempId) -> Option<(MirBlockId, usize)> {
        self.defs.get(&temp).copied()
    }

    /// Record the first unprojected value definition of each temp and the
    /// value ids those statements define. A temp with a second write is
    /// refused later by the write count, so the first definition suffices.
    fn record_definition(&mut self, block: MirBlockId, index: usize, statement: &MirStatement) {
        let MirStatementKind::Assign { place, value } = &statement.kind else {
            return;
        };
        if let MirPlaceBase::Temp(temp) = place.base
            && place.projections.is_empty()
        {
            self.defs.entry(temp).or_insert((block, index));
            self.value_temp.insert(value.id, temp);
        }
    }

    /// Count destination writes (any projection shape) and the reads their
    /// index projections make.
    fn count_destination_writes(&mut self, statement: &MirStatement) {
        let destination = match &statement.kind {
            MirStatementKind::Assign { place, .. }
            | MirStatementKind::Construct {
                destination: place, ..
            } => Some(place),
            MirStatementKind::RuntimeCall { destination, .. }
            | MirStatementKind::Call { destination, .. } => destination.as_ref(),
        };
        if let Some(place) = destination {
            if let MirPlaceBase::Temp(temp) = place.base {
                *self.writes.entry(temp).or_insert(0) += 1;
            }
            self.count_place_index_reads(place);
        }
    }

    fn count_statement_reads(&mut self, statement: &MirStatement) {
        match &statement.kind {
            MirStatementKind::Assign { place, value } => {
                self.count_place_index_reads(place);
                self.count_value_reads(value);
            }
            MirStatementKind::Call {
                destination,
                callee,
                args,
            } => {
                if let Some(place) = destination {
                    self.count_place_index_reads(place);
                }
                self.count_callee_reads(callee);
                for arg in args {
                    self.count_operand_reads(arg);
                }
            }
            MirStatementKind::RuntimeCall { destination, call } => {
                if let Some(place) = destination {
                    self.count_place_index_reads(place);
                }
                self.count_runtime_call_reads(call);
            }
            MirStatementKind::Construct {
                destination,
                aggregate,
            } => {
                self.count_place_index_reads(destination);
                self.count_aggregate_reads(aggregate);
            }
        }
    }

    fn count_terminator_writes(&mut self, terminator: &MirTerminatorKind) {
        // EDGE: a `TryCall` writes its destination and its error place on
        // the CFG edges; a temp written there is not single-definition even
        // though no statement destination names it.
        let MirTerminatorKind::TryCall {
            destination,
            error_place,
            ..
        } = terminator
        else {
            return;
        };
        for place in destination.iter().chain([error_place]) {
            if let MirPlaceBase::Temp(temp) = place.base {
                *self.writes.entry(temp).or_insert(0) += 1;
            }
        }
    }

    fn count_terminator_reads(&mut self, terminator: &MirTerminatorKind) {
        match terminator {
            MirTerminatorKind::Return(Some(operand))
            | MirTerminatorKind::ReturnError(operand)
            | MirTerminatorKind::Branch {
                condition: operand, ..
            }
            | MirTerminatorKind::Switch { value: operand, .. } => {
                self.count_operand_reads(operand);
            }
            MirTerminatorKind::TryCall {
                destination,
                callee,
                args,
                error_place,
                ..
            } => {
                if let Some(place) = destination {
                    self.count_place_index_reads(place);
                }
                self.count_place_index_reads(error_place);
                self.count_callee_reads(callee);
                for arg in args {
                    self.count_operand_reads(arg);
                }
            }
            MirTerminatorKind::Return(None)
            | MirTerminatorKind::ReturnThread
            | MirTerminatorKind::Goto(_)
            | MirTerminatorKind::Unreachable => {}
        }
    }

    fn count_runtime_call_reads(&mut self, call: &MirRuntimeCall) {
        for arg in &call.args {
            self.count_operand_reads(arg);
        }
        if let MirIntrinsic::Convert(conversion) = &call.intrinsic {
            if let Some(recovery) = &conversion.recovery {
                self.count_operand_reads(recovery);
            }
            for defaults in &conversion.struct_defaults {
                for field in &defaults.fields {
                    self.count_operand_reads(&field.value);
                }
            }
        }
    }

    fn count_operand_reads(&mut self, operand: &MirOperand) {
        match operand {
            MirOperand::Temp(temp) => {
                *self.reads.entry(*temp).or_insert(0) += 1;
            }
            MirOperand::Place(place) => {
                if let MirPlaceBase::Temp(temp) = place.base {
                    *self.reads.entry(temp).or_insert(0) += 1;
                }
                self.count_place_index_reads(place);
            }
            // EDGE: reading a statement's result by value id observes the
            // temp that statement stored into; count it so the temp's chain
            // edge cannot also be that read.
            MirOperand::Value(value) => {
                if let Some(temp) = self.value_temp.get(value).copied() {
                    *self.reads.entry(temp).or_insert(0) += 1;
                }
            }
            MirOperand::Constant(_) => {}
        }
    }

    fn count_place_index_reads(&mut self, place: &MirPlace) {
        for projection in &place.projections {
            if let MirProjection::Index(operand) | MirProjection::ProvenIndex(operand) = projection
            {
                self.count_operand_reads(operand);
            }
        }
    }

    fn count_value_reads(&mut self, value: &MirValue) {
        match &value.kind {
            MirValueKind::Operand(operand)
            | MirValueKind::Unary { operand, .. }
            | MirValueKind::TensorExtent {
                tensor: operand, ..
            } => self.count_operand_reads(operand),
            MirValueKind::Closure(closure) => self.count_operand_reads(&closure.environment),
            MirValueKind::Binary { lhs, rhs, .. } => {
                self.count_operand_reads(lhs);
                self.count_operand_reads(rhs);
            }
            MirValueKind::Option(op) => self.count_option_reads(op),
        }
    }

    fn count_option_reads(&mut self, op: &MirOptionOp) {
        match op {
            MirOptionOp::None => {}
            MirOptionOp::Some(operand)
            | MirOptionOp::IsNil(operand)
            | MirOptionOp::IsNotNil(operand)
            | MirOptionOp::Unwrap { value: operand, .. } => self.count_operand_reads(operand),
            MirOptionOp::Coalesce { value, fallback } => {
                self.count_operand_reads(value);
                self.count_operand_reads(fallback);
            }
            MirOptionOp::Chain { base, link } => {
                self.count_operand_reads(base);
                self.count_chain_link_reads(link);
            }
        }
    }

    fn count_chain_link_reads(&mut self, link: &MirOptionChainLink) {
        match link {
            MirOptionChainLink::Field(_) | MirOptionChainLink::VariantField { .. } => {}
            MirOptionChainLink::Index(index) => self.count_operand_reads(index),
            MirOptionChainLink::Call { callee, args } => {
                self.count_callee_reads(callee);
                for arg in args {
                    self.count_operand_reads(arg);
                }
            }
        }
    }

    fn count_callee_reads(&mut self, callee: &MirCallee) {
        match callee {
            MirCallee::Function(_) | MirCallee::Definition { .. } => {}
            MirCallee::Closure(closure) => self.count_operand_reads(&closure.environment),
            MirCallee::Value(operand) => self.count_operand_reads(operand),
        }
    }

    fn count_aggregate_reads(&mut self, aggregate: &MirAggregate) {
        match &aggregate.fields {
            MirAggregateFields::Ordered(items) => {
                for item in items {
                    let (MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand)) =
                        item;
                    self.count_operand_reads(operand);
                }
            }
            MirAggregateFields::Named(fields) => {
                for field in fields {
                    self.count_operand_reads(&field.value);
                }
            }
            MirAggregateFields::Keyed(fields) => {
                for field in fields {
                    self.count_operand_reads(&field.key);
                    self.count_operand_reads(&field.value);
                }
            }
        }
    }
}
