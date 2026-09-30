//! Implicit stores (d11-6 rulings 2, 3, 5, 17b, 19i).
//!
//! A value that lands in a typed integer slot applies the slot's policy:
//! a trapping slot traps when the exact value does not fit, a `wrapping<W>`
//! slot reduces the value modulo 2^W, and a `saturating<W>` slot clamps it.
//! The trap is a runner-internal failure, never a conversion failure, so it is
//! not recoverable by `cape` or `⇥` (ruling 5); its message names the value,
//! the destination type and the slot, and for an inferred slot the expression
//! the type was inferred from.

use super::{Runner, RunnerError};
use crate::exact_int::{exact_carrier, int_value};
use crate::value::{Value, modular_word_mask, modular_word_result, saturating_word_result};
use radix_hir::{HirStoreKind, HirStoreSite};
use radix_lexer::Span;
use radix_types::{NumericWidth, Primitive, Type, TypeId};

/// Stable identity of a failed implicit store: distinct from arithmetic
/// overflow (ruling 5).
pub const RUNNER_ISSUE_IMPLICIT_STORE_TRAP: &str = "implicit_store_out_of_range";

/// The policy family of an integer slot.
#[derive(Clone, Copy)]
enum SlotPolicy {
    Trapping,
    Wrapping,
    Saturating,
}

impl<H: super::Host + ?Sized> Runner<'_, H> {
    /// Apply the slot policy of `target` to an integer `value`.
    ///
    /// `None` when `target` is not an integer slot or `value` is not an
    /// integer, so the caller falls through to the ordinary conversion.
    pub(crate) fn eval_implicit_store(
        &self,
        site: &HirStoreSite,
        target: TypeId,
        value: &Value,
        span: Span,
    ) -> Option<Result<Value, RunnerError>> {
        let (policy, width) = self.integer_slot(target)?;
        let exact = int_value(value)?;
        let (lo, hi) = width.integer_range()?;
        Some(match policy {
            SlotPolicy::Trapping => match (lo..=hi).contains(&exact) {
                true => exact_carrier(exact, width)
                    .ok_or_else(|| self.store_trap(site, exact, width, span)),
                false => Err(self.store_trap(site, exact, width, span)),
            },
            SlotPolicy::Wrapping => {
                let reduced = (exact & i128::from(modular_word_mask(width))) as u64;
                Ok(modular_word_result(reduced, width))
            }
            SlotPolicy::Saturating => Ok(saturating_word_result(exact, width)),
        })
    }

    /// A value above `i64::MAX` can sit in an `i64`-typed expression at the
    /// 64-bit cap (`i64::MIN / -1`, `i64 + i64`), where the type does not
    /// bound the value and the typechecker omits the store node. The store
    /// into a signed local is the check (ruling 17b).
    pub(crate) fn guard_signed_local_store(
        &self,
        place: &radix_mir::MirPlace,
        value: &Value,
        span: Span,
    ) -> Result<(), RunnerError> {
        let Value::UInt(magnitude) = value else {
            return Ok(());
        };
        if i64::try_from(*magnitude).is_ok() || !place.projections.is_empty() {
            return Ok(());
        }
        let radix_mir::MirPlaceBase::Local(id) = place.base else {
            return Ok(());
        };
        let Some(frame) = self.frames.last() else {
            return Ok(());
        };
        let Ok(function) = self.function_by_id(frame.function, span) else {
            return Ok(());
        };
        let Some(local) = function
            .locals
            .iter()
            .find(|local| local.id == id)
            .map(|local| (local.ty, local.name))
            .or_else(|| {
                function
                    .params
                    .iter()
                    .find(|p| p.local == id)
                    .map(|p| (p.ty, None))
            })
        else {
            return Ok(());
        };
        let signed = match self.types.get(local.0.semantic_id()) {
            Type::Primitive(Primitive::Numerus) => true,
            Type::SizedNumeric(Primitive::Numerus, width) => width.is_signed_int(),
            _ => false,
        };
        if !signed {
            return Ok(());
        }
        let site = HirStoreSite {
            kind: HirStoreKind::Assignment,
            slot: local.1,
            inferred_from: None,
        };
        Err(self.store_trap(&site, i128::from(*magnitude), NumericWidth::I64, span))
    }

    fn integer_slot(&self, target: TypeId) -> Option<(SlotPolicy, NumericWidth)> {
        let mut target = target;
        while let Type::Alias(_, inner) = self.types.get(target) {
            target = *inner;
        }
        match self.types.get(target) {
            Type::Primitive(Primitive::Numerus) => Some((SlotPolicy::Trapping, NumericWidth::I64)),
            Type::SizedNumeric(Primitive::Numerus, width) if width.integer_range().is_some() => {
                Some((SlotPolicy::Trapping, *width))
            }
            Type::ModularWord(width) => Some((SlotPolicy::Wrapping, *width)),
            Type::SaturatingWord(width) => Some((SlotPolicy::Saturating, *width)),
            _ => None,
        }
    }

    fn store_trap(
        &self,
        site: &HirStoreSite,
        value: i128,
        width: NumericWidth,
        span: Span,
    ) -> RunnerError {
        let ty = width.rust_type();
        let slot = site.slot.map(|slot| self.interner.resolve(slot).to_owned());
        let position = match (site.kind, &slot) {
            (HirStoreKind::Declaration, Some(slot)) => format!("declaration of `{slot}`"),
            (HirStoreKind::Assignment, Some(slot)) => format!("assignment to `{slot}`"),
            (HirStoreKind::Argument, Some(slot)) => format!("argument `{slot}`"),
            (HirStoreKind::Field, Some(slot)) => format!("field `{slot}`"),
            (HirStoreKind::AwaitBinding, Some(slot)) => format!("await-binding `{slot}`"),
            (kind, _) => store_kind_phrase(kind).to_owned(),
        };
        let mut message = format!("{value} does not fit in `{ty}` ({position})");
        let mut args = vec![
            ("value".to_owned(), value.to_string()),
            ("type".to_owned(), ty.to_owned()),
            ("store".to_owned(), store_kind_phrase(site.kind).to_owned()),
        ];
        if let Some(slot) = &slot {
            args.push(("slot".to_owned(), slot.clone()));
        }
        if let Some(from) = site.inferred_from {
            let name = slot.as_deref().unwrap_or("the slot");
            message.push_str(&format!(
                "; `{name}` was inferred as `{ty}` from the expression at bytes {}..{}, declare its type",
                from.start, from.end
            ));
            args.push((
                "inferred_from".to_owned(),
                format!("{}..{}", from.start, from.end),
            ));
        }
        if value < 0 && width.is_unsigned_int() {
            message.push_str(" (a negative value cannot be stored in an unsigned slot)");
        }
        RunnerError {
            message,
            issue: RUNNER_ISSUE_IMPLICIT_STORE_TRAP.to_owned(),
            span,
            code: crate::RUNNER_CODE_INTERNAL,
            args,
        }
    }
}

