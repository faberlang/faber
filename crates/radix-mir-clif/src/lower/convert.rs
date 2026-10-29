//! Explicit numeric conversions (`↦`) between integers, floats, booleans,
//! text and raw bits.
//!
//! The oracle's rules (`radix-mir-runner` `conversio/scalar.rs`): a bare
//! narrowing conversion checks the value and fails on the `⇥` channel instead
//! of wrapping or relabelling; `wrapping<W>` and `saturating<W>` targets reduce
//! or clamp and cannot fail; a float source takes its `via` rounding (toward
//! zero when none is named) and a NaN or out-of-range result fails. A failure
//! with a `⊥` default yields the default; otherwise a failable function
//! returns the failure text on its error channel and any other function ends
//! the run with `error: <text>`.

use radix_mir::MirTypeCheckOp;

use super::{
    BitsWidth, Decline, FloatCC, FunctionLower, InstBuilder, IntCC, Lowered, MemFlagsData,
    MirConversion, MirOperand, MirType, NumericWidth, Primitive, Rep, TrapCode, Type, TypeId,
    TypeTable, Value, Variable, abi, rep_of, resolve, trap_text, types,
};

/// How an integer cell treats an out-of-range value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntPolicy {
    Trap,
    Wrap,
    Saturate,
}

/// An integer cell: its policy, width and the range its `i64` carrier holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct IntKind {
    policy: IntPolicy,
    width: NumericWidth,
    low: i64,
    high: i64,
}

/// The cell kinds an explicit numeric conversion moves between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Num {
    Int(IntKind),
    Float(NumericWidth),
    Bool,
    Text,
    /// A `littera`: a text handle of one letter.
    Char,
    /// A `regex`: a compiled program behind a handle.
    Regex,
    /// A `d64`: an exact decimal behind a handle.
    Dec,
    /// Open data (`valor`, `json`): a box of one tagged payload.
    Dyn,
    /// An unbounded integer behind a handle.
    Inf,
    /// An instant at a precision code (0 s, 1 ms, 2 us, 3 ns).
    Ins(i64),
    /// The absent value.
    Nil,
    Bits(BitsWidth),
}

fn int_kind(table: &TypeTable, id: TypeId) -> Option<IntKind> {
    let (policy, width) = match resolve(table, id) {
        Type::Primitive(Primitive::Numerus) => (IntPolicy::Trap, NumericWidth::I64),
        Type::SizedNumeric(Primitive::Numerus, width) => (IntPolicy::Trap, *width),
        Type::ModularWord(width) => (IntPolicy::Wrap, *width),
        Type::SaturatingWord(width) => (IntPolicy::Saturate, *width),
        _ => return None,
    };
    let (low, high) = width.integer_range()?;
    Some(IntKind {
        policy,
        width,
        low: i64::try_from(low).ok()?,
        // A u64 cell's upper bound is beyond the i64 carrier.
        high: i64::try_from(high).unwrap_or(i64::MAX),
    })
}

/// The cell kind of `id` when it takes part in a numeric conversion.
pub(super) fn num_of(table: &TypeTable, id: TypeId) -> Option<Num> {
    match resolve(table, id) {
        Type::Primitive(Primitive::Instans) => return Some(Num::Ins(0)),
        Type::SizedInstans(precision) => {
            return Some(Num::Ins(match precision {
                radix_types::InstansPrecision::Millis => 1,
                radix_types::InstansPrecision::Micros => 2,
                radix_types::InstansPrecision::Nanos => 3,
            }));
        }
        _ => {}
    }
    if matches!(
        resolve(table, id),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::Inf)
            | Type::ModularWord(NumericWidth::Inf)
            | Type::SaturatingWord(NumericWidth::Inf)
    ) {
        return Some(Num::Inf);
    }
    if matches!(resolve(table, id), Type::Primitive(Primitive::Nihil)) {
        return Some(Num::Nil);
    }
    if matches!(
        resolve(table, id),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::D64)
    ) {
        return Some(Num::Dec);
    }
    if let Some(kind) = int_kind(table, id) {
        return Some(Num::Int(kind));
    }
    match resolve(table, id) {
        Type::Primitive(Primitive::Fractus) => Some(Num::Float(NumericWidth::F64)),
        Type::SizedNumeric(Primitive::Fractus, width) if width.is_float() => {
            Some(Num::Float(*width))
        }
        Type::Primitive(Primitive::Bivalens) => Some(Num::Bool),
        Type::Primitive(Primitive::Textus | Primitive::Ascii) => Some(Num::Text),
        Type::Primitive(Primitive::Littera) => Some(Num::Char),
        Type::Primitive(Primitive::Regex) => Some(Num::Regex),
        Type::Primitive(Primitive::Json | Primitive::Valor) => Some(Num::Dyn),
        // An absent open-data value is the box's null.
        Type::Option(inner)
            if matches!(
                resolve(table, *inner),
                Type::Primitive(Primitive::Json | Primitive::Valor)
            ) =>
        {
            Some(Num::Dyn)
        }
        Type::Bits(width) => Some(Num::Bits(*width)),
        _ => None,
    }
}

/// What lowering a source/target pair needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Plan {
    source: Num,
    target: Num,
}

impl Plan {
    #[cfg(test)]
    pub(crate) fn target(self) -> Num {
        self.target
    }
}

fn signed_width(width: NumericWidth) -> bool {
    matches!(
        width,
        NumericWidth::I8 | NumericWidth::I16 | NumericWidth::I32 | NumericWidth::I64
    )
}

/// Whether a trapping integer cell of `width` can hold a text or float
/// result: a u64 cell reaches beyond the `i64` carrier.
fn carrier_exact(kind: IntKind) -> bool {
    kind.width != NumericWidth::U64
}

/// The plan for an explicit conversion, or `None` when the pair is outside
/// the lowered set.
pub(crate) fn numeric_conversion(
    table: &TypeTable,
    source: TypeId,
    target: TypeId,
) -> Option<Plan> {
    let source = num_of(table, source)?;
    let target = num_of(table, target)?;
    let supported = match (source, target) {
        (Num::Int(_), Num::Int(to)) => match to.policy {
            IntPolicy::Trap | IntPolicy::Saturate => true,
            IntPolicy::Wrap => to.width != NumericWidth::U64,
        },
        (Num::Int(_) | Num::Bool | Num::Float(_) | Num::Text | Num::Dec, Num::Float(_))
        | (Num::Text | Num::Float(_) | Num::Int(_), Num::Bool)
        | (Num::Text | Num::Int(_), Num::Char)
        | (Num::Text, Num::Regex)
        | (Num::Float(_) | Num::Text | Num::Dec, Num::Dec)
        | (
            Num::Float(_) | Num::Bool | Num::Text | Num::Char | Num::Nil | Num::Ins(_) | Num::Inf,
            Num::Dyn,
        )
        | (Num::Dyn, Num::Bool | Num::Text | Num::Inf)
        | (Num::Text | Num::Ins(_) | Num::Dyn, Num::Ins(_)) => true,
        (Num::Float(_) | Num::Text | Num::Dec | Num::Inf, Num::Int(to)) => {
            to.policy == IntPolicy::Trap && carrier_exact(to)
        }
        (Num::Bool | Num::Char, Num::Int(to)) => to.policy == IntPolicy::Trap,
        // Decimal, open-data, instant and unbounded-integer targets take an
        // integer only when its trapping cell fits the carrier.
        (Num::Int(from), Num::Dec | Num::Dyn | Num::Ins(_) | Num::Inf) => {
            from.policy == IntPolicy::Trap && carrier_exact(from)
        }
        // Open data boxes a scalar and unboxes by its tag; a mismatch fails.
        (Num::Dyn, Num::Int(to)) => to.policy == IntPolicy::Trap && to.width == NumericWidth::I64,
        (Num::Dyn, Num::Float(width)) => width == NumericWidth::F64,
        // A signed integer or an unsigned one below 64 bits reads and writes
        // a same-width pattern; a u64 pattern would leave the carrier.
        (Num::Int(from), Num::Bits(width)) => {
            from.policy == IntPolicy::Trap && width.bits() == int_bits(from.width)
        }
        (Num::Bits(width), Num::Int(to)) => {
            to.policy == IntPolicy::Trap && width.bits() == int_bits(to.width)
        }
        (Num::Float(float), Num::Bits(width)) | (Num::Bits(width), Num::Float(float)) => {
            matches!(
                (float, width),
                (NumericWidth::F32, BitsWidth::B32)
                    | (NumericWidth::F64, BitsWidth::B64)
                    | (NumericWidth::F16, BitsWidth::B16)
            )
        }
        _ => false,
    };
    supported.then_some(Plan { source, target })
}

