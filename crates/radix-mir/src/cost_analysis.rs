//! Static runtime-cost classification over MIR conversions.
//!
//! This module owns the v1 cost-class vocabulary, the revisable cost-basis
//! table, the pure conversion classifier, and the site walker over
//! [`ValidatedMir`]. Size amplification, loop membership, and F/U buckets
//! are walker-applied. Per-function roll-up *rendering* belongs to `faber
//! cost`; [`cost_rollups`] produces the data.

use crate::figura::i64s_from_literal_index;
use crate::read_analysis::{
    operand_local, statement_destination, statement_destination_place, temp_forward_local,
};
use crate::visit::terminator_successors;
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirBlockId, MirCallee, MirConversion,
    MirConversionFlavor, MirFunction, MirFunctionId, MirIntrinsic, MirLocalId, MirOperand,
    MirPlace, MirPlaceBase, MirRuntimeCall, MirStatement, MirStatementKind, MirTempId,
    MirTerminatorKind, MirType, MirValue, MirValueId, MirValueKind, ValidatedMir,
};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::DefId;
use radix_lexer::{Span, Symbol};
use radix_types::{Primitive, Type, TypeTable};
use std::collections::BTreeMap;

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
            Self::F | Self::U => self,
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
    RegisterWindow,
    RegisterBridge,
    RegisterLista,
    TextusToRegex,
    ValorJson,
    ConstantOperand,
    UnknownExtern,
}