fn store_kind_phrase(kind: HirStoreKind) -> &'static str {
    match kind {
        HirStoreKind::Declaration => "declaration",
        HirStoreKind::Assignment => "assignment",
        HirStoreKind::Argument => "argument",
        HirStoreKind::ParameterDefault => "parameter default",
        HirStoreKind::Return => "return",
        HirStoreKind::Yield => "yield",
        HirStoreKind::Throw => "throw",
        HirStoreKind::Field => "field",
        HirStoreKind::Element => "collection element",
        HirStoreKind::TuplePosition => "tuple position",
        HirStoreKind::ConversionDefault => "conversion default",
        HirStoreKind::AwaitBinding => "await-binding",
    }
}

impl<H: super::Host + ?Sized> Runner<'_, H> {
    /// Static decimal width of an operand, when its type is a decimal. Only
    /// called on the rare mixed integer-carrier / float comparison.
    pub(crate) fn operand_decimal_width(
        &self,
        operand: &radix_mir::MirOperand,
    ) -> Option<NumericWidth> {
        use radix_mir::{MirOperand, MirPlaceBase};
        let function_id = self.frames.last()?.function;
        let function = self
            .program
            .indexed_function(&self.function_index, function_id)?;
        let ty = match operand {
            MirOperand::Temp(id) => function.temps.iter().find(|t| t.id == *id).map(|t| t.ty),
            MirOperand::Place(place) if place.projections.is_empty() => match place.base {
                MirPlaceBase::Temp(id) => function.temps.iter().find(|t| t.id == id).map(|t| t.ty),
                MirPlaceBase::Local(id) => function
                    .locals
                    .iter()
                    .find(|l| l.id == id)
                    .map(|l| l.ty)
                    .or_else(|| function.params.iter().find(|p| p.local == id).map(|p| p.ty)),
            },
            MirOperand::Value(id) => self.value_types.get(&(function_id, *id)).copied(),
            _ => None,
        }?;
        let mut ty = self.types.peel_access_marker(ty.semantic_id()).1;
        while let Type::Alias(_, inner) = self.types.get(ty) {
            ty = *inner;
        }
        match self.types.get(ty) {
            Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal() => Some(*width),
            _ => None,
        }
    }
}