fn int_bits(width: NumericWidth) -> u32 {
    match width {
        NumericWidth::I8 | NumericWidth::U8 => 8,
        NumericWidth::I16 | NumericWidth::U16 => 16,
        NumericWidth::I32 | NumericWidth::U32 => 32,
        _ => 64,
    }
}

/// A list, set or map conversion that lowers to runtime calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Seq {
    /// A list to a set (duplicates collapse).
    ToSet,
    /// A map to the list of its keys or its values.
    MapProject { values: bool },
    /// A list copied into a list or a bounded list of the same element type;
    /// a capacity makes an over-long source fail.
    Copy { capacity: Option<u64> },
    /// A list whose elements keep their bits and change element tag
    /// (`list<b8>` and `list<u8>`).
    Retag { tag: i64, capacity: Option<u64> },
    /// A list regrouped at another element width through its bytes, in the
    /// byte order of the conversion's `via` hint; `kind` is the target
    /// element kind (0 raw bits, 1 unsigned, 2 signed).
    Rewidth { from: u32, to: u32, kind: i64 },
    /// A list of bytes read as one scalar in the byte order of the
    /// conversion's `via` hint; the list must hold exactly `bytes` bytes.
    ReadScalar {
        bytes: u32,
        signed: bool,
        float: bool,
    },
    /// A scalar written as its own bytes in the byte order of the conversion's
    /// `via` hint, into a byte buffer of exactly that capacity.
    WriteScalar { bytes: u32, float: bool },
    /// Text to a list of UTF-8, UTF-16 or UTF-32 units of the given width.
    Encode(u32),
    /// A list of unit patterns to text; 8, 16 and 32 decode UTF-8, UTF-16 and
    /// UTF-32, and 1 narrows bytes to ascii. A malformed list fails.
    Decode(u32),
    /// Text into a bounded text or ascii; a source of more scalars than the
    /// capacity fails.
    BoundText { capacity: u64 },
    /// A bounded text or ascii released to its unbounded text.
    Release,
    /// A non-negative integer as zero-padded digits filling a bounded ascii
    /// (the radix comes from the conversion's `via` hint).
    Pack { capacity: u64 },
}

fn list_element(table: &TypeTable, id: TypeId) -> Option<TypeId> {
    match resolve(table, id) {
        Type::Array(element) | Type::BoundedArray { element, .. } => Some(*element),
        Type::BoundedOcteti { .. } | Type::Primitive(Primitive::Octeti) => byte_element(table),
        _ => None,
    }
}

/// The raw byte type, which a bounded byte buffer holds; `None` when the
/// program never names one.
pub(crate) fn byte_element(table: &TypeTable) -> Option<TypeId> {
    table.find_equal(&Type::Bits(BitsWidth::B8))
}

fn same_type(table: &TypeTable, a: TypeId, b: TypeId) -> bool {
    table.equals(a, b)
}

/// A bits element and an unsigned integer of the same width hold the same
/// pattern: only the element tag differs.
fn bits_unsigned_pair(table: &TypeTable, a: TypeId, b: TypeId) -> bool {
    let pair = |bits: &Type, int: &Type| match (bits, int) {
        (Type::Bits(width), Type::SizedNumeric(Primitive::Numerus, int_width)) => {
            width.bits() < 64 && int_bits(*int_width) == width.bits() && !signed_width(*int_width)
        }
        _ => false,
    };
    let (a, b) = (resolve(table, a), resolve(table, b));
    pair(a, b) || pair(b, a)
}

/// The scalar a byte list reads as: 2, 4 and 8 byte integers (a 64-bit
/// unsigned is unrepresentable) and 4 and 8 byte floats.
fn read_scalar(target: &Type) -> Option<Seq> {
    Some(match target {
        Type::SizedNumeric(Primitive::Numerus, width) => match width {
            NumericWidth::I16 => Seq::ReadScalar {
                bytes: 2,
                signed: true,
                float: false,
            },
            NumericWidth::U16 => Seq::ReadScalar {
                bytes: 2,
                signed: false,
                float: false,
            },
            NumericWidth::I32 => Seq::ReadScalar {
                bytes: 4,
                signed: true,
                float: false,
            },
            NumericWidth::U32 => Seq::ReadScalar {
                bytes: 4,
                signed: false,
                float: false,
            },
            NumericWidth::I64 => Seq::ReadScalar {
                bytes: 8,
                signed: true,
                float: false,
            },
            _ => return None,
        },
        Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32) => Seq::ReadScalar {
            bytes: 4,
            signed: false,
            float: true,
        },
        Type::SizedNumeric(Primitive::Fractus, NumericWidth::F64) => Seq::ReadScalar {
            bytes: 8,
            signed: false,
            float: true,
        },
        _ => return None,
    })
}

/// An integer or raw-bits element: its width in bits and kind (0 raw bits, 1
/// unsigned, 2 signed). A 64-bit unsigned element is unrepresentable.
fn bits_cell(table: &TypeTable, id: TypeId) -> Option<(u32, i64)> {
    Some(match resolve(table, id) {
        Type::Bits(width) => (width.bits(), 0),
        Type::SizedNumeric(Primitive::Numerus, width) => match width {
            NumericWidth::U8 => (8, 1),
            NumericWidth::U16 => (16, 1),
            NumericWidth::U32 => (32, 1),
            NumericWidth::I8 => (8, 2),
            NumericWidth::I16 => (16, 2),
            NumericWidth::I32 => (32, 2),
            NumericWidth::I64 => (64, 2),
            _ => return None,
        },
        _ => return None,
    })
}

/// A width change between element kinds where one side is raw bits.
fn rewidth(table: &TypeTable, source: TypeId, target_element: TypeId) -> Option<Seq> {
    let (from, from_kind) = bits_cell(table, list_element(table, source)?)?;
    let (to, kind) = bits_cell(table, target_element)?;
    (from != to && (from_kind == 0 || kind == 0)).then_some(Seq::Rewidth { from, to, kind })
}

/// The plan for an explicit collection conversion, or `None` when the pair
/// is outside the lowered set.
pub(crate) fn sequence_conversion(
    table: &TypeTable,
    source: TypeId,
    target: TypeId,
) -> Option<Seq> {
    let rep_ok = |id: TypeId| rep_of(table, MirType::semantic(id)).is_ok();
    let target_type = resolve(table, target);
    let literal = |capacity: &radix_types::IndexId| match table.get_index(*capacity) {
        radix_types::IndexExpr::Literal(bound) => Some(*bound),
        _ => None,
    };
    match (resolve(table, source), target_type) {
        (
            Type::Array(_)
            | Type::BoundedArray { .. }
            | Type::BoundedOcteti { .. }
            | Type::Primitive(Primitive::Octeti),
            scalar,
        ) if list_element(table, source).is_some_and(|element| {
            matches!(resolve(table, element), Type::Bits(BitsWidth::B8))
        }) && read_scalar(scalar).is_some() =>
        {
            read_scalar(scalar)
        }
        (Type::SizedNumeric(kind, width), Type::BoundedOcteti { capacity }) => {
            let seq = match (kind, width) {
                (Primitive::Numerus, NumericWidth::I16 | NumericWidth::U16) => (2, false),
                (Primitive::Numerus, NumericWidth::I32 | NumericWidth::U32) => (4, false),
                (Primitive::Numerus, NumericWidth::I64) => (8, false),
                (Primitive::Fractus, NumericWidth::F32) => (4, true),
                (Primitive::Fractus, NumericWidth::F64) => (8, true),
                _ => return None,
            };
            byte_element(table)?;
            (literal(capacity)? == u64::from(seq.0)).then_some(Seq::WriteScalar {
                bytes: seq.0,
                float: seq.1,
            })
        }
        (
            Type::Primitive(Primitive::Textus) | Type::BoundedTextus { .. },
            Type::BoundedTextus { capacity },
        )
        | (
            Type::Primitive(Primitive::Ascii) | Type::BoundedAscii { .. },
            Type::BoundedAscii { capacity },
        ) => Some(Seq::BoundText {
            capacity: literal(capacity)?,
        }),
        (
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _),
            Type::BoundedAscii { capacity },
        ) => match resolve(table, source) {
            Type::SizedNumeric(_, NumericWidth::U64) => None,
            _ => Some(Seq::Pack {
                capacity: literal(capacity)?,
            }),
        },
        (Type::BoundedTextus { .. }, Type::Primitive(Primitive::Textus))
        | (Type::BoundedAscii { .. }, Type::Primitive(Primitive::Textus | Primitive::Ascii)) => {
            Some(Seq::Release)
        }
        (Type::Array(from) | Type::BoundedArray { element: from, .. }, Type::Set(to))
            if same_type(table, *from, *to) && rep_ok(*to) =>
        {
            Some(Seq::ToSet)
        }
        (Type::Primitive(Primitive::Textus | Primitive::Ascii), Type::Array(to)) => {
            match resolve(table, *to) {
                Type::Bits(width) => Some(Seq::Encode(width.bits())),
                _ => None,
            }
        }
        (Type::Array(from), Type::Primitive(Primitive::Textus)) => match resolve(table, *from) {
            Type::Bits(width) => Some(Seq::Decode(width.bits())),
            _ => None,
        },
        (Type::Array(from), Type::Primitive(Primitive::Ascii)) => match resolve(table, *from) {
            Type::Bits(BitsWidth::B8) => Some(Seq::Decode(1)),
            _ => None,
        },
        (Type::Map(key, value), Type::Array(to)) => {
            if same_type(table, *key, *to) && rep_ok(*to) {
                Some(Seq::MapProject { values: false })
            } else if same_type(table, *value, *to) && rep_ok(*to) {
                Some(Seq::MapProject { values: true })
            } else {
                None
            }
        }
        (
            Type::Array(_) | Type::BoundedArray { .. } | Type::BoundedOcteti { .. },
            Type::Array(to),
        ) if rewidth(table, source, *to).is_some() => rewidth(table, source, *to),
        (
            _,
            Type::Array(_)
            | Type::BoundedArray { .. }
            | Type::BoundedOcteti { .. }
            | Type::Primitive(Primitive::Octeti),
        ) => sequence_copy(table, source, target_type),
        _ => None,
    }
}

