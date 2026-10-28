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
use crate::value::{
    Value, modular_word_result, policy_float_cell, saturating_word_result,
    trapping_float_store_fails,
};
use radix_hir::{HirStoreKind, HirStoreSite};
use radix_lexer::Span;
use radix_types::{FloatPolicy, NumericWidth, Primitive, Type, TypeId};
use std::cell::RefCell;
use std::rc::Rc;

/// An integer slot's policy, width and, for a ranged cell, its inclusive range
/// (`None`: the whole width).
type IntegerSlot = (SlotPolicy, NumericWidth, Option<(i128, i128)>);

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

/// The indexed container families whose elements carry a slot policy.
#[derive(Clone, Copy)]
enum ContainerCarrier {
    /// `tensor<E, ..>` and `sparsa<E, ..>`, both carried as `Value::Tensor`.
    Sequence,
    /// `vector<E, size N>` carried as `Value::Vector`.
    Vector,
    /// `matrix<E, [R, C]>` carried as `Value::Matrix`.
    Matrix,
}

impl<H: super::Host + ?Sized> Runner<'_, H> {
    /// Apply the slot policy of `target` to an integer `value`.
    ///
    /// `None` when `target` is not an integer slot or `value` is not an
    /// integer, so the caller falls through to the ordinary conversion.
    pub(crate) fn eval_implicit_store(
        &mut self,
        site: &HirStoreSite,
        target: TypeId,
        value: &Value,
        span: Span,
    ) -> Option<Result<Value, RunnerError>> {
        if let Some(width) = self.decimal_slot(target) {
            return self.eval_decimal_store(site, width, value, span);
        }
        if let Some((policy, width)) = policy_float_cell(self.types, target) {
            return self.eval_policy_float_store(site, policy, width, value, span);
        }
        // Ruling 19: an implicit store into an indexed container applies the
        // element type's policy to every live element, so an out-of-range
        // element traps here instead of surviving the authored conversion
        // (which passes a `Value::Tensor` through unchanged).
        let container = match self.types.get(target) {
            Type::Tensor(element, _) | Type::Sparsa(element, _) => {
                Some((ContainerCarrier::Sequence, *element))
            }
            Type::Vector(element, _) => Some((ContainerCarrier::Vector, *element)),
            Type::Matrix(element, _) => Some((ContainerCarrier::Matrix, *element)),
            _ => None,
        };
        if let Some((carrier, element)) = container {
            // The recursion passes the real site, so a trapped element names
            // the declaration, assignment or argument that stored it.
            return match (carrier, value) {
                (ContainerCarrier::Sequence, Value::Tensor(cell)) => {
                    let values = match self.tensor_logical_values(cell, span) {
                        Ok(values) => values,
                        Err(error) => return Some(Err(error)),
                    };
                    let shape = cell.borrow().shape.clone();
                    let mapped = values
                        .into_iter()
                        .map(|value| self.store_element(site, element, value, span))
                        .collect::<Result<Vec<_>, _>>();
                    Some(mapped.and_then(|mapped| {
                        crate::runtime::tensor_value_typed(
                            self.types,
                            mapped,
                            Some(element),
                            shape,
                            span,
                        )
                    }))
                }
                (ContainerCarrier::Vector, Value::Vector(cell)) => {
                    let lanes = cell.borrow().clone();
                    let mapped = lanes
                        .into_iter()
                        .map(|value| self.store_element(site, element, value, span))
                        .collect::<Result<Vec<_>, _>>();
                    Some(mapped.map(|mapped| Value::Vector(Rc::new(RefCell::new(mapped)))))
                }
                (ContainerCarrier::Matrix, Value::Matrix(cell)) => {
                    let mut matrix = cell.borrow_mut();
                    for value in &mut matrix.data {
                        match self.store_element(site, element, value.clone(), span) {
                            Ok(stored) => *value = stored,
                            Err(error) => return Some(Err(error)),
                        }
                    }
                    Some(Ok(Value::Matrix(Rc::clone(cell))))
                }
                _ => None,
            };
        }
        let (policy, width, ranged) = self.integer_slot(target)?;
        // Ruling 38: an `inf` beyond the 64-bit carriers fits no bounded
        // slot. A trapping slot traps with the store identity (value, type,
        // position); the policy words reduce/clamp the exact value.
        if let Value::Magnus(big) = value {
            if let Some((lo, hi)) = ranged {
                return Some(match policy {
                    SlotPolicy::Trapping => Err(self.ranged_store_trap(
                        site,
                        big.to_string(),
                        big.is_negative(),
                        width,
                        (lo, hi),
                        span,
                    )),
                    SlotPolicy::Wrapping => Ok(ranged_big_wrap(big, width, lo, hi)),
                    SlotPolicy::Saturating => Ok(ranged_big_clamp(big, width, lo, hi)),
                });
            }
            return Some(match policy {
                SlotPolicy::Trapping => {
                    Err(self.store_trap(site, big.to_string(), big.is_negative(), width, span))
                }
                SlotPolicy::Wrapping => Ok(crate::inf::wrap_to_width(big, width)),
                SlotPolicy::Saturating => Ok(crate::inf::clamp_to_width(big, width)),
            });
        }
        let exact = int_value(value)?;
        // A ranged cell applies its policy to its own range; a plain cell to
        // its width. `integer_slot` never yields `inf` (no range), so the
        // width range is always `Some`.
        let (lo, hi) = match ranged {
            Some(range) => range,
            None => width.integer_range()?,
        };
        Some(match policy {
            SlotPolicy::Trapping => {
                if (lo..=hi).contains(&exact) {
                    exact_carrier(exact, width).ok_or_else(|| {
                        self.store_trap(site, exact.to_string(), exact < 0, width, span)
                    })
                } else if ranged.is_some() {
                    Err(self.ranged_store_trap(
                        site,
                        exact.to_string(),
                        exact < 0,
                        width,
                        (lo, hi),
                        span,
                    ))
                } else {
                    Err(self.store_trap(site, exact.to_string(), exact < 0, width, span))
                }
            }
            SlotPolicy::Wrapping if ranged.is_some() => {
                Ok(ranged_carrier(ranged_wrap(exact, lo, hi), width))
            }
            SlotPolicy::Saturating if ranged.is_some() => {
                Ok(ranged_carrier(exact.clamp(lo, hi), width))
            }
            SlotPolicy::Wrapping => Ok(modular_word_result(exact, width)),
            SlotPolicy::Saturating => Ok(saturating_word_result(exact, width)),
        })
    }

    /// A float value lands in a `trapping<W>` or `saturating<W>` cell (D1, D2).
    ///
    /// Only a `trapping` cell checks, and only here: a NaN or infinite value
    /// (for `f16` and `bf16`, one that rounds to infinity in the format) traps
    /// with the store identity. Everything else, and every `saturating` store,
    /// returns `None` so the ordinary conversion keeps the bare float of the
    /// cell's width. Nothing clamps.
    fn eval_policy_float_store(
        &self,
        site: &HirStoreSite,
        policy: FloatPolicy,
        width: NumericWidth,
        value: &Value,
        span: Span,
    ) -> Option<Result<Value, RunnerError>> {
        let Value::Float(float) = value else {
            return None;
        };
        if policy != FloatPolicy::Trapping || !trapping_float_store_fails(*float, width) {
            return None;
        }
        let text = crate::value::display_float_at_width(*float, Some(NumericWidth::F64));
        Some(Err(self.store_trap(
            site,
            text,
            float.is_sign_negative(),
            width,
            span,
        )))
    }

    /// Store one exactly computed tensor element under its element type's
    /// slot policy (ruling 19). Non-integer elements pass through.
    pub(crate) fn store_tensor_element(
        &mut self,
        element: TypeId,
        value: Value,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let site = HirStoreSite {
            kind: HirStoreKind::Element,
            slot: None,
            inferred_from: None,
            inferred_text: None,
        };
        self.store_element(&site, element, value, span)
    }

    /// One element of a container store: apply the element slot policy at the
    /// given site, or keep the element when its type is not an integer slot.
    fn store_element(
        &mut self,
        site: &HirStoreSite,
        element: TypeId,
        value: Value,
        span: Span,
    ) -> Result<Value, RunnerError> {
        self.eval_implicit_store(site, element, &value, span)
            .unwrap_or(Ok(value))
    }

    /// The decimal width of a `d64` slot.
    fn decimal_slot(&self, target: TypeId) -> Option<NumericWidth> {
        let mut target = target;
        while let Type::Alias(_, inner) = self.types.get(target) {
            target = *inner;
        }
        match self.types.get(target) {
            Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal() => Some(*width),
            _ => None,
        }
    }

    /// A decimal value lands in a `d64` slot (D11.7): the exact value is
    /// rounded half-even to scale 8, and a value outside the carrier range
    /// traps with the store identity. `None` for a non-decimal value, so the
    /// caller falls through to the ordinary conversion.
    fn eval_decimal_store(
        &self,
        site: &HirStoreSite,
        width: NumericWidth,
        value: &Value,
        span: Span,
    ) -> Option<Result<Value, RunnerError>> {
        let (carrier, scale) = crate::decimal::parts(value)?;
        Some(match crate::decimal::store_carrier(carrier, scale) {
            Some(stored) => Ok(Value::Int(stored)),
            None => Err(self.store_trap(
                site,
                crate::decimal::format(carrier, scale),
                carrier < 0,
                width,
                span,
            )),
        })
    }

    /// Policy, width and, for a ranged cell, the inclusive range of an integer
    /// slot (`None` range: the whole width).
    fn integer_slot(&self, target: TypeId) -> Option<IntegerSlot> {
        let mut target = target;
        while let Type::Alias(_, inner) = self.types.get(target) {
            target = *inner;
        }
        match self.types.get(target) {
            Type::Primitive(Primitive::Numerus) => {
                Some((SlotPolicy::Trapping, NumericWidth::I64, None))
            }
            // Deliberate: no store slot for `inf` (ruling 38: a store into an
            // `inf` slot is total, nothing to check).
            Type::SizedNumeric(Primitive::Numerus, width) if width.integer_range().is_some() => {
                Some((SlotPolicy::Trapping, *width, None))
            }
            Type::ModularWord(width) => Some((SlotPolicy::Wrapping, *width, None)),
            Type::SaturatingWord(width) => Some((SlotPolicy::Saturating, *width, None)),
            Type::RangedInt {
                policy,
                width,
                lo,
                hi,
            } => Some((
                match policy {
                    radix_types::IntPolicy::Trapping => SlotPolicy::Trapping,
                    radix_types::IntPolicy::Wrapping => SlotPolicy::Wrapping,
                    radix_types::IntPolicy::Saturating => SlotPolicy::Saturating,
                },
                *width,
                Some((*lo, *hi)),
            )),
            _ => None,
        }
    }

    /// A value outside a trapping ranged cell: the store identity names the
    /// range, `trapping<i32, -2‥3>`, instead of the bare width.
    fn ranged_store_trap(
        &self,
        site: &HirStoreSite,
        value: String,
        negative: bool,
        width: NumericWidth,
        (lo, hi): (i128, i128),
        span: Span,
    ) -> RunnerError {
        let ty = format!("trapping<{}, {lo}‥{}>", width.rust_type(), hi + 1);
        self.store_trap_in(site, value, negative, &ty, width.is_unsigned_int(), span)
    }

    fn store_trap(
        &self,
        site: &HirStoreSite,
        value: String,
        negative: bool,
        width: NumericWidth,
        span: Span,
    ) -> RunnerError {
        self.store_trap_in(
            site,
            value,
            negative,
            width.rust_type(),
            width.is_unsigned_int(),
            span,
        )
    }

    fn store_trap_in(
        &self,
        site: &HirStoreSite,
        value: String,
        negative: bool,
        ty: &str,
        unsigned: bool,
        span: Span,
    ) -> RunnerError {
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
            ("value".to_owned(), value),
            ("type".to_owned(), ty.to_owned()),
            ("store".to_owned(), store_kind_phrase(site.kind).to_owned()),
        ];
        if let Some(slot) = &slot {
            args.push(("slot".to_owned(), slot.clone()));
        }
        if let Some(from) = site.inferred_from {
            let name = slot.as_deref().unwrap_or("the slot");
            let origin = site.inferred_text.as_deref().map_or_else(
                || format!("the expression at bytes {}..{}", from.start, from.end),
                |text| format!("`{text}`"),
            );
            message = format!(
                "{message}; `{name}` was inferred as `{ty}` from {origin}, declare its type"
            );
            args.push((
                "inferred_from".to_owned(),
                format!("{}..{}", from.start, from.end),
            ));
        }
        if negative && unsigned {
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
    /// Static semantic type of an operand with a typed slot (temp, plain
    /// place, value id), alias-peeled; `None` for constants and projections.
    /// Only called on the rare mixed-carrier comparison paths.
    pub(crate) fn operand_plain_type(&self, operand: &radix_mir::MirOperand) -> Option<TypeId> {
        use radix_mir::{MirOperand, MirPlaceBase};
        let function_id = self.frames.last()?.function;
        let slots = self.prepared.slot_types.get(&function_id)?;
        let ty = match operand {
            MirOperand::Temp(id) => slots.temp_ty(*id),
            MirOperand::Place(place) if place.projections.is_empty() => match place.base {
                MirPlaceBase::Temp(id) => slots.temp_ty(id),
                MirPlaceBase::Local(id) => slots.local_ty(id),
            },
            MirOperand::Value(id) => self
                .prepared
                .value_types
                .get(&(function_id, *id))
                .map(|ty| ty.semantic_id()),
            _ => None,
        }?;
        // `peel_access_marker` already returns an alias-free id; re-peeling
        // here was dead.
        Some(self.types.peel_access_marker(ty).1)
    }

    /// Whether an operand's static type is the unbounded `inf`.
    pub(crate) fn operand_is_inf(&self, operand: &radix_mir::MirOperand) -> bool {
        self.operand_plain_type(operand).is_some_and(|ty| {
            matches!(
                self.types.get(ty),
                Type::SizedNumeric(Primitive::Numerus, NumericWidth::Inf)
            )
        })
    }

    /// Static decimal width of an operand, when its type is a decimal. Only
    /// called on the rare mixed integer-carrier / float comparison.
    pub(crate) fn operand_decimal_width(
        &self,
        operand: &radix_mir::MirOperand,
    ) -> Option<NumericWidth> {
        match self.types.get(self.operand_plain_type(operand)?) {
            Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal() => Some(*width),
            _ => None,
        }
    }
}

