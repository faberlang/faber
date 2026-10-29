//! `emit_object`: validated MIR to a native object file.

use radix_lexer::Interner;
use radix_mir::names::MirNames;
use radix_mir::{MirProgram, MirType, ValidatedMir};
use radix_types::vocabulary::{CapabilityGap, Lowerability};
use radix_types::{Type, TypeTable};

use crate::capability::{SLUG_CLIF_SUBSET, classify_clif_reachable};
use crate::lower::{JitProgram, ProgramInput, StructMeta, lower_program, lower_program_jit};

/// A relocatable native object for the host triple.
#[derive(Debug, Clone)]
pub struct ClifObject {
    /// Object file bytes (Mach-O or ELF, per host).
    pub bytes: Vec<u8>,
}

/// A decline or failure of the CLIF backend. `category` is the first gap's
/// slug, `shape` its shape text; `detail` joins every gap in storage order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClifError {
    pub message: String,
    pub category: String,
    pub shape: String,
    pub detail: String,
}

impl ClifError {
    fn from_gaps(gaps: &[CapabilityGap]) -> Self {
        let first = gaps.first().expect("rejected carries a gap");
        let mut shapes: Vec<&str> = Vec::new();
        for gap in gaps {
            if !shapes.contains(&gap.shape()) {
                shapes.push(gap.shape());
            }
        }
        let detail = shapes.join(", ");
        Self {
            message: format!("clif target declines: {}; gaps: {detail}", first.shape()),
            category: first.slug().to_owned(),
            shape: first.shape().to_owned(),
            detail,
        }
    }

    fn late_decline(shape: &str) -> Self {
        Self {
            message: format!("clif target declines: {shape}; gaps: {shape}"),
            category: SLUG_CLIF_SUBSET.to_owned(),
            shape: shape.to_owned(),
            detail: shape.to_owned(),
        }
    }
}

/// Everything the lowering reads that validation derives: the entry, the
/// output words and the `genus` metadata. Both routes build it the same way,
/// after the same capability pre-pass.
struct Prepared {
    entry: Option<radix_mir::MirFunctionId>,
    tokens: [String; 4],
    table: TypeTable,
    structs: StructMeta,
}

/// Classify `validated` against the CLIF subset and gather the lowering inputs.
fn prepare(validated: &ValidatedMir<'_>, interner: &Interner) -> Result<Prepared, ClifError> {
    let program = validated.program();
    let types = validated.validation().types.as_table();
    let entry = MirNames::new(program, types, interner)
        .explicit_entry_function()
        .map(|function| function.id);
    if let Lowerability::Rejected(gaps) = classify_clif_reachable(validated, entry) {
        return Err(ClifError::from_gaps(&gaps));
    }
    let tokens = match entry {
        Some(id) => display_tokens(validated, id),
        None => ["true", "false", "none", "tuple"].map(str::to_owned),
    };
    let validation = validated.validation();
    // Optional fields and parameters keep a plain declared type; the lowering
    // sees them as the optional they behave as, in a table extended for it.
    let mut table = types.clone();
    let mut structs = StructMeta {
        order: validation
            .struct_field_order
            .iter()
            .chain(validation.variant_field_order.iter())
            .map(|(def, order)| (*def, order.clone()))
            .collect(),
        fields: validation
            .struct_fields
            .iter()
            .chain(validation.variant_fields.iter())
            .map(|(def, fields)| (*def, fields.iter().map(|(n, t)| (*n, *t)).collect()))
            .collect(),
        names: validation
            .display_names
            .iter()
            .map(|(def, name)| (*def, name.clone()))
            .collect(),
        enum_variants: validation
            .enum_variants
            .iter()
            .map(|(def, variants)| (*def, variants.clone()))
            .collect(),
        optional_params: handroll::FxHashMap::default(),
        environments: validation
            .closure_environments
            .iter()
            .map(|(id, environment)| (*id, environment.clone()))
            .collect(),
    };
    for (def, names) in &validation.nullable_struct_fields {
        let Some(fields) = structs.fields.get_mut(def) else {
            continue;
        };
        for name in names {
            if let Some(field) = fields.get_mut(name) {
                *field = optional_of(&mut table, *field);
            }
        }
    }
    structs.optional_params = optional_params(program, &mut table);
    Ok(Prepared {
        entry,
        tokens,
        table,
        structs,
    })
}