fn sequence_copy(table: &TypeTable, source: TypeId, target_type: &Type) -> Option<Seq> {
    let rep_ok = |id: TypeId| rep_of(table, MirType::semantic(id)).is_ok();
    let to = &match target_type {
        Type::Array(to) | Type::BoundedArray { element: to, .. } => *to,
        _ => byte_element(table)?,
    };
    let from = list_element(table, source)?;
    let capacity = match target_type {
        Type::BoundedArray { capacity, .. } | Type::BoundedOcteti { capacity } => {
            match table.get_index(*capacity) {
                radix_types::IndexExpr::Literal(bound) => Some(*bound),
                _ => return None,
            }
        }
        _ => None,
    };
    if same_type(table, from, *to) {
        rep_ok(*to).then_some(Seq::Copy { capacity })
    } else if bits_unsigned_pair(table, from, *to) {
        let rep = rep_of(table, MirType::semantic(*to)).ok()?;
        let tag = match rep {
            Rep::Int => abi::TAG_INT,
            Rep::Bits(width) => abi::TAG_BITS + i64::from(width.bits().trailing_zeros() - 3),
            _ => return None,
        };
        Some(Seq::Retag { tag, capacity })
    } else {
        None
    }
}

/// The failure text a conversion raises.
#[derive(Clone, Copy)]
enum Failure {
    Static(&'static str),
    /// A runtime status code (see `abi::CONVERT_MESSAGE`).
    Code(Value),
    /// A message text already built.
    Message(Value),
}

type SequenceValue = (Value, Option<(Value, Failure)>, Rep);

impl FunctionLower<'_, '_> {
    /// Lower an explicit numeric conversion into `variable`.
    pub(super) fn numeric_convert(
        &mut self,
        plan: Plan,
        conversion: &MirConversion,
        value: Value,
        variable: Variable,
    ) -> Lowered<()> {
        let region = self.recovery_region(conversion);
        let recovery = match (&conversion.recovery, region) {
            (Some(operand), None) => Some(self.scalar(operand)?),
            _ => None,
        };
        let (converted, failed) = self.numeric_value(plan, conversion, value)?;
        let want = match plan.target {
            Num::Int(_) => Rep::Int,
            Num::Float(_) => Rep::Float,
            Num::Bool => Rep::Bool,
            Num::Bits(width) => Rep::Bits(width),
            Num::Char => Rep::Char,
            Num::Regex => Rep::Regex,
            Num::Dec => Rep::Dec,
            Num::Dyn => Rep::Opt,
            Num::Inf => Rep::Inf,
            Num::Ins(_) => Rep::Ins,
            Num::Nil => return Err(Decline::new("conversion to the absent value")),
            Num::Text if plan.source == Num::Dyn => Rep::Text,
            Num::Text => return Err(Decline::new("text conversion target")),
        };
        let converted = match (failed, recovery) {
            (None, Some((default, rep)))
                if plan.target == Num::Bool && plan.source != Num::Bool =>
            {
                // `⊥` on a truthiness conversion supplies the default for a falsy source.
                let default = self.coerce(default, rep, Rep::Bool)?;
                let truthy = self.builder.ins().iconst(types::I8, 1);
                self.builder.ins().select(converted, truthy, default)
            }
            (failed, recovery) => {
                self.finish_conversion(converted, failed, recovery, want, region)?
            }
        };
        self.builder.def_var(variable, converted);
        Ok(())
    }

    /// The recovery region a conversion falls into on failure, when its `⊥`
    /// fallback is a side-effecting region rather than a value in hand.
    fn recovery_region(&self, conversion: &MirConversion) -> Option<cranelift_codegen::ir::Block> {
        let Some(MirOperand::Temp(id)) = &conversion.recovery else {
            return None;
        };
        let entry = self.recovery_regions.get(id)?;
        self.blocks.get(entry).copied()
    }

    /// Apply a conversion's failure: the `⊥` default replaces the value, and
    /// without one the failure is raised.
    fn finish_conversion(
        &mut self,
        converted: Value,
        failed: Option<(Value, Failure)>,
        recovery: Option<(Value, Rep)>,
        want: Rep,
        region: Option<cranelift_codegen::ir::Block>,
    ) -> Lowered<Value> {
        if let (Some(region), Some((failed, _))) = (region, &failed) {
            // The fallback runs as its own blocks and stores the destination.
            let next = self.builder.create_block();
            self.builder.ins().brif(*failed, region, &[], next, &[]);
            self.builder.switch_to_block(next);
            return Ok(converted);
        }
        Ok(match (failed, recovery) {
            (Some((failed, _)), Some((default, rep))) => {
                let default = self.coerce(default, rep, want)?;
                self.builder.ins().select(failed, default, converted)
            }
            (Some((failed, failure)), None) => {
                self.raise(failed, failure)?;
                converted
            }
            (None, _) => converted,
        })
    }

    /// Lower an explicit list, set or map conversion into `variable`.
    pub(super) fn sequence_convert(
        &mut self,
        plan: Seq,
        conversion: &MirConversion,
        value: Value,
        variable: Variable,
    ) -> Lowered<()> {
        let region = self.recovery_region(conversion);
        let recovery = match (&conversion.recovery, region) {
            (Some(operand), None) => Some(self.scalar(operand)?),
            _ => None,
        };
        let (converted, failed, want) = match plan {
            Seq::ToSet => (
                self.call_runtime(&abi::LIST_TO_SET, &[value])?
                    .ok_or_else(|| Decline::new("set result"))?,
                None,
                Rep::Set,
            ),
            Seq::MapProject { values } => {
                let symbol = if values {
                    &abi::MAP_VALUES
                } else {
                    &abi::MAP_KEYS
                };
                (
                    self.call_runtime(symbol, &[value])?
                        .ok_or_else(|| Decline::new("projection result"))?,
                    None,
                    Rep::List,
                )
            }
            Seq::Retag { tag, capacity } => {
                let tag = self.builder.ins().iconst(types::I32, tag);
                let retagged = self
                    .call_runtime(&abi::LIST_RETAG, &[value, tag])?
                    .ok_or_else(|| Decline::new("retag result"))?;
                let failed = self.over_capacity(value, capacity)?;
                (retagged, failed, Rep::List)
            }
            Seq::Encode(width) => {
                let width = self.builder.ins().iconst(types::I32, i64::from(width));
                (
                    self.call_runtime(&abi::TEXT_ENCODE, &[value, width])?
                        .ok_or_else(|| Decline::new("encode result"))?,
                    None,
                    Rep::List,
                )
            }
            Seq::Decode(mode) => {
                let mode = self.builder.ins().iconst(types::I32, i64::from(mode));
                let decoded = self
                    .call_runtime(&abi::LIST_DECODE, &[value, mode])?
                    .ok_or_else(|| Decline::new("decode result"))?;
                let zero = self.builder.ins().iconst(types::I64, 0);
                let failed = self.builder.ins().icmp(IntCC::Equal, decoded, zero);
                (
                    decoded,
                    Some((
                        failed,
                        Failure::Static("bits list to textus conversion failed"),
                    )),
                    Rep::Text,
                )
            }
            Seq::Rewidth { from, to, kind } => {
                self.sequence_rewidth(conversion, value, from, to, kind)?
            }
            Seq::ReadScalar {
                bytes,
                signed,
                float,
            } => self.sequence_read_scalar(conversion, value, bytes, signed, float)?,
            Seq::WriteScalar { bytes, float } => {
                self.sequence_write_scalar(conversion, value, bytes, float)?
            }
            Seq::Release => (value, None, Rep::Text),
            Seq::BoundText { capacity } => self.sequence_bound_text(value, capacity)?,
            Seq::Pack { capacity } => self.sequence_pack(conversion, value, capacity)?,
            Seq::Copy { capacity } => {
                let start = self.builder.ins().iconst(types::I64, 0);
                let end = self.builder.ins().iconst(types::I64, i64::MAX);
                let copied = self
                    .call_runtime(&abi::LIST_SLICE, &[value, start, end])?
                    .ok_or_else(|| Decline::new("copy result"))?;
                let failed = self.over_capacity(value, capacity)?;
                (copied, failed, Rep::List)
            }
        };
        let converted = self.finish_conversion(converted, failed, recovery, want, region)?;
        self.builder.def_var(variable, converted);
        Ok(())
    }

