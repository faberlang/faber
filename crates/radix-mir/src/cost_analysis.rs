//! Static runtime-cost classification over MIR conversions.
//!
//! This module owns the v1 cost-class vocabulary, the revisable cost-basis
//! table, and the pure conversion classifier. It does not walk `ValidatedMir`,
//! apply loop tags, or emit a report — those belong to the site walker and
//! `faber cost`. Size amplification is a walker-applied hook over the
//! intrinsic class this classifier returns.

use crate::{MirConversion, MirConversionFlavor, MirType};
use radix_types::{Primitive, Type, TypeTable};

/// Logarithmic cost class. `C0 = 1`, `C1 = 10`, `C2 = 100`, `C3 = 1000`.
///
/// Higher classes exist only through size amplification, which is class
/// **addition** (`+ round(log10(element_count))`), never multiplication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CostClass(u8);

impl CostClass {
    pub const C0: Self = Self(0);
    pub const C1: Self = Self(1);
    pub const C2: Self = Self(2);
    pub const C3: Self = Self(3);

    /// Construct a class from its log10 exponent (`0` → `C0`).
    #[must_use]
    pub const fn from_log10(log10: u8) -> Self {
        Self(log10)
    }

    /// Log10 exponent of this class.
    #[must_use]
    pub const fn log10(self) -> u8 {
        self.0
    }

    /// Relative units (`10^log10`). Saturates at `u64::MAX`.
    #[must_use]
    pub fn units(self) -> u64 {
        10u64.saturating_pow(u32::from(self.0))
    }

    /// Walker-applied size amplification: add `round(log10(element_count))`.
    #[must_use]
    pub fn amplify(self, element_count: u64) -> Self {
        Self(self.0.saturating_add(round_log10(element_count)))
    }
}

/// Site tag: foldable, uncosted, or a C-class. Every site gets exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CostTag {
    /// Constant-operand conversio — foldable, costed zero, reported separately.
    F,
    /// Package/runtime call or conversion without intrinsic cost data.
    U,
    Class(CostClass),
}

impl CostTag {
    /// Walker-applied size amplification. Side buckets (`F`, `U`) stay put.
    #[must_use]
    pub fn amplify(self, element_count: u64) -> Self {
        match self {
            Self::Class(class) => Self::Class(class.amplify(element_count)),
            Self::F => self,
            Self::U => self,
        }
    }
}

/// Conversion family keyed by the cost-basis table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CostFamily {
    Cast,
    OctetiWindowUnpack,
    TextusToNumerus,
    NumerusToTextus,
    GenusRecordExtraction,
    ListTensor,
    TextusToRegex,
    ValorJson,
    ConstantOperand,
    UnknownExtern,
}

impl CostFamily {
    /// v1 families in table order.
    pub const ALL: [Self; 10] = [
        Self::Cast,
        Self::OctetiWindowUnpack,
        Self::TextusToNumerus,
        Self::NumerusToTextus,
        Self::GenusRecordExtraction,
        Self::ListTensor,
        Self::TextusToRegex,
        Self::ValorJson,
        Self::ConstantOperand,
        Self::UnknownExtern,
    ];
}

/// One revisable cost-basis entry. Pass logic looks up this table; it does not
/// hard-code class numbers beside family predicates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CostBasisRow {
    pub family: CostFamily,
    pub tag: CostTag,
}

/// Initial cost basis (operator-revisitable). One table, not scattered arms.
pub const COST_BASIS: &[CostBasisRow] = &[
    // Register op: `∷` / width / int↔float scalar. No allocation, no scan.
    CostBasisRow {
        family: CostFamily::Cast,
        tag: CostTag::Class(CostClass::C0),
    },
    // Fixed 2/4/8-byte `octeti` window unpack into `numerus<W, Be/Le>`.
    CostBasisRow {
        family: CostFamily::OctetiWindowUnpack,
        tag: CostTag::Class(CostClass::C0),
    },
    // Bounded text parse (`textus ↦ numerus`, including Hex/Bin/Oct).
    CostBasisRow {
        family: CostFamily::TextusToNumerus,
        tag: CostTag::Class(CostClass::C1),
    },
    // Bounded text format (`numerus ↦ textus`).
    CostBasisRow {
        family: CostFamily::NumerusToTextus,
        tag: CostTag::Class(CostClass::C1),
    },
    // Shallow per-field copy (`struct_defaults` / valor → genus).
    CostBasisRow {
        family: CostFamily::GenusRecordExtraction,
        tag: CostTag::Class(CostClass::C1),
    },
    // Allocate + elementwise copy. Walker adds `+log10(N)` when shape is static.
    CostBasisRow {
        family: CostFamily::ListTensor,
        tag: CostTag::Class(CostClass::C2),
    },
    // Allocation-heavy compile of a bounded pattern (`textus ↦ regex`).
    CostBasisRow {
        family: CostFamily::TextusToRegex,
        tag: CostTag::Class(CostClass::C2),
    },
    // Full-tree walk + transform (`valor ↦ json` / `json ↦` genus).
    CostBasisRow {
        family: CostFamily::ValorJson,
        tag: CostTag::Class(CostClass::C3),
    },
    // Operand traceable to a constant / literal-fed local. Walker-applied.
    CostBasisRow {
        family: CostFamily::ConstantOperand,
        tag: CostTag::F,
    },
    // Extern dispatch without intrinsic cost data. Never silently free.
    CostBasisRow {
        family: CostFamily::UnknownExtern,
        tag: CostTag::U,
    },
];