/// The representative of `exact` in `lo‥=hi`: the ring of `hi - lo + 1`
/// values starting at `lo` (a `wrapping<W, lo‥hi>` store).
pub(crate) fn ranged_wrap(exact: i128, lo: i128, hi: i128) -> i128 {
    lo + (exact - lo).rem_euclid(hi - lo + 1)
}

/// Surface a value already inside a ranged cell in the carrier of its width:
/// `UInt` at `u64`, `Int` at every other width.
pub(crate) fn ranged_carrier(value: i128, width: NumericWidth) -> Value {
    if width == NumericWidth::U64 {
        Value::UInt(u64::try_from(value).unwrap_or(0))
    } else {
        Value::Int(i64::try_from(value).unwrap_or(0))
    }
}

/// `inf` beyond the 64-bit carriers reduced into a wrapping ranged cell.
pub(crate) fn ranged_big_wrap(
    big: &handroll::Magnus,
    width: NumericWidth,
    lo: i128,
    hi: i128,
) -> Value {
    let lo_m = handroll::Magnus::from_i128(lo);
    let span = handroll::Magnus::from_i128(hi - lo + 1);
    let offset = (big - &lo_m)
        .mod_floor(&span)
        .ok()
        .and_then(|offset| offset.to_i128())
        .unwrap_or(0);
    ranged_carrier(lo + offset, width)
}

/// `inf` beyond the 64-bit carriers clamped into a saturating ranged cell.
pub(crate) fn ranged_big_clamp(
    big: &handroll::Magnus,
    width: NumericWidth,
    lo: i128,
    hi: i128,
) -> Value {
    let clamped = match big.to_i128() {
        Some(exact) => exact.clamp(lo, hi),
        None if big.is_negative() => lo,
        None => hi,
    };
    ranged_carrier(clamped, width)
}