impl CostFamily {
    /// v1 families in table order.
    pub const ALL: [Self; 13] = [
        Self::Cast,
        Self::OctetiWindowUnpack,
        Self::TextusToNumerus,
        Self::NumerusToTextus,
        Self::GenusRecordExtraction,
        Self::ListTensor,
        Self::RegisterWindow,
        Self::RegisterBridge,
        Self::RegisterLista,
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
    // Fixed 2/4/8-byte `octeti` window unpack into `W via Be/Le`.
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
    // Register window: `octeti<N> ↦ vector<T, N/width>` in, `vector<T, size N> ↦ octeti<N>` out.
    CostBasisRow {
        family: CostFamily::RegisterWindow,
        tag: CostTag::Class(CostClass::C0),
    },
    // Register op: `vector<T, size N> ↦ vector<U, size N>` element bridge. No allocation.
    CostBasisRow {
        family: CostFamily::RegisterBridge,
        tag: CostTag::Class(CostClass::C0),
    },
    // Allocate + elementwise copy (`vector<T, size N> ↦ lista<T>`, `lista<T, magnitudo N> ↦ vector<T, size N>`).
    CostBasisRow {
        family: CostFamily::RegisterLista,
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
    if matches!(
        conversion.flavor,
        MirConversionFlavor::Cast | MirConversionFlavor::Proven
    ) {
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
    if (is_octeti(source) && is_vector(target)) || (is_vector(source) && is_octeti(target)) {
        return CostFamily::RegisterWindow;
    }
    if is_vector(source) && is_vector(target) {
        return CostFamily::RegisterBridge;
    }
    if (is_vector(source) && is_list(target)) || (is_list(source) && is_vector(target)) {
        return CostFamily::RegisterLista;
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
    matches!(
        ty,
        Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. }
    )
}

fn is_tensor(ty: &Type) -> bool {
    matches!(ty, Type::Tensor(_, _))
}

fn is_vector(ty: &Type) -> bool {
    matches!(ty, Type::Vector(_, _))
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

/// Origin of a cost site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CostSiteKind {
    /// `MirIntrinsic::Convert` runtime call.
    Convert,
    /// Package `Call`/`TryCall` to a `Definition` callee, or a `Provider`
    /// runtime call — no intrinsic cost data.
    ExternCall,
}

/// One cost site in a [`ValidatedMir`] program.
///
/// **Span decider (U2):** `Span` is a byte range with no file id. MIR does
/// not carry a multi-file def→path map; diagnostics attach `file` separately
/// via `Diagnostic::with_source_context`. v1 records `function` + optional
/// `source` [`DefId`] + the statement/terminator span. `faber cost` renders the
/// path through that existing diagnostics context, not a new package-relative
/// span type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CostSite {
    pub function: MirFunctionId,
    pub source: Option<DefId>,
    pub name: Option<Symbol>,
    pub block: MirBlockId,
    pub span: Span,
    pub kind: CostSiteKind,
    pub family: CostFamily,
    pub tag: CostTag,
    pub hot: bool,
    pub element_count: Option<u64>,
}

/// Per-function roll-up: dominant class plus per-class histogram.
///
/// Rendering (human/JSON) is U3's concern; this is the data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostFunctionRollup {
    pub function: MirFunctionId,
    pub dominant: Option<CostClass>,
    pub class_histogram: Vec<(CostClass, usize)>,
    pub foldable: usize,
    pub uncosted: usize,
    pub hot: usize,
}

/// Enumerate conversion sites and uncosted extern calls over `validated`.
///
/// Convert sites get an intrinsic class from [`classify_conversion`], then
/// F (constant/literal-fed operand), size amplification, and a loop-member
/// hot tag. Package `Definition` calls and `Provider` runtime calls are U.
#[must_use]
pub fn cost_sites(validated: &ValidatedMir<'_>) -> Vec<CostSite> {
    let types = validated.validation().types.as_table();
    let mut sites = Vec::new();
    for function in &validated.program().functions {
        let hot_blocks = loop_member_blocks(function);
        let foldable = foldable_bindings(function);
        for block in &function.blocks {
            let hot = hot_blocks.contains(&block.id);
            let loc = SiteLoc {
                function,
                block: block.id,
                hot,
            };
            for statement in &block.statements {
                if let Some(site) = site_from_statement(loc, statement, &foldable, types) {
                    sites.push(site);
                }
            }
            if let Some(site) =
                site_from_terminator(loc, &block.terminator.kind, block.terminator.span)
            {
                sites.push(site);
            }
        }
    }
    sites
}

/// Group [`cost_sites`] by function into dominant class + histogram.
#[must_use]
pub fn cost_rollups(sites: &[CostSite]) -> Vec<CostFunctionRollup> {
    let mut order = Vec::new();
    let mut grouped: BTreeMap<u32, Vec<&CostSite>> = BTreeMap::new();
    for site in sites {
        grouped
            .entry(site.function.0)
            .or_insert_with(|| {
                order.push(site.function);
                Vec::new()
            })
            .push(site);
    }
    order
        .into_iter()
        .map(|function| {
            rollup_function(
                function,
                grouped.get(&function.0).map_or(&[], Vec::as_slice),
            )
        })
        .collect()
}

fn rollup_function(function: MirFunctionId, sites: &[&CostSite]) -> CostFunctionRollup {
    let mut counts: BTreeMap<u8, usize> = BTreeMap::new();
    let mut foldable = 0;
    let mut uncosted = 0;
    let mut hot = 0;
    let mut dominant = None;
    for site in sites {
        hot += usize::from(site.hot);
        match site.tag {
            CostTag::F => foldable += 1,
            CostTag::U => uncosted += 1,
            CostTag::Class(class) => {
                *counts.entry(class.log10()).or_insert(0) += 1;
                dominant = Some(dominant.map_or(class, |best: CostClass| best.max(class)));
            }
        }
    }
    CostFunctionRollup {
        function,
        dominant,
        class_histogram: counts
            .into_iter()
            .map(|(log10, count)| (CostClass::from_log10(log10), count))
            .collect(),
        foldable,
        uncosted,
        hot,
    }
}

#[derive(Clone, Copy)]
struct SiteLoc<'a> {
    function: &'a MirFunction,
    block: MirBlockId,
    hot: bool,
}