/// Emit a native object for `validated`.
///
/// # Errors
///
/// Returns [`ClifError`] naming the first gap when the program is outside the
/// CLIF subset, or the failing shape when the lowering declines late.
pub fn emit_object(
    validated: &ValidatedMir<'_>,
    interner: &Interner,
) -> Result<ClifObject, ClifError> {
    let prepared = prepare(validated, interner)?;
    emit_program(
        validated.program(),
        &prepared.table,
        interner,
        prepared.entry,
        prepared.tokens,
        &prepared.structs,
    )
}

/// Compile `validated` into this process's memory.
///
/// The same capability pre-pass and the same lowering as [`emit_object`]; only
/// the module the functions are defined in differs. `runtime` is the explicit
/// table of the runtime's `extern "C"` entry points: every runtime symbol the
/// program imports must be listed.
///
/// # Errors
///
/// Returns the identical [`ClifError`] [`emit_object`] returns for a program
/// outside the CLIF subset, or the failing shape when the lowering declines.
pub fn emit_jit(
    validated: &ValidatedMir<'_>,
    interner: &Interner,
    runtime: &[(&str, *const u8)],
) -> Result<JitProgram, ClifError> {
    let prepared = prepare(validated, interner)?;
    let input = ProgramInput {
        structs: &prepared.structs,
        program: validated.program(),
        types: &prepared.table,
        interner,
        entry: prepared.entry,
        tokens: prepared.tokens.clone(),
    };
    lower_program_jit(&input, runtime).map_err(|decline| ClifError::late_decline(&decline.shape))
}

/// `ty` as an optional: unchanged when it already admits none.
fn optional_of(table: &mut TypeTable, ty: MirType) -> MirType {
    let mut inner = ty.semantic_id();
    while let Type::Alias(_, next) | Type::Ref(_, next) = table.get(inner) {
        inner = *next;
    }
    if admits_none(table, inner) {
        return ty;
    }
    MirType::semantic(table.option(inner))
}

fn admits_none(table: &TypeTable, ty: radix_types::TypeId) -> bool {
    match table.get(ty) {
        Type::Alias(_, inner) => admits_none(table, *inner),
        Type::Option(_)
        | Type::Primitive(radix_types::Primitive::Nihil | radix_types::Primitive::Ignotum) => true,
        Type::Union(members) => members.iter().any(|member| admits_none(table, *member)),
        _ => false,
    }
}

