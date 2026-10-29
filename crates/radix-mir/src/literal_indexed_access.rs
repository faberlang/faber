//! Exact literal-coordinate tensor access facts for device kernels.
//!
//! A literal index list is a compile-time coordinate carrier, not a runtime
//! `lista` value. This analysis admits that carrier only when every use is the
//! index operand of `TensorGet`/`TensorSet`, every coordinate is in bounds,
//! optional get results are consumed only by `Coalesce`, and indexed writes
//! cover the returned tensor completely.

use crate::{
    MirAggregateFields, MirAggregateItem, MirAggregateKind, MirCollectionOp, MirConstant,
    MirFunction, MirIntrinsic, MirLocalId, MirOperand, MirOptionOp, MirPlace, MirPlaceBase,
    MirRuntimeCall, MirStatementKind, MirTempId, MirTerminatorKind, MirType, MirValueKind,
    figura::i64s_from_literal_index,
};
use handroll::{FxHashMap, FxHashSet};
use radix_types::{Type, TypeTable};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MirSlot {
    Local(MirLocalId),
    Temp(MirTempId),
}

impl MirSlot {
    #[must_use]
    pub fn from_place(place: &MirPlace) -> Option<Self> {
        if !place.projections.is_empty() {
            return None;
        }
        Some(match place.base {
            MirPlaceBase::Local(local) => Self::Local(local),
            MirPlaceBase::Temp(temp) => Self::Temp(temp),
        })
    }