    fn byte_order_flag(
        &self,
        conversion: &MirConversion,
        missing_hint: &'static str,
    ) -> Lowered<i64> {
        conversion
            .params
            .iter()
            .find_map(|param| match self.state.input.interner.resolve(*param) {
                "be" | "Be" => Some(1),
                "le" | "Le" => Some(0),
                _ => None,
            })
            .ok_or_else(|| Decline::new(missing_hint))
    }

    fn sequence_rewidth(
        &mut self,
        conversion: &MirConversion,
        value: Value,
        from: u32,
        to: u32,
        kind: i64,
    ) -> Lowered<SequenceValue> {
        Ok({
            let big = self.byte_order_flag(conversion, "list regroup without a byte order")?;
            let from = self.builder.ins().iconst(types::I32, i64::from(from));
            let to = self.builder.ins().iconst(types::I32, i64::from(to));
            let flags = self.builder.ins().iconst(types::I32, big | (kind << 1));
            let regrouped = self
                .call_runtime(&abi::LIST_REWIDTH, &[value, from, to, flags])?
                .ok_or_else(|| Decline::new("regroup result"))?;
            let zero = self.builder.ins().iconst(types::I64, 0);
            let failed = self.builder.ins().icmp(IntCC::Equal, regrouped, zero);
            (
                regrouped,
                Some((failed, Failure::Static("bits list length mismatch"))),
                Rep::List,
            )
        })
    }

    fn sequence_read_scalar(
        &mut self,
        conversion: &MirConversion,
        value: Value,
        bytes: u32,
        signed: bool,
        float: bool,
    ) -> Lowered<SequenceValue> {
        Ok({
            let big = self.byte_order_flag(conversion, "byte read without a byte order")?;
            let flags = big | if signed { 2 } else { 0 } | if float { 4 } else { 0 };
            let width = self.builder.ins().iconst(types::I32, i64::from(bytes));
            let flags = self.builder.ins().iconst(types::I32, flags);
            let bits = self
                .call_runtime(&abi::LIST_READ_SCALAR, &[value, width, flags])?
                .ok_or_else(|| Decline::new("byte read result"))?;
            let length = self
                .call_runtime(&abi::LIST_LEN, &[value])?
                .ok_or_else(|| Decline::new("length result"))?;
            let need = self.builder.ins().iconst(types::I64, i64::from(bytes));
            let wrong = self.builder.ins().icmp(IntCC::NotEqual, length, need);
            let failure = Some((
                wrong,
                Failure::Static(trap_text::BYTES_ENDIAN_WIDTH_MISMATCH),
            ));
            if float {
                let float = self
                    .builder
                    .ins()
                    .bitcast(types::F64, MemFlagsData::new(), bits);
                (float, failure, Rep::Float)
            } else {
                (bits, failure, Rep::Int)
            }
        })
    }

    fn sequence_write_scalar(
        &mut self,
        conversion: &MirConversion,
        value: Value,
        bytes: u32,
        float: bool,
    ) -> Lowered<SequenceValue> {
        Ok({
            let big = self.byte_order_flag(conversion, "byte write without a byte order")?;
            let bits = if float {
                self.builder
                    .ins()
                    .bitcast(types::I64, MemFlagsData::new(), value)
            } else {
                value
            };
            let width = self.builder.ins().iconst(types::I32, i64::from(bytes));
            let flags = self
                .builder
                .ins()
                .iconst(types::I32, big | if float { 4 } else { 0 });
            (
                self.call_runtime(&abi::SCALAR_BYTES, &[bits, width, flags])?
                    .ok_or_else(|| Decline::new("byte write result"))?,
                None,
                Rep::List,
            )
        })
    }

    fn sequence_bound_text(&mut self, value: Value, capacity: u64) -> Lowered<SequenceValue> {
        Ok({
            let length = self
                .call_runtime(&abi::TEXT_LEN, &[value])?
                .ok_or_else(|| Decline::new("length result"))?;
            let bound = self.builder.ins().iconst(
                types::I64,
                i64::try_from(capacity).map_err(|_| Decline::new("capacity"))?,
            );
            let over = self
                .builder
                .ins()
                .icmp(IntCC::SignedGreaterThan, length, bound);
            (
                value,
                Some((
                    over,
                    Failure::Static("text to bounded text conversion failed"),
                )),
                Rep::Text,
            )
        })
    }

    fn sequence_pack(
        &mut self,
        conversion: &MirConversion,
        value: Value,
        capacity: u64,
    ) -> Lowered<SequenceValue> {
        Ok({
            let radix = conversion
                .params
                .iter()
                .find_map(|param| match self.state.input.interner.resolve(*param) {
                    "hex" | "Hex" => Some(16),
                    "bin" | "Bin" => Some(2),
                    "oct" | "Oct" => Some(8),
                    _ => None,
                })
                .ok_or_else(|| Decline::new("ascii pack without a radix"))?;
            let width = self.builder.ins().iconst(
                types::I64,
                i64::try_from(capacity).map_err(|_| Decline::new("capacity"))?,
            );
            let radix = self.builder.ins().iconst(types::I32, radix);
            let packed = self
                .call_runtime(&abi::TEXT_PACK, &[value, width, radix])?
                .ok_or_else(|| Decline::new("pack result"))?;
            let zero = self.builder.ins().iconst(types::I64, 0);
            let failed = self.builder.ins().icmp(IntCC::Equal, packed, zero);
            (
                packed,
                Some((failed, Failure::Static("ascii_pack_width_overflow"))),
                Rep::Text,
            )
        })
    }

    /// The failure of a list longer than a bounded capacity, if there is one.
    fn over_capacity(
        &mut self,
        list: Value,
        capacity: Option<u64>,
    ) -> Lowered<Option<(Value, Failure)>> {
        let Some(capacity) = capacity else {
            return Ok(None);
        };
        let length = self
            .call_runtime(&abi::LIST_LEN, &[list])?
            .ok_or_else(|| Decline::new("length result"))?;
        let bound = self.builder.ins().iconst(
            types::I64,
            i64::try_from(capacity).map_err(|_| Decline::new("capacity"))?,
        );
        let over = self
            .builder
            .ins()
            .icmp(IntCC::SignedGreaterThan, length, bound);
        Ok(Some((
            over,
            Failure::Static("lista conversion exceeds the bounded capacity"),
        )))
    }