fn site_from_statement(
    loc: SiteLoc<'_>,
    statement: &MirStatement,
    foldable: &Foldable,
    types: &TypeTable,
) -> Option<CostSite> {
    match &statement.kind {
        MirStatementKind::RuntimeCall { call, .. } => match &call.intrinsic {
            MirIntrinsic::Convert(conversion) => Some(convert_site(
                loc,
                statement.span,
                call,
                conversion,
                foldable,
                types,
            )),
            MirIntrinsic::Provider(_) => Some(extern_site(loc, statement.span)),
            _ => None,
        },
        MirStatementKind::Call { callee, .. } if is_extern_callee(callee) => {
            Some(extern_site(loc, statement.span))
        }
        MirStatementKind::Assign { .. }
        | MirStatementKind::Construct { .. }
        | MirStatementKind::Call { .. } => None,
    }
}

fn site_from_terminator(
    loc: SiteLoc<'_>,
    terminator: &MirTerminatorKind,
    span: Span,
) -> Option<CostSite> {
    let MirTerminatorKind::TryCall { callee, .. } = terminator else {
        return None;
    };
    is_extern_callee(callee).then(|| extern_site(loc, span))
}

fn convert_site(
    loc: SiteLoc<'_>,
    span: Span,
    call: &MirRuntimeCall,
    conversion: &MirConversion,
    foldable: &Foldable,
    types: &TypeTable,
) -> CostSite {
    let family = conversion_family(conversion, types);
    let element_count = conversion_element_count(conversion, types);
    let source_foldable = call
        .args
        .first()
        .is_some_and(|operand| operand_is_foldable(operand, loc.function, foldable));
    let tag = if source_foldable {
        foldable_tag()
    } else {
        let intrinsic = classify_conversion(conversion, types);
        element_count.map_or(intrinsic, |count| intrinsic.amplify(count))
    };
    CostSite {
        function: loc.function.id,
        source: loc.function.source,
        name: loc.function.name,
        block: loc.block,
        span,
        kind: CostSiteKind::Convert,
        family,
        tag,
        hot: loc.hot,
        element_count,
    }
}

fn extern_site(loc: SiteLoc<'_>, span: Span) -> CostSite {
    CostSite {
        function: loc.function.id,
        source: loc.function.source,
        name: loc.function.name,
        block: loc.block,
        span,
        kind: CostSiteKind::ExternCall,
        family: CostFamily::UnknownExtern,
        tag: uncosted_tag(),
        hot: loc.hot,
        element_count: None,
    }
}

fn is_extern_callee(callee: &MirCallee) -> bool {
    matches!(callee, MirCallee::Definition { .. })
}

fn conversion_element_count(conversion: &MirConversion, types: &TypeTable) -> Option<u64> {
    static_element_count(conversion.source_ty, types)
        .or_else(|| static_element_count(conversion.target_ty, types))
}

/// Literal tensor/list element count — same index facts [`crate::static_shape_fold`]
/// reads via [`i64s_from_literal_index`].
fn static_element_count(ty: MirType, types: &TypeTable) -> Option<u64> {
    let shape = match type_kind(types, ty) {
        Type::Tensor(_, shape) => *shape,
        Type::BoundedArray { capacity, .. } => *capacity,
        _ => return None,
    };
    let dims = i64s_from_literal_index(shape, types)?;
    let mut product = 1u64;
    for dim in dims {
        product = product.saturating_mul(u64::try_from(dim).ok()?);
    }
    Some(product)
}

struct Foldable {
    locals: FxHashSet<MirLocalId>,
    temps: FxHashSet<MirTempId>,
    values: FxHashSet<MirValueId>,
}