/// Look up the tag for a family. Missing rows fail closed to `U`.
#[must_use]
pub fn basis_tag(family: CostFamily) -> CostTag {
    COST_BASIS
        .iter()
        .find(|row| row.family == family)
        .map_or(CostTag::U, |row| row.tag)
}

/// Intrinsic tag of a conversion from flavor + type pair.
///
/// Does not trace operands (`F` is [`foldable_tag`]) and does not apply size
/// amplification ([`amplify_class`] / [`CostClass::amplify`]).
#[must_use]
pub fn classify_conversion(conversion: &MirConversion, types: &TypeTable) -> CostTag {
    basis_tag(conversion_family(conversion, types))
}

/// Family of a conversion from `flavor`, `source_ty`, and `target_ty`.
///
/// Never returns [`CostFamily::ConstantOperand`] — that row is operand-trace
/// and belongs to the walker via [`foldable_tag`].
#[must_use]
pub fn conversion_family(conversion: &MirConversion, types: &TypeTable) -> CostFamily {
    if conversion.flavor == MirConversionFlavor::Cast {
        return CostFamily::Cast;
    }
    let source = type_kind(types, conversion.source_ty);
    let target = type_kind(types, conversion.target_ty);
    if is_octeti(source) && is_numerus(target) {
        return CostFamily::OctetiWindowUnpack;
    }
    if is_textus(source) && is_numerus(target) {
        return CostFamily::TextusToNumerus;
    }
    if is_numerus(source) && is_textus(target) {
        return CostFamily::NumerusToTextus;
    }
    if is_valor(source) && is_struct(target) {
        return CostFamily::GenusRecordExtraction;
    }
    if is_list_tensor_pair(source, target) {
        return CostFamily::ListTensor;
    }
    if is_textus(source) && is_regex(target) {
        return CostFamily::TextusToRegex;
    }
    if is_valor_json_pair(source, target) {
        return CostFamily::ValorJson;
    }
    if is_numeric_scalar(source) && is_numeric_scalar(target) {
        return CostFamily::Cast;
    }
    CostFamily::UnknownExtern
}

/// `F` tag from the [`CostFamily::ConstantOperand`] basis row. Walker applies
/// this when the operand traces to `MirOperand::Constant` / a literal-fed local.
#[must_use]
pub fn foldable_tag() -> CostTag {
    basis_tag(CostFamily::ConstantOperand)
}

/// `U` tag from the [`CostFamily::UnknownExtern`] basis row. Walker applies
/// this for package or runtime calls without intrinsic cost data.
#[must_use]
pub fn uncosted_tag() -> CostTag {
    basis_tag(CostFamily::UnknownExtern)
}

/// Walker-applied size amplification: `intrinsic + round(log10(element_count))`.
#[must_use]
pub fn amplify_class(intrinsic: CostClass, element_count: u64) -> CostClass {
    intrinsic.amplify(element_count)
}

fn type_kind(types: &TypeTable, ty: MirType) -> &Type {
    let mut id = ty.semantic_id();
    loop {
        match types.get(id) {
            Type::Alias(_, inner) => id = *inner,
            resolved => return resolved,
        }
    }
}

fn is_textus(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Primitive(Primitive::Textus) | Type::BoundedTextus { .. }
    )
}

fn is_numerus(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _)
    )
}

fn is_octeti(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Primitive(Primitive::Octeti) | Type::BoundedOcteti { .. }
    )
}

fn is_regex(ty: &Type) -> bool {
    matches!(ty, Type::Primitive(Primitive::Regex))
}

fn is_valor(ty: &Type) -> bool {
    matches!(ty, Type::Primitive(Primitive::Valor))
}

fn is_json(ty: &Type) -> bool {
    matches!(ty, Type::Primitive(Primitive::Json))
}

fn is_struct(ty: &Type) -> bool {
    matches!(ty, Type::Struct(_))
}

fn is_list(ty: &Type) -> bool {
    matches!(ty, Type::Array(_) | Type::BoundedArray { .. })
}

fn is_tensor(ty: &Type) -> bool {
    matches!(ty, Type::Tensor(_, _))
}

fn is_numeric_scalar(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Primitive(Primitive::Numerus | Primitive::Fractus)
            | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
    )
}

fn is_list_tensor_pair(source: &Type, target: &Type) -> bool {
    (is_list(source) && is_tensor(target)) || (is_tensor(source) && is_list(target))
}

fn is_valor_json_pair(source: &Type, target: &Type) -> bool {
    (is_valor(source) && is_json(target)) || (is_json(source) && is_struct(target))
}

/// `round(log10(n))` as a class bump. Counts `0` and `1` add nothing.
fn round_log10(element_count: u64) -> u8 {
    if element_count <= 1 {
        return 0;
    }
    let floor = u8::try_from(element_count.ilog10()).unwrap_or(u8::MAX);
    let pow = u128::from(10u64.saturating_pow(u32::from(floor)));
    let n = u128::from(element_count);
    let rounds_up = n.saturating_mul(n) >= pow.saturating_mul(pow).saturating_mul(10);
    floor.saturating_add(u8::from(rounds_up))
}

#[cfg(test)]
#[path = "cost_analysis_test.rs"]
mod tests;