    /// The converted value plus, when the conversion can fail, the failure
    /// flag and its text. A failing source still produces a defined value.
    fn numeric_value(
        &mut self,
        plan: Plan,
        conversion: &MirConversion,
        value: Value,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok(match (plan.source, plan.target) {
            (Num::Int(from), Num::Int(to)) => self.int_to_int(from, to, value),
            (Num::Int(_), Num::Float(width)) => self.convert_int_float(value, width),
            (Num::Bool, Num::Float(_)) => {
                let wide = self.builder.ins().uextend(types::I64, value);
                (self.builder.ins().fcvt_from_sint(types::F64, wide), None)
            }
            (Num::Bool, Num::Int(_)) => (self.builder.ins().uextend(types::I64, value), None),
            (Num::Float(_), Num::Float(width)) => {
                // A float target narrower than `f64` rounds at the slot
                // through `f32` (the runner's `round_float_at_slot`).
                (self.round_float_width(value, width), None)
            }
            (Num::Nil, Num::Dyn) => (value, None),
            (Num::Float(_), Num::Int(to)) => self.float_to_int(conversion, to, value),
            (Num::Text, Num::Int(to)) => self.text_to_int(conversion, to, value)?,
            (Num::Int(_) | Num::Float(_) | Num::Bool | Num::Text | Num::Char, Num::Dyn) => {
                self.box_numeric_scalar(plan, value)?
            }
            (Num::Dyn, target)
                if matches!(target, Num::Int(_) | Num::Float(_) | Num::Bool | Num::Text) =>
            {
                self.unbox_numeric_scalar(value, target)?
            }
            (Num::Text, Num::Ins(code)) => self.convert_text_ins(value, code)?,
            (Num::Int(_), Num::Ins(code)) => self.convert_int_ins(value, code)?,
            (Num::Ins(_), Num::Ins(code)) => self.convert_ins_ins(value, code)?,
            (Num::Ins(_), Num::Dyn) => self.convert_ins_dyn(value)?,
            (Num::Dyn, Num::Ins(code)) => self.convert_dyn_ins(value, code)?,
            (Num::Int(_), Num::Inf) => self.convert_int_inf(value)?,
            (Num::Inf, Num::Int(to)) => self.convert_inf_int(value, to)?,
            (Num::Inf, Num::Dyn) => self.convert_inf_dyn(value)?,
            (Num::Dyn, Num::Inf) => self.convert_dyn_inf(value)?,
            (Num::Int(_), Num::Dec) => self.decimal_in(&abi::DEC_FROM_INT, value)?,
            (Num::Float(_), Num::Dec) => self.decimal_in(&abi::DEC_FROM_FLOAT, value)?,
            (Num::Text, Num::Dec) => self.decimal_in(&abi::DEC_FROM_TEXT, value)?,
            // The same width: the value lands at scale 8, or fails out of range.
            (Num::Dec, Num::Dec) => self.convert_dec_dec(value)?,
            (Num::Dec, Num::Float(width)) => self.convert_dec_float(value, width)?,
            (Num::Dec, Num::Int(to)) => self.convert_dec_int(conversion, value, to)?,
            (Num::Text, Num::Regex) => self.convert_text_regex(value)?,
            (Num::Text, Num::Char) => self.convert_text_char(value)?,
            (Num::Int(_), Num::Char) => self.convert_int_char(conversion, value)?,
            (Num::Char, Num::Int(_)) => self.convert_char_int(conversion, value)?,
            (Num::Text, Num::Float(width)) => {
                let (parsed, failure) = self.convert_text_float(value)?;
                // Text parses in `f64` and narrows at the slot: the double
                // rounding is accepted for text sources, as in the runner.
                (self.round_float_width(parsed, width), failure)
            }
            (Num::Int(_), Num::Bool) => {
                let zero = self.builder.ins().iconst(types::I64, 0);
                (self.builder.ins().icmp(IntCC::NotEqual, value, zero), None)
            }
            (Num::Float(_), Num::Bool) => self.convert_float_bool(value),
            (Num::Text, Num::Bool) => self.convert_text_bool(value)?,
            (Num::Int(_), Num::Bits(width)) => (self.mask_pattern(value, width), None),
            (Num::Bits(width), Num::Int(to)) => self.convert_bits_int(value, to, width),
            (Num::Float(NumericWidth::F16), Num::Bits(_)) => self.convert_float_bits_half(value)?,
            (Num::Bits(_), Num::Float(NumericWidth::F16)) => self.convert_bits_float_half(value)?,
            (Num::Float(_), Num::Bits(width)) => self.convert_float_bits(value, width),
            (Num::Bits(width), Num::Float(_)) => self.convert_bits_float(value, width),
            _ => return Err(Decline::new("explicit numeric conversion")),
        })
    }

    fn convert_int_float(
        &mut self,
        value: Value,
        width: NumericWidth,
    ) -> (Value, Option<(Value, Failure)>) {
        let widened = if width == NumericWidth::F32 {
            // Rounded once, nearest-even, straight from the integer.
            let narrow = self.builder.ins().fcvt_from_sint(types::F32, value);
            self.builder.ins().fpromote(types::F64, narrow)
        } else {
            self.builder.ins().fcvt_from_sint(types::F64, value)
        };
        (widened, None)
    }