fn foldable_bindings(function: &MirFunction) -> Foldable {
    let params: FxHashSet<MirLocalId> = function.params.iter().map(|param| param.local).collect();
    let mut foldable = Foldable {
        locals: FxHashSet::default(),
        temps: FxHashSet::default(),
        values: FxHashSet::default(),
    };
    let mut tainted_locals = params.clone();
    let mut tainted_temps = FxHashSet::default();
    let mut changed = true;
    while changed {
        changed = false;
        for block in &function.blocks {
            for statement in &block.statements {
                changed |= absorb_statement(
                    function,
                    statement,
                    &params,
                    &mut foldable,
                    &mut tainted_locals,
                    &mut tainted_temps,
                );
            }
        }
    }
    foldable
}

fn absorb_statement(
    function: &MirFunction,
    statement: &MirStatement,
    params: &FxHashSet<MirLocalId>,
    foldable: &mut Foldable,
    tainted_locals: &mut FxHashSet<MirLocalId>,
    tainted_temps: &mut FxHashSet<MirTempId>,
) -> bool {
    let src_foldable = statement_source_foldable(function, statement, foldable);
    let mut changed = false;
    if let MirStatementKind::Assign { value, .. } = &statement.kind
        && src_foldable
    {
        changed |= foldable.values.insert(value.id);
    }
    if let Some(local) = statement_destination(statement) {
        changed |= consider_local(local, src_foldable, params, foldable, tainted_locals);
        return changed;
    }
    let Some(place) = statement_destination_place(statement) else {
        return changed;
    };
    changed |= consider_temp_place(place, src_foldable, foldable, tainted_temps);
    changed
}

fn statement_source_foldable(
    function: &MirFunction,
    statement: &MirStatement,
    foldable: &Foldable,
) -> bool {
    match &statement.kind {
        MirStatementKind::Assign { value, .. } => value_is_foldable(value, function, foldable),
        MirStatementKind::Construct { aggregate, .. } => {
            aggregate_operands_foldable(aggregate, function, foldable)
        }
        MirStatementKind::RuntimeCall { call, .. } => match &call.intrinsic {
            MirIntrinsic::Convert(_) => call
                .args
                .first()
                .is_some_and(|operand| operand_is_foldable(operand, function, foldable)),
            _ => false,
        },
        MirStatementKind::Call { .. } => false,
    }
}

fn consider_local(
    id: MirLocalId,
    src_foldable: bool,
    params: &FxHashSet<MirLocalId>,
    foldable: &mut Foldable,
    tainted_locals: &mut FxHashSet<MirLocalId>,
) -> bool {
    if params.contains(&id) || !src_foldable {
        tainted_locals.insert(id);
        return foldable.locals.remove(&id);
    }
    !tainted_locals.contains(&id) && foldable.locals.insert(id)
}

fn consider_temp_place(
    place: &MirPlace,
    src_foldable: bool,
    foldable: &mut Foldable,
    tainted_temps: &mut FxHashSet<MirTempId>,
) -> bool {
    if !place.projections.is_empty() {
        return false;
    }
    let MirPlaceBase::Temp(id) = place.base else {
        return false;
    };
    if src_foldable {
        return !tainted_temps.contains(&id) && foldable.temps.insert(id);
    }
    tainted_temps.insert(id);
    foldable.temps.remove(&id)
}

fn value_is_foldable(value: &MirValue, function: &MirFunction, foldable: &Foldable) -> bool {
    match &value.kind {
        MirValueKind::Operand(operand) => operand_is_foldable(operand, function, foldable),
        MirValueKind::Closure(_)
        | MirValueKind::Unary { .. }
        | MirValueKind::Binary { .. }
        | MirValueKind::TensorExtent { .. }
        | MirValueKind::Option(_) => false,
    }
}