    #[must_use]
    pub fn from_operand(operand: &MirOperand) -> Option<Self> {
        match operand {
            MirOperand::Place(place) => Self::from_place(place),
            MirOperand::Temp(temp) => Some(Self::Temp(*temp)),
            MirOperand::Value(_) | MirOperand::Constant(_) => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LiteralIndexedAccessPlan {
    aliases: FxHashMap<MirSlot, MirSlot>,
    indices: FxHashMap<MirSlot, Vec<u64>>,
    get_results: FxHashSet<MirSlot>,
    returned_output: MirSlot,
    output_elements: u64,
}

impl LiteralIndexedAccessPlan {
    #[must_use]
    pub fn for_function(function: &MirFunction, types: &TypeTable) -> Option<Self> {
        let aliases = direct_slot_aliases(function);
        let mut indices = FxHashMap::default();
        for block in &function.blocks {
            for statement in &block.statements {
                let MirStatementKind::Construct {
                    destination,
                    aggregate,
                } = &statement.kind
                else {
                    continue;
                };
                if !matches!(aggregate.kind, MirAggregateKind::Array) {
                    continue;
                }
                let slot = MirSlot::from_place(destination)?;
                let values = literal_array_values(&aggregate.fields)?;
                indices.insert(resolve_slot(slot, &aliases)?, values);
            }
        }

        let returned_output = returned_tensor_slot(function, types, &aliases)?;
        let output_dims = tensor_shape(slot_ty(function, returned_output)?, types)?;
        let output_elements = output_dims
            .iter()
            .try_fold(1_u64, |count, dim| count.checked_mul(*dim))?;
        let mut get_results = FxHashSet::default();
        let mut set_offsets = FxHashSet::default();
        let mut access_count = 0_usize;
        for block in &function.blocks {
            for statement in &block.statements {
                let MirStatementKind::RuntimeCall { destination, call } = &statement.kind else {
                    continue;
                };
                let MirIntrinsic::Collection(
                    op @ (MirCollectionOp::TensorGet | MirCollectionOp::TensorSet),
                ) = call.intrinsic
                else {
                    continue;
                };
                access_count += 1;
                let offset = literal_call_offset(function, call, types, &aliases, &indices)?;
                if let MirCollectionOp::TensorGet = op {
                    let destination = MirSlot::from_place(destination.as_ref()?)?;
                    get_results.insert(resolve_slot(destination, &aliases)?);
                } else {
                    // `TensorSet`: the let-else above admits only Get and Set.
                    let receiver =
                        resolve_slot(MirSlot::from_operand(call.args.first()?)?, &aliases)?;
                    if receiver != returned_output || offset >= output_elements {
                        return None;
                    }
                    set_offsets.insert(offset);
                }
            }
        }
        if access_count == 0
            || set_offsets.len() != usize::try_from(output_elements).ok()?
            || !(0..output_elements).all(|offset| set_offsets.contains(&offset))
        {
            return None;
        }
        if !index_uses_are_exact(function, &aliases, &indices)
            || !get_uses_are_exact(function, &aliases, &get_results)
        {
            return None;
        }
        Some(Self {
            aliases,
            indices,
            get_results,
            returned_output,
            output_elements,
        })
    }

    #[must_use]
    pub fn resolve_slot(&self, slot: MirSlot) -> Option<MirSlot> {
        resolve_slot(slot, &self.aliases)
    }

    #[must_use]
    pub fn is_index_slot(&self, place: &MirPlace) -> bool {
        MirSlot::from_place(place)
            .and_then(|slot| self.resolve_slot(slot))
            .is_some_and(|slot| self.indices.contains_key(&slot))
    }

    #[must_use]
    pub fn is_get_slot(&self, slot: MirSlot) -> bool {
        self.resolve_slot(slot)
            .is_some_and(|slot| self.get_results.contains(&slot))
    }

    #[must_use]
    pub fn literal_offset(
        &self,
        function: &MirFunction,
        call: &MirRuntimeCall,
        types: &TypeTable,
    ) -> Option<u64> {
        literal_call_offset(function, call, types, &self.aliases, &self.indices)
    }

    #[must_use]
    pub fn returned_output(&self) -> MirSlot {
        self.returned_output
    }

    #[must_use]
    pub fn output_elements(&self) -> u64 {
        self.output_elements
    }
}

fn literal_array_values(fields: &MirAggregateFields) -> Option<Vec<u64>> {
    let MirAggregateFields::Ordered(items) = fields else {
        return None;
    };
    items
        .iter()
        .map(|item| match item {
            MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Int(value))) => {
                u64::try_from(*value).ok()
            }
            MirAggregateItem::Operand(MirOperand::Constant(MirConstant::UInt(value))) => {
                Some(*value)
            }
            MirAggregateItem::Operand(_) | MirAggregateItem::Spread(_) => None,
        })
        .collect()
}

fn direct_slot_aliases(function: &MirFunction) -> FxHashMap<MirSlot, MirSlot> {
    let mut aliases = FxHashMap::default();
    for block in &function.blocks {
        for statement in &block.statements {
            let MirStatementKind::Assign { place, value } = &statement.kind else {
                continue;
            };
            let Some(destination) = MirSlot::from_place(place) else {
                continue;
            };
            let MirValueKind::Operand(source) = &value.kind else {
                continue;
            };
            let Some(source) = MirSlot::from_operand(source) else {
                continue;
            };
            aliases.insert(destination, source);
        }
    }
    aliases
}

fn resolve_slot(mut slot: MirSlot, aliases: &FxHashMap<MirSlot, MirSlot>) -> Option<MirSlot> {
    let mut seen = FxHashSet::default();
    while let Some(next) = aliases.get(&slot).copied() {
        if !seen.insert(slot) {
            return None;
        }
        slot = next;
    }
    Some(slot)
}

fn slot_ty(function: &MirFunction, slot: MirSlot) -> Option<MirType> {
    match slot {
        MirSlot::Local(local) => function
            .params
            .iter()
            .find(|param| param.local == local)
            .map(|param| param.ty)
            .or_else(|| {
                function
                    .locals
                    .iter()
                    .find(|entry| entry.id == local)
                    .map(|entry| entry.ty)
            }),
        MirSlot::Temp(temp) => function
            .temps
            .iter()
            .find(|entry| entry.id == temp)
            .map(|entry| entry.ty),
    }
}

fn tensor_shape(ty: MirType, types: &TypeTable) -> Option<Vec<u64>> {
    let mut semantic = ty.semantic_id();
    loop {
        match types.get(semantic) {
            Type::Alias(_, inner) => semantic = *inner,
            Type::Tensor(_, shape) => {
                return i64s_from_literal_index(*shape, types)?
                    .into_iter()
                    .map(u64::try_from)
                    .collect::<Result<Vec<_>, _>>()
                    .ok();
            }
            _ => return None,
        }
    }
}

fn returned_tensor_slot(
    function: &MirFunction,
    types: &TypeTable,
    aliases: &FxHashMap<MirSlot, MirSlot>,
) -> Option<MirSlot> {
    let mut returned = None;
    for block in &function.blocks {
        let MirTerminatorKind::Return(Some(operand)) = &block.terminator.kind else {
            continue;
        };
        let slot = resolve_slot(MirSlot::from_operand(operand)?, aliases)?;
        tensor_shape(slot_ty(function, slot)?, types)?;
        if returned.replace(slot).is_some_and(|prior| prior != slot) {
            return None;
        }
    }
    returned
}

fn literal_call_offset(
    function: &MirFunction,
    call: &MirRuntimeCall,
    types: &TypeTable,
    aliases: &FxHashMap<MirSlot, MirSlot>,
    indices: &FxHashMap<MirSlot, Vec<u64>>,
) -> Option<u64> {
    let receiver = resolve_slot(MirSlot::from_operand(call.args.first()?)?, aliases)?;
    let dims = tensor_shape(slot_ty(function, receiver)?, types)?;
    let index = resolve_slot(MirSlot::from_operand(call.args.get(1)?)?, aliases)?;
    let coordinates = indices.get(&index)?;
    if coordinates.len() != dims.len() {
        return None;
    }
    coordinates
        .iter()
        .zip(dims)
        .try_fold(0_u64, |offset, (coordinate, dim)| {
            if *coordinate >= dim {
                None
            } else {
                offset.checked_mul(dim)?.checked_add(*coordinate)
            }
        })
}

fn index_uses_are_exact(
    function: &MirFunction,
    aliases: &FxHashMap<MirSlot, MirSlot>,
    indices: &FxHashMap<MirSlot, Vec<u64>>,
) -> bool {
    let index_slots = indices.keys().copied().collect::<FxHashSet<_>>();
    function.blocks.iter().all(|block| {
        block.statements.iter().all(|statement| match &statement.kind {
            MirStatementKind::Assign { place, value } => {
                let is_alias = matches!(&value.kind, MirValueKind::Operand(source)
                    if MirSlot::from_place(place).is_some_and(|dest| resolve_slot(dest, aliases).is_some_and(|slot| indices.contains_key(&slot)))
                    && MirSlot::from_operand(source).is_some_and(|source| resolve_slot(source, aliases).is_some_and(|slot| indices.contains_key(&slot))));
                is_alias || !value_reads_slots(&value.kind, aliases, &index_slots)
            }
            MirStatementKind::RuntimeCall { call, .. } => call.args.iter().enumerate().all(|(position, operand)| {
                let is_index = operand_base_slot(operand)
                    .and_then(|slot| resolve_slot(slot, aliases))
                    .is_some_and(|slot| indices.contains_key(&slot));
                !is_index
                    || (position == 1
                        && matches!(call.intrinsic, MirIntrinsic::Collection(MirCollectionOp::TensorGet | MirCollectionOp::TensorSet)))
            }),
            MirStatementKind::Construct { aggregate, .. } => !aggregate
                .fields
                .operands()
                .into_iter()
                .any(|operand| operand_resolves_into(operand, aliases, &index_slots)),
            MirStatementKind::Call { args, .. } => !args
                .iter()
                .any(|operand| operand_resolves_into(operand, aliases, &index_slots)),
        }) && !terminator_reads_set(&block.terminator.kind, aliases, &index_slots)
    })
}

fn get_uses_are_exact(
    function: &MirFunction,
    aliases: &FxHashMap<MirSlot, MirSlot>,
    gets: &FxHashSet<MirSlot>,
) -> bool {
    function.blocks.iter().all(|block| {
        block.statements.iter().all(|statement| match &statement.kind {
            MirStatementKind::Assign { place, value } => {
                let is_alias = matches!(&value.kind, MirValueKind::Operand(source)
                    if MirSlot::from_place(place).is_some_and(|dest| resolve_slot(dest, aliases).is_some_and(|slot| gets.contains(&slot)))
                    && MirSlot::from_operand(source).is_some_and(|source| resolve_slot(source, aliases).is_some_and(|slot| gets.contains(&slot))));
                let is_coalesce = matches!(&value.kind, MirValueKind::Option(MirOptionOp::Coalesce { value, fallback })
                    if operand_resolves_into(value, aliases, gets)
                    && !operand_resolves_into(fallback, aliases, gets));
                is_alias || is_coalesce || !value_reads_slots(&value.kind, aliases, gets)
            }
            MirStatementKind::RuntimeCall { call, .. } => !call
                .args
                .iter()
                .any(|operand| operand_resolves_into(operand, aliases, gets)),
            MirStatementKind::Construct { aggregate, .. } => !aggregate
                .fields
                .operands()
                .into_iter()
                .any(|operand| operand_resolves_into(operand, aliases, gets)),
            MirStatementKind::Call { args, .. } => !args
                .iter()
                .any(|operand| operand_resolves_into(operand, aliases, gets)),
        }) && !terminator_reads_set(&block.terminator.kind, aliases, gets)
    })
}

fn value_reads_slots(
    value: &MirValueKind,
    aliases: &FxHashMap<MirSlot, MirSlot>,
    slots: &FxHashSet<MirSlot>,
) -> bool {
    match value {
        MirValueKind::Operand(operand)
        | MirValueKind::Unary { operand, .. }
        | MirValueKind::TensorExtent {
            tensor: operand, ..
        } => operand_resolves_into(operand, aliases, slots),
        MirValueKind::Binary { lhs, rhs, .. } => {
            operand_resolves_into(lhs, aliases, slots) || operand_resolves_into(rhs, aliases, slots)
        }
        MirValueKind::Option(op) => match op {
            MirOptionOp::Some(value)
            | MirOptionOp::IsNil(value)
            | MirOptionOp::IsNotNil(value)
            | MirOptionOp::Unwrap { value, .. } => operand_resolves_into(value, aliases, slots),
            MirOptionOp::Coalesce { value, fallback } => {
                operand_resolves_into(value, aliases, slots)
                    || operand_resolves_into(fallback, aliases, slots)
            }
            MirOptionOp::Chain { base, .. } => operand_resolves_into(base, aliases, slots),
            MirOptionOp::None => false,
        },
        MirValueKind::Closure(closure) => {
            operand_resolves_into(&closure.environment, aliases, slots)
        }
    }
}

fn operand_base_slot(operand: &MirOperand) -> Option<MirSlot> {
    match operand {
        MirOperand::Place(place) => Some(match place.base {
            MirPlaceBase::Local(local) => MirSlot::Local(local),
            MirPlaceBase::Temp(temp) => MirSlot::Temp(temp),
        }),
        MirOperand::Temp(temp) => Some(MirSlot::Temp(*temp)),
        MirOperand::Value(_) | MirOperand::Constant(_) => None,
    }
}

fn operand_resolves_into(
    operand: &MirOperand,
    aliases: &FxHashMap<MirSlot, MirSlot>,
    slots: &FxHashSet<MirSlot>,
) -> bool {
    operand_base_slot(operand)
        .and_then(|slot| resolve_slot(slot, aliases))
        .is_some_and(|slot| slots.contains(&slot))
}

fn terminator_reads_set(
    terminator: &MirTerminatorKind,
    aliases: &FxHashMap<MirSlot, MirSlot>,
    slots: &FxHashSet<MirSlot>,
) -> bool {
    match terminator {
        MirTerminatorKind::Return(Some(operand))
        | MirTerminatorKind::ReturnError(operand)
        | MirTerminatorKind::Branch {
            condition: operand, ..
        }
        | MirTerminatorKind::Switch { value: operand, .. } => {
            operand_resolves_into(operand, aliases, slots)
        }
        MirTerminatorKind::TryCall { args, .. } => args
            .iter()
            .any(|operand| operand_resolves_into(operand, aliases, slots)),
        MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable
        | MirTerminatorKind::Return(None)
        | MirTerminatorKind::ReturnThread => false,
    }
}