    fn box_numeric_scalar(
        &mut self,
        plan: Plan,
        value: Value,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let rep = match plan.source {
                Num::Int(_) => Rep::Int,
                Num::Float(_) => Rep::Float,
                Num::Bool => Rep::Bool,
                Num::Char => Rep::Char,
                _ => Rep::Text,
            };
            (self.wrap_some(value, rep)?, None)
        })
    }

    fn unbox_numeric_scalar(
        &mut self,
        value: Value,
        target: Num,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let (tag, rep, message) = match target {
                Num::Int(_) => (abi::TAG_INT, Rep::Int, "valor to numerus conversion failed"),
                Num::Float(_) => (
                    abi::TAG_FLOAT,
                    Rep::Float,
                    "valor to fractus conversion failed",
                ),
                Num::Bool => (
                    abi::TAG_BOOL,
                    Rep::Bool,
                    "valor to bivalens conversion failed",
                ),
                _ => (
                    abi::TAG_TEXT,
                    Rep::Text,
                    "valor to textus conversion failed",
                ),
            };
            let wanted = self.builder.ins().iconst(types::I32, tag);
            let has = self
                .call_runtime(&abi::UNION_HAS_TAG, &[value, wanted])?
                .ok_or_else(|| Decline::new("tag test result"))?;
            let zero = self.builder.ins().iconst(types::I64, 0);
            let bits = self
                .call_runtime(&abi::OPT_BITS_OR, &[value, zero])?
                .ok_or_else(|| Decline::new("payload result"))?;
            let mut extracted = self.element_value(bits, rep);
            let mut has = has;
            // A float also takes an integer payload, widened.
            if rep == Rep::Float {
                let int_tag = self.builder.ins().iconst(types::I32, abi::TAG_INT);
                let is_int = self
                    .call_runtime(&abi::UNION_HAS_TAG, &[value, int_tag])?
                    .ok_or_else(|| Decline::new("tag test result"))?;
                let widened = self.builder.ins().fcvt_from_sint(types::F64, bits);
                extracted = self.builder.ins().select(is_int, widened, extracted);
                has = self.builder.ins().bor(has, is_int);
            }
            let one = self.builder.ins().iconst(types::I8, 1);
            let failed = self.builder.ins().bxor(has, one);
            (extracted, Some((failed, Failure::Static(message))))
        })
    }

    fn convert_text_ins(
        &mut self,
        value: Value,
        code: i64,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let code = self.builder.ins().iconst(types::I32, code);
            let instant = self
                .call_runtime(&abi::INS_PARSE, &[value, code])?
                .ok_or_else(|| Decline::new("instant result"))?;
            let zero = self.builder.ins().iconst(types::I64, 0);
            let failed = self.builder.ins().icmp(IntCC::Equal, instant, zero);
            (
                instant,
                Some((
                    failed,
                    Failure::Static("textus to instans conversion failed"),
                )),
            )
        })
    }

    fn convert_int_ins(
        &mut self,
        value: Value,
        code: i64,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let code = self.builder.ins().iconst(types::I32, code);
            let instant = self
                .call_runtime(&abi::INS_FROM_EPOCH, &[value, code])?
                .ok_or_else(|| Decline::new("instant result"))?;
            (instant, None)
        })
    }

    fn convert_ins_ins(
        &mut self,
        value: Value,
        code: i64,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let code = self.builder.ins().iconst(types::I32, code);
            let instant = self
                .call_runtime(&abi::INS_RETAG, &[value, code])?
                .ok_or_else(|| Decline::new("instant result"))?;
            (instant, None)
        })
    }

    fn convert_ins_dyn(&mut self, value: Value) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let boxed = self
                .call_runtime(&abi::INS_BOX, &[value])?
                .ok_or_else(|| Decline::new("open data result"))?;
            (boxed, None)
        })
    }

    fn convert_dyn_ins(
        &mut self,
        value: Value,
        code: i64,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let code = self.builder.ins().iconst(types::I32, code);
            let instant = self
                .call_runtime(&abi::INS_UNBOX, &[value, code])?
                .ok_or_else(|| Decline::new("instant result"))?;
            let zero = self.builder.ins().iconst(types::I64, 0);
            let failed = self.builder.ins().icmp(IntCC::Equal, instant, zero);
            (
                instant,
                Some((
                    failed,
                    Failure::Static("valor to instans conversion failed"),
                )),
            )
        })
    }

    fn convert_int_inf(&mut self, value: Value) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let big = self
                .call_runtime(&abi::INF_INT, &[value])?
                .ok_or_else(|| Decline::new("inf result"))?;
            (big, None)
        })
    }

    fn convert_inf_int(
        &mut self,
        value: Value,
        to: IntKind,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let low = self.builder.ins().iconst(types::I64, to.low);
            let high = self.builder.ins().iconst(types::I64, to.high);
            let integer = self
                .call_runtime(&abi::INF_TO_INT, &[value, low, high])?
                .ok_or_else(|| Decline::new("inf integer result"))?;
            let status = self.parse_status()?;
            let zero = self.builder.ins().iconst(types::I32, 0);
            let failed = self.builder.ins().icmp(IntCC::NotEqual, status, zero);
            (integer, Some((failed, Failure::Code(status))))
        })
    }

    fn convert_inf_dyn(&mut self, value: Value) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let boxed = self
                .call_runtime(&abi::INF_BOX, &[value])?
                .ok_or_else(|| Decline::new("open data result"))?;
            (boxed, None)
        })
    }

    fn convert_dyn_inf(&mut self, value: Value) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let big = self
                .call_runtime(&abi::INF_UNBOX, &[value])?
                .ok_or_else(|| Decline::new("inf result"))?;
            let zero = self.builder.ins().iconst(types::I64, 0);
            let failed = self.builder.ins().icmp(IntCC::Equal, big, zero);
            (
                big,
                Some((
                    failed,
                    Failure::Static("valor to numerus conversion failed"),
                )),
            )
        })
    }

    fn convert_dec_dec(&mut self, value: Value) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let stored = self
                .call_runtime(&abi::DEC_STORE, &[value])?
                .ok_or_else(|| Decline::new("decimal store result"))?;
            let zero = self.builder.ins().iconst(types::I64, 0);
            let failed = self.builder.ins().icmp(IntCC::Equal, stored, zero);
            (
                stored,
                Some((
                    failed,
                    Failure::Static("textus to numerus conversion out of range"),
                )),
            )
        })
    }

    fn convert_dec_float(
        &mut self,
        value: Value,
        width: NumericWidth,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let narrow = self
                .builder
                .ins()
                .iconst(types::I32, i64::from(width != NumericWidth::F64));
            let float = self
                .call_runtime(&abi::DEC_TO_FLOAT, &[value, narrow])?
                .ok_or_else(|| Decline::new("decimal float result"))?;
            (float, None)
        })
    }

    fn convert_dec_int(
        &mut self,
        conversion: &MirConversion,
        value: Value,
        to: IntKind,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let mode = match self.rounding_mode_or(&conversion.params, Rounding::HalfEven) {
                Rounding::Floor => 0,
                Rounding::Ceil => 1,
                Rounding::HalfEven => 2,
                Rounding::Trunc => 3,
            };
            let mode = self.builder.ins().iconst(types::I32, mode);
            let low = self.builder.ins().iconst(types::I64, to.low);
            let high = self.builder.ins().iconst(types::I64, to.high);
            let integer = self
                .call_runtime(&abi::DEC_TO_INT, &[value, mode, low, high])?
                .ok_or_else(|| Decline::new("decimal integer result"))?;
            let status = self.parse_status()?;
            let zero = self.builder.ins().iconst(types::I32, 0);
            let failed = self.builder.ins().icmp(IntCC::NotEqual, status, zero);
            (integer, Some((failed, Failure::Code(status))))
        })
    }

    fn convert_text_regex(&mut self, value: Value) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let program = self
                .call_runtime(&abi::REGEX_COMPILE, &[value])?
                .ok_or_else(|| Decline::new("regex result"))?;
            let zero = self.builder.ins().iconst(types::I64, 0);
            let failed = self.builder.ins().icmp(IntCC::Equal, program, zero);
            let message = self
                .call_runtime(&abi::REGEX_ERROR, &[])?
                .ok_or_else(|| Decline::new("regex error result"))?;
            (program, Some((failed, Failure::Message(message))))
        })
    }

    fn convert_text_char(&mut self, value: Value) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let letter = self
                .call_runtime(&abi::CHAR_FROM_TEXT, &[value])?
                .ok_or_else(|| Decline::new("letter result"))?;
            self.letter_or_failure(letter)
        })
    }

    fn convert_int_char(
        &mut self,
        conversion: &MirConversion,
        value: Value,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let symbol = if self.has_code_point_hint(&conversion.params) {
                &abi::CHAR_FROM_CODE
            } else {
                &abi::CHAR_FROM_DIGIT
            };
            let letter = self
                .call_runtime(symbol, &[value])?
                .ok_or_else(|| Decline::new("letter result"))?;
            self.letter_or_failure(letter)
        })
    }

    fn convert_char_int(
        &mut self,
        conversion: &MirConversion,
        value: Value,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            if self.has_code_point_hint(&conversion.params) {
                let code = self
                    .call_runtime(&abi::CHAR_CODE, &[value])?
                    .ok_or_else(|| Decline::new("code point result"))?;
                (code, None)
            } else {
                let digit = self
                    .call_runtime(&abi::CHAR_DIGIT, &[value])?
                    .ok_or_else(|| Decline::new("digit result"))?;
                let zero = self.builder.ins().iconst(types::I64, 0);
                let failed = self.builder.ins().icmp(IntCC::SignedLessThan, digit, zero);
                (
                    digit,
                    Some((
                        failed,
                        Failure::Static("littera to numerus conversion failed"),
                    )),
                )
            }
        })
    }

    fn convert_text_float(&mut self, value: Value) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let parsed = self
                .call_runtime(&abi::PARSE_F64, &[value])?
                .ok_or_else(|| Decline::new("parse result"))?;
            let status = self.parse_status()?;
            let zero = self.builder.ins().iconst(types::I32, 0);
            let failed = self.builder.ins().icmp(IntCC::NotEqual, status, zero);
            (parsed, Some((failed, Failure::Code(status))))
        })
    }

    fn convert_float_bool(&mut self, value: Value) -> (Value, Option<(Value, Failure)>) {
        let zero = self.builder.ins().f64const(0.0);
        (
            self.builder.ins().fcmp(FloatCC::NotEqual, value, zero),
            None,
        )
    }

    fn convert_text_bool(&mut self, value: Value) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let parsed = self
                .call_runtime(&abi::TEXT_PARSE_BOOL, &[value])?
                .ok_or_else(|| Decline::new("text parse result"))?;
            let zero = self.builder.ins().iconst(types::I8, 0);
            (
                self.builder.ins().icmp(IntCC::NotEqual, parsed, zero),
                Some((
                    self.builder.ins().icmp(IntCC::SignedLessThan, parsed, zero),
                    Failure::Static("text to bivalens conversion failed"),
                )),
            )
        })
    }

    fn convert_bits_int(
        &mut self,
        value: Value,
        to: IntKind,
        width: BitsWidth,
    ) -> (Value, Option<(Value, Failure)>) {
        let read = self.extend_pattern(value, width, signed_width(to.width));
        if to.width == NumericWidth::U64 {
            // A pattern with the top bit set is a u64 the i64 carrier
            // cannot hold: refuse it by name at run time.
            let zero = self.builder.ins().iconst(types::I64, 0);
            let beyond = self.builder.ins().icmp(IntCC::SignedLessThan, read, zero);
            return (
                read,
                Some((
                    beyond,
                    Failure::Static("u64 value beyond 2^63 is not carried by the clif target"),
                )),
            );
        }
        (read, None)
    }

    fn convert_float_bits_half(
        &mut self,
        value: Value,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let pattern = self
                .call_runtime(&abi::F16_TO_BITS, &[value])?
                .ok_or_else(|| Decline::new("f16 pattern result"))?;
            (pattern, None)
        })
    }

    fn convert_bits_float_half(
        &mut self,
        value: Value,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        Ok({
            let float = self
                .call_runtime(&abi::F16_FROM_BITS, &[value])?
                .ok_or_else(|| Decline::new("f16 value result"))?;
            (float, None)
        })
    }

    fn convert_float_bits(
        &mut self,
        value: Value,
        width: BitsWidth,
    ) -> (Value, Option<(Value, Failure)>) {
        let pattern = if width == BitsWidth::B32 {
            let narrow = self.builder.ins().fdemote(types::F32, value);
            let bits = self
                .builder
                .ins()
                .bitcast(types::I32, MemFlagsData::new(), narrow);
            self.builder.ins().uextend(types::I64, bits)
        } else {
            self.builder
                .ins()
                .bitcast(types::I64, MemFlagsData::new(), value)
        };
        (pattern, None)
    }

    fn convert_bits_float(
        &mut self,
        value: Value,
        width: BitsWidth,
    ) -> (Value, Option<(Value, Failure)>) {
        let float = if width == BitsWidth::B32 {
            let bits = self.builder.ins().ireduce(types::I32, value);
            let narrow = self
                .builder
                .ins()
                .bitcast(types::F32, MemFlagsData::new(), bits);
            self.builder.ins().fpromote(types::F64, narrow)
        } else {
            self.builder
                .ins()
                .bitcast(types::F64, MemFlagsData::new(), value)
        };
        (float, None)
    }

    /// A letter result: null is the failure.
    fn letter_or_failure(&mut self, letter: Value) -> (Value, Option<(Value, Failure)>) {
        let zero = self.builder.ins().iconst(types::I64, 0);
        let failed = self.builder.ins().icmp(IntCC::Equal, letter, zero);
        (
            letter,
            Some((failed, Failure::Static("littera conversion failed"))),
        )
    }

    /// Whether a conversion names the `Code` hint.
    fn has_code_point_hint(&self, params: &[radix_lexer::Symbol]) -> bool {
        params
            .iter()
            .any(|param| radix_types::is_code_point_hint(self.state.input.interner.resolve(*param)))
    }

    fn parse_status(&mut self) -> Lowered<Value> {
        self.call_runtime(&abi::PARSE_STATUS, &[])?
            .ok_or_else(|| Decline::new("parse status result"))
    }

    /// The low `width` bits of `value`, zero-extended.
    fn mask_pattern(&mut self, value: Value, width: BitsWidth) -> Value {
        if width.bits() == 64 {
            return value;
        }
        let mask = self
            .builder
            .ins()
            .iconst(types::I64, (1i64 << width.bits()) - 1);
        self.builder.ins().band(value, mask)
    }

    /// A `width`-bit pattern read as an integer: sign-extended when signed.
    fn extend_pattern(&mut self, pattern: Value, width: BitsWidth, signed: bool) -> Value {
        if width.bits() == 64 || !signed {
            return pattern;
        }
        let shift = i64::from(64 - width.bits());
        let raised = self.builder.ins().ishl_imm_u(pattern, shift);
        self.builder.ins().sshr_imm_u(raised, shift)
    }

    fn int_to_int(
        &mut self,
        from: IntKind,
        to: IntKind,
        value: Value,
    ) -> (Value, Option<(Value, Failure)>) {
        match to.policy {
            IntPolicy::Trap => {
                if from.low >= to.low && from.high <= to.high {
                    return (value, None);
                }
                let below = if to.low == i64::MIN {
                    None
                } else {
                    let bound = self.builder.ins().iconst(types::I64, to.low);
                    Some(self.builder.ins().icmp(IntCC::SignedLessThan, value, bound))
                };
                let above = if to.high == i64::MAX {
                    None
                } else {
                    let bound = self.builder.ins().iconst(types::I64, to.high);
                    Some(
                        self.builder
                            .ins()
                            .icmp(IntCC::SignedGreaterThan, value, bound),
                    )
                };
                let failed = match (below, above) {
                    (Some(below), Some(above)) => self.builder.ins().bor(below, above),
                    (Some(flag), None) | (None, Some(flag)) => flag,
                    (None, None) => return (value, None),
                };
                (
                    value,
                    Some((
                        failed,
                        Failure::Static("numerus to numerus conversion out of range"),
                    )),
                )
            }
            IntPolicy::Saturate => {
                if from.low >= to.low && from.high <= to.high {
                    return (value, None);
                }
                let low = self.builder.ins().iconst(types::I64, to.low);
                let high = self.builder.ins().iconst(types::I64, to.high);
                let raised = self.builder.ins().smax(value, low);
                (self.builder.ins().smin(raised, high), None)
            }
            IntPolicy::Wrap => {
                if from.low >= to.low && from.high <= to.high {
                    return (value, None);
                }
                let bits = int_bits(to.width);
                if bits == 64 {
                    return (value, None);
                }
                if signed_width(to.width) {
                    let shift = i64::from(64 - bits);
                    let raised = self.builder.ins().ishl_imm_u(value, shift);
                    (self.builder.ins().sshr_imm_u(raised, shift), None)
                } else {
                    let mask = self.builder.ins().iconst(types::I64, to.high);
                    (self.builder.ins().band(value, mask), None)
                }
            }
        }
    }

    /// `x ↦ int via <rounding>`: round, then check the result against the
    /// target's range; NaN and infinities fail.
    fn float_to_int(
        &mut self,
        conversion: &MirConversion,
        to: IntKind,
        value: Value,
    ) -> (Value, Option<(Value, Failure)>) {
        let mode = self.rounding_mode(&conversion.params);
        let rounded = match mode {
            Rounding::Floor => self.builder.ins().floor(value),
            Rounding::Ceil => self.builder.ins().ceil(value),
            Rounding::HalfEven => self.builder.ins().nearest(value),
            Rounding::Trunc => self.builder.ins().trunc(value),
        };
        // The bounds are exact in f64: powers of two and small integers.
        let low = self.builder.ins().f64const(to.low as f64);
        let above = self.builder.ins().f64const((to.high as f64) + 1.0);
        let at_least = self
            .builder
            .ins()
            .fcmp(FloatCC::GreaterThanOrEqual, rounded, low);
        let below = self.builder.ins().fcmp(FloatCC::LessThan, rounded, above);
        let fits = self.builder.ins().band(at_least, below);
        let one = self.builder.ins().iconst(types::I8, 1);
        let failed = self.builder.ins().bxor(fits, one);
        let converted = self.builder.ins().fcvt_to_sint_sat(types::I64, rounded);
        (
            converted,
            Some((
                failed,
                Failure::Static("fractus to numerus conversion out of range"),
            )),
        )
    }

    fn rounding_mode(&self, params: &[radix_lexer::Symbol]) -> Rounding {
        self.rounding_mode_or(params, Rounding::Trunc)
    }

    /// A decimal or text converting in: the handle, or a failure by status.
    fn decimal_in(
        &mut self,
        symbol: &'static abi::RuntimeSymbol,
        value: Value,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        let decimal = self
            .call_runtime(symbol, &[value])?
            .ok_or_else(|| Decline::new("decimal result"))?;
        let status = self.parse_status()?;
        let zero = self.builder.ins().iconst(types::I32, 0);
        let failed = self.builder.ins().icmp(IntCC::NotEqual, status, zero);
        Ok((decimal, Some((failed, Failure::Code(status)))))
    }

    fn rounding_mode_or(&self, params: &[radix_lexer::Symbol], default: Rounding) -> Rounding {
        params
            .iter()
            .find_map(|param| match self.state.input.interner.resolve(*param) {
                "floor" | "Floor" => Some(Rounding::Floor),
                "ceil" | "Ceil" => Some(Rounding::Ceil),
                "half_even" | "HalfEven" => Some(Rounding::HalfEven),
                "trunc" | "Trunc" => Some(Rounding::Trunc),
                _ => None,
            })
            .unwrap_or(default)
    }

    /// `textus ↦ int`: a parse by the `hex`/`bin`/`oct` radix hint (decimal
    /// otherwise), range-checked against the target.
    fn text_to_int(
        &mut self,
        conversion: &MirConversion,
        to: IntKind,
        value: Value,
    ) -> Lowered<(Value, Option<(Value, Failure)>)> {
        let radix = conversion
            .params
            .iter()
            .find_map(|param| match self.state.input.interner.resolve(*param) {
                "hex" | "Hex" => Some(16),
                "bin" | "Bin" => Some(2),
                "oct" | "Oct" => Some(8),
                _ => None,
            })
            .unwrap_or(10);
        let radix = self.builder.ins().iconst(types::I32, radix);
        let low = self.builder.ins().iconst(types::I64, to.low);
        let high = self.builder.ins().iconst(types::I64, to.high);
        let parsed = self
            .call_runtime(&abi::PARSE_INT, &[value, radix, low, high])?
            .ok_or_else(|| Decline::new("parse result"))?;
        let status = self.parse_status()?;
        let zero = self.builder.ins().iconst(types::I32, 0);
        let failed = self.builder.ins().icmp(IntCC::NotEqual, status, zero);
        Ok((parsed, Some((failed, Failure::Code(status)))))
    }

    /// Raise a conversion failure when `failed` is set; continue otherwise.
    fn raise(&mut self, failed: Value, failure: Failure) -> Lowered<()> {
        let raise = self.builder.create_block();
        self.builder.set_cold_block(raise);
        let next = self.builder.create_block();
        self.builder.ins().brif(failed, raise, &[], next, &[]);
        self.builder.switch_to_block(raise);
        let message = match failure {
            Failure::Static(text) => self.text_literal(text.as_bytes())?,
            Failure::Message(text) => text,
            Failure::Code(code) => self
                .call_runtime(&abi::CONVERT_MESSAGE, &[code])?
                .ok_or_else(|| Decline::new("conversion message result"))?,
        };
        match self.error_rep {
            Some(Rep::Text) => {
                let mut results = vec![self.builder.ins().iconst(types::I8, 1)];
                let ret_rep = self.ret_rep;
                if let Some(zero) = self.zero(ret_rep) {
                    results.push(zero);
                }
                results.push(message);
                self.builder.ins().return_(&results);
            }
            None => {
                let length = self
                    .call_runtime(&abi::TEXT_LEN, &[message])?
                    .ok_or_else(|| Decline::new("text length result"))?;
                let bytes = self.builder.ins().iadd_imm_s(
                    message,
                    i64::try_from(abi::TEXT_HEADER)
                        .map_err(|_| Decline::new("text header offset"))?,
                );
                let reference = self.runtime_ref(&abi::TRAP_CONVERSION)?;
                self.builder.ins().call(reference, &[bytes, length]);
                self.builder.ins().trap(TrapCode::unwrap_user(1));
            }
            Some(_) => return Err(Decline::new("conversion failure channel type")),
        }
        self.builder.switch_to_block(next);
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Rounding {
    Floor,
    Ceil,
    HalfEven,
    Trunc,
}

/// What a union member test compares at run time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MemberTest {
    /// The payload tag alone (widths and element types are erased, as the
    /// runner erases them).
    Tag(i64),
    /// A `genus` instance of this definition.
    Struct(radix_hir::DefId),
    /// The absence of a value (an optional's none).
    Nil,
}