fn aggregate_operands_foldable(
    aggregate: &MirAggregate,
    function: &MirFunction,
    foldable: &Foldable,
) -> bool {
    match &aggregate.fields {
        MirAggregateFields::Ordered(items) => items.iter().all(|item| {
            let (MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand)) = item;
            operand_is_foldable(operand, function, foldable)
        }),
        MirAggregateFields::Named(fields) => fields
            .iter()
            .all(|field| operand_is_foldable(&field.value, function, foldable)),
        MirAggregateFields::Keyed(fields) => fields.iter().all(|field| {
            operand_is_foldable(&field.key, function, foldable)
                && operand_is_foldable(&field.value, function, foldable)
        }),
    }
}

fn operand_is_foldable(operand: &MirOperand, function: &MirFunction, foldable: &Foldable) -> bool {
    match operand {
        MirOperand::Constant(_) => true,
        MirOperand::Place(_) => place_operand_foldable(operand, foldable),
        MirOperand::Temp(temp) => {
            foldable.temps.contains(temp)
                || temp_forward_local(function, *temp)
                    .is_some_and(|local| foldable.locals.contains(&local))
        }
        MirOperand::Value(value) => foldable.values.contains(value),
    }
}

fn place_operand_foldable(operand: &MirOperand, foldable: &Foldable) -> bool {
    if let Some(local) = operand_local(operand) {
        return foldable.locals.contains(&local);
    }
    let MirOperand::Place(place) = operand else {
        return false;
    };
    if !place.projections.is_empty() {
        return false;
    }
    matches!(place.base, MirPlaceBase::Temp(temp) if foldable.temps.contains(&temp))
}

fn loop_member_blocks(function: &MirFunction) -> FxHashSet<MirBlockId> {
    if function.blocks.is_empty() {
        return FxHashSet::default();
    }
    let mut successors = FxHashMap::default();
    let mut predecessors: FxHashMap<MirBlockId, Vec<MirBlockId>> = FxHashMap::default();
    for block in &function.blocks {
        let succs = terminator_successors(&block.terminator.kind);
        for &succ in &succs {
            predecessors.entry(succ).or_default().push(block.id);
        }
        successors.insert(block.id, succs);
    }
    let mut members = FxHashSet::default();
    for (tail, head) in dfs_back_edges(MirBlockId(0), &successors) {
        collect_natural_loop(tail, head, &predecessors, &mut members);
    }
    members
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DfsColor {
    White,
    Gray,
    Black,
}

fn dfs_back_edges(
    entry: MirBlockId,
    successors: &FxHashMap<MirBlockId, Vec<MirBlockId>>,
) -> Vec<(MirBlockId, MirBlockId)> {
    let mut color = FxHashMap::default();
    let mut back_edges = Vec::new();
    let mut stack = vec![(entry, 0usize)];
    color.insert(entry, DfsColor::Gray);
    while let Some((id, idx)) = stack.pop() {
        let succs = successors.get(&id).map_or(&[][..], Vec::as_slice);
        if idx < succs.len() {
            stack.push((id, idx + 1));
            let succ = succs[idx];
            match color.get(&succ).copied().unwrap_or(DfsColor::White) {
                DfsColor::Gray => back_edges.push((id, succ)),
                DfsColor::White => {
                    color.insert(succ, DfsColor::Gray);
                    stack.push((succ, 0));
                }
                DfsColor::Black => {}
            }
        } else {
            color.insert(id, DfsColor::Black);
        }
    }
    back_edges
}

fn collect_natural_loop(
    tail: MirBlockId,
    head: MirBlockId,
    predecessors: &FxHashMap<MirBlockId, Vec<MirBlockId>>,
    members: &mut FxHashSet<MirBlockId>,
) {
    members.insert(head);
    if tail == head {
        return;
    }
    let mut stack = vec![tail];
    while let Some(id) = stack.pop() {
        if !members.insert(id) {
            continue;
        }
        if let Some(preds) = predecessors.get(&id) {
            stack.extend(preds.iter().copied());
        }
    }
}

#[cfg(test)]
#[path = "cost_analysis_test.rs"]
mod tests;

#[cfg(test)]
#[path = "cost_walker_test.rs"]
mod walker_tests;