/// Parameters some call passes none to although their declared type does not
/// admit it: the `optional` modifier. An omitted argument is passed as none.
fn optional_params(
    program: &MirProgram,
    table: &mut TypeTable,
) -> handroll::FxHashMap<(radix_mir::MirFunctionId, usize), MirType> {
    use radix_mir::{
        MirCallee, MirConstant, MirOperand, MirPlaceBase, MirStatementKind, MirTerminatorKind,
    };
    let mut found = handroll::FxHashMap::default();
    for function in &program.functions {
        let locals: handroll::FxHashMap<_, _> = function
            .locals
            .iter()
            .map(|local| (local.id, local.ty))
            .collect();
        let temps: handroll::FxHashMap<_, _> = function
            .temps
            .iter()
            .map(|temp| (temp.id, temp.ty))
            .collect();
        let none_like = |table: &TypeTable, operand: &MirOperand| match operand {
            MirOperand::Constant(MirConstant::Nil) => true,
            MirOperand::Place(place) if place.projections.is_empty() => {
                let ty = match place.base {
                    MirPlaceBase::Local(id) => locals.get(&id),
                    MirPlaceBase::Temp(id) => temps.get(&id),
                };
                ty.is_some_and(|ty| admits_none(table, ty.semantic_id()))
            }
            MirOperand::Temp(id) => temps
                .get(id)
                .is_some_and(|ty| admits_none(table, ty.semantic_id())),
            _ => false,
        };
        let mut calls: Vec<(radix_mir::MirFunctionId, &[MirOperand])> = Vec::new();
        let resolve = |callee: &MirCallee| match callee {
            MirCallee::Function(id) => Some(*id),
            MirCallee::Definition { source, .. } => {
                let mut matches = program
                    .functions
                    .iter()
                    .filter(|candidate| candidate.source == Some(*source));
                match (matches.next(), matches.next()) {
                    (Some(only), None) => Some(only.id),
                    _ => None,
                }
            }
            _ => None,
        };
        for block in &function.blocks {
            for statement in &block.statements {
                if let MirStatementKind::Call { callee, args, .. } = &statement.kind
                    && let Some(id) = resolve(callee)
                {
                    calls.push((id, args));
                }
            }
            if let MirTerminatorKind::TryCall { callee, args, .. } = &block.terminator.kind
                && let Some(id) = resolve(callee)
            {
                calls.push((id, args));
            }
        }
        for (id, args) in calls {
            let Some(target) = program
                .functions
                .iter()
                .find(|candidate| candidate.id == id)
            else {
                continue;
            };
            for (index, arg) in args.iter().enumerate() {
                let Some(param) = target.params.get(index) else {
                    continue;
                };
                if none_like(table, arg) && !found.contains_key(&(id, index)) {
                    let optional = optional_of(table, param.ty);
                    if optional != param.ty {
                        found.insert((id, index), optional);
                    }
                }
            }
        }
    }
    found
}

fn display_tokens(validated: &ValidatedMir<'_>, entry: radix_mir::MirFunctionId) -> [String; 4] {
    let table = &validated.validation().display_tokens;
    match table.for_function(entry) {
        Some(tokens) => [
            tokens.true_.clone(),
            tokens.false_.clone(),
            tokens.none.clone(),
            tokens.tuple.clone(),
        ],
        None => ["true", "false", "none", "tuple"].map(str::to_owned),
    }
}

/// Lower a program that already passed the capability pre-pass. Exposed for
/// fixture tests that have no validation context.
///
/// # Errors
///
/// Returns [`ClifError`] naming the shape the lowering declined.
pub fn emit_program(
    program: &MirProgram,
    types: &TypeTable,
    interner: &Interner,
    entry: Option<radix_mir::MirFunctionId>,
    tokens: [String; 4],
    structs: &StructMeta,
) -> Result<ClifObject, ClifError> {
    let input = ProgramInput {
        structs,
        program,
        types,
        interner,
        entry,
        tokens,
    };
    lower_program(&input)
        .map(|bytes| ClifObject { bytes })
        .map_err(|decline| ClifError::late_decline(&decline.shape))
}

#[cfg(test)]
mod tests {
    use super::{admits_none, optional_of};
    use radix_mir::MirType;
    use radix_types::{Primitive, Type, TypeTable};

    #[test]
    fn a_plain_type_becomes_optional_and_an_optional_stays_itself() {
        let mut table = TypeTable::new();
        let text = table.primitive(Primitive::Textus);
        let plain = MirType::semantic(text);
        let optional = optional_of(&mut table, plain);
        assert_ne!(optional, plain);
        assert!(matches!(table.get(optional.semantic_id()), Type::Option(inner) if *inner == text));
        assert!(admits_none(&table, optional.semantic_id()));
        assert_eq!(optional_of(&mut table, optional), optional);
    }

    #[test]
    fn none_and_unions_with_none_admit_none() {
        let mut table = TypeTable::new();
        let nihil = table.primitive(Primitive::Nihil);
        let numerus = table.primitive(Primitive::Numerus);
        assert!(admits_none(&table, nihil));
        assert!(!admits_none(&table, numerus));
        let union = table.intern(Type::Union(vec![numerus, nihil]));
        assert!(admits_none(&table, union));
    }
}