/// The run-time tests for `x est <target>` over a boxed union, any of which
/// may match (a union target tests each member); `None` when a target is not
/// told apart by tag or definition.
pub(crate) fn member_tests(table: &TypeTable, target: TypeId) -> Option<Vec<MemberTest>> {
    Some(vec![match resolve(table, target) {
        Type::Union(members) => {
            let mut tests = Vec::new();
            for member in members {
                tests.extend(member_tests(table, *member)?);
            }
            return Some(tests);
        }
        // An unbounded integer is an integer inside `i64` (the plain tag) or
        // one beyond it (the inf tag): `est inf` holds for every integer.
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::Inf) => {
            return Some(vec![
                MemberTest::Tag(abi::TAG_INT),
                MemberTest::Tag(abi::TAG_INF),
            ]);
        }
        // A decimal has its own payload and no open-data box.
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::D64) => {
            return None;
        }
        Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _) => {
            MemberTest::Tag(abi::TAG_INT)
        }
        Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {
            MemberTest::Tag(abi::TAG_FLOAT)
        }
        Type::Primitive(Primitive::Bivalens) => MemberTest::Tag(abi::TAG_BOOL),
        Type::Primitive(Primitive::Textus | Primitive::Ascii) => MemberTest::Tag(abi::TAG_TEXT),
        Type::Array(_) | Type::BoundedArray { .. } => MemberTest::Tag(abi::TAG_LIST),
        Type::Struct(def) => MemberTest::Struct(*def),
        Type::Primitive(Primitive::Nihil) => MemberTest::Nil,
        _ => return None,
    }])
}

impl FunctionLower<'_, '_> {
    /// `value ¶ "spec"`: the operand's rep picks the renderer, the spec
    /// travels as a text and is parsed by the runtime.
    pub(super) fn format_value(
        &mut self,
        spec: radix_lexer::Symbol,
        operand: &MirOperand,
    ) -> Lowered<Value> {
        let narrow = self.float_narrow(operand);
        let (value, rep) = self.scalar_wide(operand)?;
        let text = self.state.input.interner.resolve(spec).to_owned();
        let spec = self.text_literal(text.as_bytes())?;
        let rendered = match rep {
            Rep::Int => {
                let wide = self.widen(value);
                let (low, high) = self.split_wide(wide);
                self.call_runtime(&abi::FORMAT_INT, &[low, high, spec])?
            }
            Rep::Float => {
                let narrow = self.builder.ins().iconst(types::I32, narrow);
                self.call_runtime(&abi::FORMAT_FLOAT, &[value, narrow, spec])?
            }
            Rep::Text => self.call_runtime(&abi::FORMAT_TEXT, &[value, spec])?,
            Rep::Dec => self.call_runtime(&abi::FORMAT_DEC, &[value, spec])?,
            Rep::Inf => self.call_runtime(&abi::FORMAT_INF, &[value, spec])?,
            Rep::Ins => self.call_runtime(&abi::FORMAT_INS, &[value, spec])?,
            _ => return Err(Decline::new("format of this value")),
        };
        rendered.ok_or_else(|| Decline::new("format result"))
    }

    /// `x est T` / `x non est T` over a boxed union payload.
    pub(super) fn type_check(
        &mut self,
        op: &MirTypeCheckOp,
        operand: &MirOperand,
    ) -> Lowered<Value> {
        let (handle, rep) = self.scalar(operand)?;
        let tests = member_tests(self.state.input.types, op.ty.semantic_id())
            .ok_or_else(|| Decline::new("type check target"))?;
        if !matches!(rep, Rep::Opt | Rep::Record) {
            // A monomorphized generic can leave a plain scalar operand: its
            // one type is known, so the test is decided here.
            let own = member_tests(
                self.state.input.types,
                self.operand_type(operand)?.semantic_id(),
            )
            .ok_or_else(|| Decline::new("type check of a non-union"))?;
            let holds = own.iter().all(|test| tests.contains(test)) == op.positive;
            return Ok(self.builder.ins().iconst(types::I8, i64::from(holds)));
        }
        let mut found: Option<Value> = None;
        for test in tests {
            let hit = match test {
                // A genus or interface value is never none and has no tag.
                MemberTest::Nil | MemberTest::Tag(_) if rep == Rep::Record => {
                    Some(self.builder.ins().iconst(types::I8, 0))
                }
                MemberTest::Struct(def) if rep == Rep::Record => {
                    let name = self
                        .state
                        .input
                        .structs
                        .names
                        .get(&def)
                        .ok_or_else(|| Decline::new("type check of an unnamed definition"))?
                        .clone();
                    let name = self.text_literal(name.as_bytes())?;
                    let is_variant = self
                        .state
                        .input
                        .structs
                        .enum_variants
                        .values()
                        .any(|variants| variants.contains(&def));
                    let variant = self.builder.ins().iconst(types::I32, i64::from(is_variant));
                    self.call_runtime(&abi::RECORD_IS_NAMED, &[handle, name, variant])?
                }
                MemberTest::Nil => {
                    let zero = self.builder.ins().iconst(types::I64, 0);
                    Some(self.builder.ins().icmp(IntCC::Equal, handle, zero))
                }
                MemberTest::Tag(tag) => {
                    let tag = self.builder.ins().iconst(types::I32, tag);
                    self.call_runtime(&abi::UNION_HAS_TAG, &[handle, tag])?
                }
                MemberTest::Struct(def) => {
                    let name = self
                        .state
                        .input
                        .structs
                        .names
                        .get(&def)
                        .ok_or_else(|| Decline::new("type check of an unnamed definition"))?
                        .clone();
                    let name = self.text_literal(name.as_bytes())?;
                    self.call_runtime(&abi::UNION_IS_STRUCT, &[handle, name])?
                }
            }
            .ok_or_else(|| Decline::new("type check result"))?;
            found = Some(match found {
                Some(seen) => self.builder.ins().bor(seen, hit),
                None => hit,
            });
        }
        let found = found.ok_or_else(|| Decline::new("type check target"))?;
        if op.positive {
            Ok(found)
        } else {
            let one = self.builder.ins().iconst(types::I8, 1);
            Ok(self.builder.ins().bxor(found, one))
        }
    }
}

#[cfg(test)]
#[path = "convert_test.rs"]
mod convert_test;
