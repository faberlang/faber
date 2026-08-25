//! Portable export contracts for file-as-namespace imports.
//!
//! `FileInterface` is the cross-unit boundary for imports: it records a file's
//! public exports without carrying producer-local `TypeId` or `DefId` values.
//! Importers remap structural snapshots into their own semantic `TypeTable`.

use crate::{
    FuncSig, IndexExpr, IndexId, InstansPrecision, NumericWidth, ParamType, Primitive, Resolver,
    SemanticMutability, SemanticParamMode, SymbolKind, Type, TypeId, TypeParamConstraint,
    TypeTable,
};
use radix_hir::{DefId, HirExpression};
use radix_lexer::Interner;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Export contract for one analyzed `.fab` file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileInterface {
    /// Stable provider/package/module identity for locale projection.
    ///
    /// Import binding aliases are deliberately not part of this identity. A
    /// missing identity keeps the historical canonical-name behavior.
    pub identity: Option<InterfaceLibraryIdentity>,
    pub exports: BTreeMap<String, FileExport>,
}

impl FileInterface {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, export: FileExport) {
        self.exports.insert(export.name.clone(), export);
    }

    /// Attach the stable library module identity used by locale packs.
    #[must_use]
    pub fn with_identity(mut self, identity: InterfaceLibraryIdentity) -> Self {
        self.identity = Some(identity);
        self
    }
}

/// Portable identity for one library module's public interface.
///
/// This identity is independent of the consumer's local import binding and
/// therefore remains stable when a module is imported under another alias.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct InterfaceLibraryIdentity {
    pub provider: String,
    pub package: Option<String>,
    pub module_path: Vec<String>,
}

impl InterfaceLibraryIdentity {
    /// Stable display key for diagnostics, tests, and locale pack validation.
    #[must_use]
    pub fn display_key(&self) -> String {
        let mut parts = vec![self.provider.clone()];
        if let Some(package) = &self.package {
            parts.push(package.clone());
        }
        parts.extend(self.module_path.iter().cloned());
        parts.join("::")
    }
}

/// Public member category used by locale projection rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceMemberKind {
    Function,
    Type,
    Const,
    Method,
    Field,
    Variant,
}

/// One top-level public export from a file interface.
#[derive(Debug, Clone, PartialEq)]
pub struct FileExport {
    pub name: String,
    pub kind: FileExportKind,
}

/// Public declaration kinds that can appear in a file interface.
#[derive(Debug, Clone, PartialEq)]
pub enum FileExportKind {
    Function(InterfaceCallable),
    TypeAlias(InterfaceTypeSnapshot),
    Struct(InterfaceStructExport),
    Enum(InterfaceNominalExport),
    Interface(InterfaceNominalExport),
    Const(InterfaceTypeSnapshot),
}

/// Struct declaration contract needed by importers to validate struct literals
/// and receiver method calls without copying source declarations into the
/// importing unit.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceStructExport {
    pub name: String,
    pub fields: Vec<InterfaceStructField>,
    /// Public inherent methods (`genus` methods) remapped like interface methods.
    pub methods: Vec<InterfaceMethodExport>,
    /// Present when the genus was marked `@ annotatio` in the exporting unit.
    pub annotation_contract: Option<InterfaceAnnotationContract>,
}

/// Portable annotation-contract schema exported with a genus.
///
/// Importers install this into the annotation-contract registry so applications
/// can resolve without re-reading the exporter's source AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceAnnotationContract {
    /// Attachment target token (`functio` in v1).
    pub target: String,
    pub fields: Vec<InterfaceAnnotationContractField>,
    /// Qualified portable identity for the contract declaration.
    ///
    /// Provider/package/module/export components allow importers and frameworks
    /// to match contracts without relying on unit-local `DefId` values.
    pub qualified_identity: Option<InterfaceQualifiedIdentity>,
}

/// Portable provider/package/module/export identity for cross-unit handoff.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InterfaceQualifiedIdentity {
    pub provider: String,
    pub package: Option<String>,
    pub module_path: Vec<String>,
    pub export_name: String,
}

/// One portable field in an exported annotation-contract schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceAnnotationContractField {
    pub name: String,
    /// Scalar type spelling (`textus`, `numerus`, `fractus`, `bivalens`).
    pub ty: String,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceStructField {
    pub name: String,
    pub ty: InterfaceTypeSnapshot,
    pub optional: bool,
    pub required: bool,
    /// Producer-side compile-time initializer for imported class construction.
    pub default: Option<HirExpression>,
}

/// Nominal declarations whose import contract is their type identity.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceNominalExport {
    pub name: String,
    pub methods: Vec<InterfaceMethodExport>,
    /// Enum variant declarations (name + optional payload fields). Carried
    /// across the import boundary so consumers can reference library enum
    /// variants as values and construct them (codex-gap S1 U2 VALUE members).
    pub variants: Vec<InterfaceEnumVariant>,
    /// Union `@ commune` fields shared by every variant. Empty for `ordo`
    /// members and unions without a commune region. Member access on the
    /// imported union type (`e.message`) reads this table, not variant
    /// payloads.
    pub shared_fields: Vec<InterfaceStructField>,
}

/// One enum variant carried by a [`InterfaceNominalExport`].
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceEnumVariant {
    pub name: String,
    /// Payload fields in declaration order; empty for unit variants.
    pub fields: Vec<InterfaceStructField>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceMethodExport {
    pub name: String,
    pub callable: InterfaceCallable,
}

/// Structural type snapshot independent from a producer `TypeTable`.
#[derive(Debug, Clone, PartialEq)]
pub enum InterfaceTypeSnapshot {
    Primitive(Primitive),
    Array(Box<InterfaceTypeSnapshot>),
    Queue(Box<InterfaceTypeSnapshot>),
    BoundedQueue(Box<InterfaceTypeSnapshot>, InterfaceIndexSnapshot),
    Stack(Box<InterfaceTypeSnapshot>),
    BoundedStack(Box<InterfaceTypeSnapshot>, InterfaceIndexSnapshot),
    Map(Box<InterfaceTypeSnapshot>, Box<InterfaceTypeSnapshot>),
    Record(BTreeMap<String, InterfaceTypeSnapshot>),
    Set(Box<InterfaceTypeSnapshot>),
    Promissum(Box<InterfaceTypeSnapshot>),
    PromissumFailable(Box<InterfaceTypeSnapshot>, Box<InterfaceTypeSnapshot>),
    Cursor(Box<InterfaceTypeSnapshot>),
    AsyncCursor(Box<InterfaceTypeSnapshot>, Box<InterfaceTypeSnapshot>),
    Tensor(Box<InterfaceTypeSnapshot>, InterfaceIndexSnapshot),
    Vector(Box<InterfaceTypeSnapshot>, InterfaceIndexSnapshot),
    Matrix(Box<InterfaceTypeSnapshot>, InterfaceIndexSnapshot),
    Sparsa(Box<InterfaceTypeSnapshot>, InterfaceIndexSnapshot),
    Atomic(Box<InterfaceTypeSnapshot>),
    Intervallum(Box<InterfaceTypeSnapshot>),
    SizedNumeric(Primitive, NumericWidth),
    ModularWord(NumericWidth),
    SizedInstans(InstansPrecision),
    Option(Box<InterfaceTypeSnapshot>),
    Ref(SemanticMutability, Box<InterfaceTypeSnapshot>),
    Func(InterfaceCallable),
    Param(String),
    Applied(Box<InterfaceTypeSnapshot>, Vec<InterfaceTypeSnapshot>),
    Union(Vec<InterfaceTypeSnapshot>),
    Tuple(Vec<InterfaceTypeSnapshot>),
    Nominal(InterfaceNominalType),
}

/// Snapshot of a type-level index expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterfaceIndexSnapshot {
    Literal(u64),
    Param(String),
    Tuple(Vec<InterfaceIndexSnapshot>),
    Unspecified,
}

/// Callable signature snapshot used by function exports and function types.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceCallable {
    pub type_params: Vec<String>,
    pub type_param_constraints: Vec<InterfaceTypeParamConstraint>,
    pub params: Vec<InterfaceParam>,
    pub ret: Box<InterfaceTypeSnapshot>,
    pub err: Option<Box<InterfaceTypeSnapshot>>,
    pub is_async: bool,
    pub is_generator: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InterfaceTypeParamConstraint {
    Any,
    OneOf(Vec<InterfaceTypeSnapshot>),
}

/// Parameter type and passing mode for a callable snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceParam {
    pub ty: InterfaceTypeSnapshot,
    pub mode: SemanticParamMode,
    pub optional: bool,
}

/// Nominal type identity that must not be guessed during Stage 1 import.
///
/// `identity` is the canonical module-qualified key of the nominal's home
/// module (provider + package + module path). Importers resolve nested
/// nominals through this key so the same nominal referenced from a library
/// analysis and from the consumer analysis unifies to one semantic type in
/// the merged package-MIR program (codex-gap S1 U2 nominal identity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceNominalType {
    pub kind: InterfaceNominalKind,
    pub name: String,
    /// Canonical module-qualified identity of the nominal's home module.
    pub identity: Option<InterfaceLibraryIdentity>,
    /// The AST export snapshot guesses the kind of names the exporting module
    /// does not declare locally (qualified references default to Struct).
    /// When set, importers may resolve the nominal through its module
    /// identity or name under any sibling kind instead of failing on the
    /// guessed kind alone (cds-u6 G4). Authoritative semantic snapshots
    /// (faber package path) leave this false and stay kind-exact
    /// (council-11 correct_before_next_phase).
    pub kind_guessed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InterfaceNominalKind {
    Struct,
    Enum,
    Interface,
}

/// Structured failure from snapshot or import operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileInterfaceError {
    NominalTypeUnsupported {
        kind: InterfaceNominalKind,
    },
    NominalTypeAmbiguous {
        kind: InterfaceNominalKind,
        name: String,
    },
    /// Identity-bearing nominal snapshot whose canonical module-qualified key
    /// the consumer's canonical table does not know. Fail-closed (council-11
    /// correct_before_next_phase): a snapshot carrying a canonical identity
    /// must never fall through to legacy short-name / local-symbol resolution,
    /// which could silently bind an unknown nominal to a different consumer
    /// nominal. Legacy name-based resolution remains available only for
    /// identity-less legacy snapshots.
    NominalIdentityUnknown {
        kind: InterfaceNominalKind,
        name: String,
    },
    InferenceTypeUnsupported,
    ErrorTypeUnsupported,
    IndexInferenceUnsupported,
}

pub type FileInterfaceResult<T> = Result<T, FileInterfaceError>;

// TYPE SNAPSHOT

/// Snapshot a semantic type into a portable interface type.
///
/// # Errors
///
/// Returns [`FileInterfaceError`] when the type is inference-only, an error
/// type, a tensor index still inferring, or a nominal that cannot be named
/// without a resolver.
pub fn snapshot_interface_type(
    type_id: TypeId,
    types: &TypeTable,
    interner: &Interner,
) -> FileInterfaceResult<InterfaceTypeSnapshot> {
    snapshot_interface_type_inner(type_id, types, interner, None, None)
}

/// Snapshot a semantic type with access to nominal declaration names.
///
/// # Errors
///
/// Returns [`FileInterfaceError`] when the type is inference-only, an error
/// type, a tensor index still inferring, or a nominal whose declaration name
/// cannot be recovered.
pub fn snapshot_interface_type_with_resolver(
    type_id: TypeId,
    types: &TypeTable,
    interner: &Interner,
    resolver: &Resolver,
) -> FileInterfaceResult<InterfaceTypeSnapshot> {
    snapshot_interface_type_with_identity(type_id, types, interner, resolver, None)
}

/// Snapshot a semantic type with access to nominal declaration names and the
/// canonical module-qualified identity of the snapshotting module.
///
/// Nested nominal snapshots carry the nominal's home-module identity so the
/// consumer resolves them through the canonical nominal-identity key
/// (codex-gap S1 U2). `module_identity` is the snapshotting module's own
/// identity; nominals declared by *another* imported module resolve their
/// identity through the resolver's file-interface table.
///
/// # Errors
///
/// Returns [`FileInterfaceError`] when the type is inference-only, an error
/// type, a tensor index still inferring, or a nominal whose declaration name
/// cannot be recovered.
pub fn snapshot_interface_type_with_identity(
    type_id: TypeId,
    types: &TypeTable,
    interner: &Interner,
    resolver: &Resolver,
    module_identity: Option<&InterfaceLibraryIdentity>,
) -> FileInterfaceResult<InterfaceTypeSnapshot> {
    snapshot_interface_type_inner(type_id, types, interner, Some(resolver), module_identity)
}

fn snapshot_interface_type_inner(
    type_id: TypeId,
    types: &TypeTable,
    interner: &Interner,
    resolver: Option<&Resolver>,
    module_identity: Option<&InterfaceLibraryIdentity>,
) -> FileInterfaceResult<InterfaceTypeSnapshot> {
    match types.get(type_id) {
        Type::Primitive(primitive) => Ok(InterfaceTypeSnapshot::Primitive(*primitive)),
        Type::Array(element) => Ok(InterfaceTypeSnapshot::Array(Box::new(
            snapshot_interface_type_inner(*element, types, interner, resolver, module_identity)?,
        ))),
        Type::Queue(element) => Ok(InterfaceTypeSnapshot::Queue(Box::new(
            snapshot_interface_type_inner(*element, types, interner, resolver, module_identity)?,
        ))),
        Type::BoundedQueue { element, capacity } => Ok(InterfaceTypeSnapshot::BoundedQueue(
            Box::new(snapshot_interface_type_inner(
                *element,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            snapshot_interface_index(*capacity, types, interner)?,
        )),
        Type::Stack(element) => Ok(InterfaceTypeSnapshot::Stack(Box::new(
            snapshot_interface_type_inner(*element, types, interner, resolver, module_identity)?,
        ))),
        Type::BoundedStack { element, capacity } => Ok(InterfaceTypeSnapshot::BoundedStack(
            Box::new(snapshot_interface_type_inner(
                *element,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            snapshot_interface_index(*capacity, types, interner)?,
        )),
        Type::Map(key, value) => Ok(InterfaceTypeSnapshot::Map(
            Box::new(snapshot_interface_type_inner(
                *key,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            Box::new(snapshot_interface_type_inner(
                *value,
                types,
                interner,
                resolver,
                module_identity,
            )?),
        )),
        Type::Record(fields) => fields
            .iter()
            .map(|(name, field_ty)| {
                Ok((
                    interner.resolve(*name).to_owned(),
                    snapshot_interface_type_inner(
                        *field_ty,
                        types,
                        interner,
                        resolver,
                        module_identity,
                    )?,
                ))
            })
            .collect::<FileInterfaceResult<BTreeMap<_, _>>>()
            .map(InterfaceTypeSnapshot::Record),
        Type::Set(element) => Ok(InterfaceTypeSnapshot::Set(Box::new(
            snapshot_interface_type_inner(*element, types, interner, resolver, module_identity)?,
        ))),
        Type::Promissum(inner) => Ok(InterfaceTypeSnapshot::Promissum(Box::new(
            snapshot_interface_type_inner(*inner, types, interner, resolver, module_identity)?,
        ))),
        Type::PromissumFailable(success, alternate) => {
            Ok(InterfaceTypeSnapshot::PromissumFailable(
                Box::new(snapshot_interface_type_inner(
                    *success,
                    types,
                    interner,
                    resolver,
                    module_identity,
                )?),
                Box::new(snapshot_interface_type_inner(
                    *alternate,
                    types,
                    interner,
                    resolver,
                    module_identity,
                )?),
            ))
        }
        Type::Cursor(inner) => Ok(InterfaceTypeSnapshot::Cursor(Box::new(
            snapshot_interface_type_inner(*inner, types, interner, resolver, module_identity)?,
        ))),
        Type::AsyncCursor(item, alternate) => Ok(InterfaceTypeSnapshot::AsyncCursor(
            Box::new(snapshot_interface_type_inner(
                *item,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            Box::new(snapshot_interface_type_inner(
                *alternate,
                types,
                interner,
                resolver,
                module_identity,
            )?),
        )),
        Type::Tensor(element, shape) => Ok(InterfaceTypeSnapshot::Tensor(
            Box::new(snapshot_interface_type_inner(
                *element,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            snapshot_interface_index(*shape, types, interner)?,
        )),
        Type::Vector(element, width) => Ok(InterfaceTypeSnapshot::Vector(
            Box::new(snapshot_interface_type_inner(
                *element,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            snapshot_interface_index(*width, types, interner)?,
        )),
        Type::Matrix(element, shape) => Ok(InterfaceTypeSnapshot::Matrix(
            Box::new(snapshot_interface_type_inner(
                *element,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            snapshot_interface_index(*shape, types, interner)?,
        )),
        Type::Sparsa(element, shape) => Ok(InterfaceTypeSnapshot::Sparsa(
            Box::new(snapshot_interface_type_inner(
                *element,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            snapshot_interface_index(*shape, types, interner)?,
        )),
        Type::Atomic(element) => Ok(InterfaceTypeSnapshot::Atomic(Box::new(
            snapshot_interface_type_inner(*element, types, interner, resolver, module_identity)?,
        ))),
        Type::Intervallum(bound) => Ok(InterfaceTypeSnapshot::Intervallum(Box::new(
            snapshot_interface_type_inner(*bound, types, interner, resolver, module_identity)?,
        ))),
        Type::SizedNumeric(primitive, width) => {
            Ok(InterfaceTypeSnapshot::SizedNumeric(*primitive, *width))
        }
        Type::ModularWord(width) => Ok(InterfaceTypeSnapshot::ModularWord(*width)),
        Type::SizedInstans(precision) => Ok(InterfaceTypeSnapshot::SizedInstans(*precision)),
        Type::Option(inner) => Ok(InterfaceTypeSnapshot::Option(Box::new(
            snapshot_interface_type_inner(*inner, types, interner, resolver, module_identity)?,
        ))),
        Type::Ref(mutability, inner) => Ok(InterfaceTypeSnapshot::Ref(
            *mutability,
            Box::new(snapshot_interface_type_inner(
                *inner,
                types,
                interner,
                resolver,
                module_identity,
            )?),
        )),
        Type::Struct(def_id) => snapshot_nominal_type(
            *def_id,
            InterfaceNominalKind::Struct,
            interner,
            resolver,
            module_identity,
        ),
        Type::Enum(def_id) => snapshot_nominal_type(
            *def_id,
            InterfaceNominalKind::Enum,
            interner,
            resolver,
            module_identity,
        ),
        Type::Interface(def_id) => snapshot_nominal_type(
            *def_id,
            InterfaceNominalKind::Interface,
            interner,
            resolver,
            module_identity,
        ),
        Type::Alias(_, resolved) => {
            snapshot_interface_type_inner(*resolved, types, interner, resolver, module_identity)
        }
        Type::Func(sig) => Ok(InterfaceTypeSnapshot::Func(
            snapshot_interface_callable_inner(sig, types, interner, resolver, module_identity)?,
        )),
        Type::Param(name) => Ok(InterfaceTypeSnapshot::Param(
            interner.resolve(*name).to_owned(),
        )),
        Type::Applied(base, args) => Ok(InterfaceTypeSnapshot::Applied(
            Box::new(snapshot_interface_type_inner(
                *base,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            args.iter()
                .map(|arg| match arg {
                    radix_types::AppliedArg::Type(id) => snapshot_interface_type_inner(
                        *id,
                        types,
                        interner,
                        resolver,
                        module_identity,
                    ),
                    radix_types::AppliedArg::Size(_) => {
                        Err(FileInterfaceError::IndexInferenceUnsupported)
                    }
                })
                .collect::<FileInterfaceResult<Vec<_>>>()?,
        )),
        Type::Infer(_) => Err(FileInterfaceError::InferenceTypeUnsupported),
        Type::InferUnion(_) => Err(FileInterfaceError::InferenceTypeUnsupported),
        Type::MarkerHole { .. } => Err(FileInterfaceError::InferenceTypeUnsupported),
        Type::Union(members) => members
            .iter()
            .map(|member| {
                snapshot_interface_type_inner(*member, types, interner, resolver, module_identity)
            })
            .collect::<FileInterfaceResult<Vec<_>>>()
            .map(InterfaceTypeSnapshot::Union),
        Type::Tuple(elements) => elements
            .iter()
            .map(|element| {
                snapshot_interface_type_inner(*element, types, interner, resolver, module_identity)
            })
            .collect::<FileInterfaceResult<Vec<_>>>()
            .map(InterfaceTypeSnapshot::Tuple),
        Type::Error => Err(FileInterfaceError::ErrorTypeUnsupported),
        Type::BoundedArray { .. }
        | Type::BoundedTextus { .. }
        | Type::BoundedAscii { .. }
        | Type::BoundedOcteti { .. } => Err(FileInterfaceError::InferenceTypeUnsupported),
    }
}

fn snapshot_nominal_type(
    def_id: DefId,
    kind: InterfaceNominalKind,
    interner: &Interner,
    resolver: Option<&Resolver>,
    module_identity: Option<&InterfaceLibraryIdentity>,
) -> FileInterfaceResult<InterfaceTypeSnapshot> {
    let Some(resolver) = resolver else {
        return Err(FileInterfaceError::NominalTypeUnsupported { kind });
    };
    let Some(symbol) = resolver.get_symbol(def_id) else {
        return Err(FileInterfaceError::NominalTypeUnsupported { kind });
    };
    let identity = resolver
        .nominal_module_identity(def_id)
        .or_else(|| module_identity.cloned());
    Ok(InterfaceTypeSnapshot::Nominal(InterfaceNominalType {
        kind,
        name: interner.resolve(symbol.name).to_owned(),
        identity,
        kind_guessed: false,
    }))
}

// TYPE IMPORT

/// Import a portable interface type into the target semantic type table.
///
/// # Errors
///
/// Returns [`FileInterfaceError`] when a nested nominal cannot be remapped
/// without a resolver.
pub fn import_interface_type(
    snapshot: &InterfaceTypeSnapshot,
    types: &mut TypeTable,
    interner: &mut Interner,
) -> FileInterfaceResult<TypeId> {
    import_interface_type_inner(snapshot, types, interner, None)
}

/// Import a portable interface type with access to collected local declarations.
///
/// # Errors
///
/// Returns [`FileInterfaceError`] when a nominal is missing, ambiguous, or
/// carries a canonical identity the consumer does not know.
pub fn import_interface_type_with_resolver(
    snapshot: &InterfaceTypeSnapshot,
    types: &mut TypeTable,
    interner: &mut Interner,
    resolver: &Resolver,
) -> FileInterfaceResult<TypeId> {
    import_interface_type_inner(snapshot, types, interner, Some(resolver))
}

fn import_interface_type_inner(
    snapshot: &InterfaceTypeSnapshot,
    types: &mut TypeTable,
    interner: &mut Interner,
    resolver: Option<&Resolver>,
) -> FileInterfaceResult<TypeId> {
    match snapshot {
        InterfaceTypeSnapshot::Primitive(primitive) => Ok(types.primitive(*primitive)),
        InterfaceTypeSnapshot::Array(element) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            Ok(types.array(element))
        }
        InterfaceTypeSnapshot::Queue(element) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            Ok(types.queue(element))
        }
        InterfaceTypeSnapshot::BoundedQueue(element, capacity) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            let capacity = import_interface_index(capacity, types, interner)?;
            Ok(types.bounded_queue(element, capacity))
        }
        InterfaceTypeSnapshot::Stack(element) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            Ok(types.stack(element))
        }
        InterfaceTypeSnapshot::BoundedStack(element, capacity) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            let capacity = import_interface_index(capacity, types, interner)?;
            Ok(types.bounded_stack(element, capacity))
        }
        InterfaceTypeSnapshot::Map(key, value) => {
            let key = import_interface_type_inner(key, types, interner, resolver)?;
            let value = import_interface_type_inner(value, types, interner, resolver)?;
            Ok(types.map(key, value))
        }
        InterfaceTypeSnapshot::Record(fields) => {
            let mut remapped = Vec::with_capacity(fields.len());
            for (name, field_ty) in fields {
                remapped.push((
                    interner.intern(name),
                    import_interface_type_inner(field_ty, types, interner, resolver)?,
                ));
            }
            Ok(types.record(remapped))
        }
        InterfaceTypeSnapshot::Set(element) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            Ok(types.set(element))
        }
        InterfaceTypeSnapshot::Promissum(inner) => {
            let inner = import_interface_type_inner(inner, types, interner, resolver)?;
            Ok(types.promissum(inner))
        }
        InterfaceTypeSnapshot::PromissumFailable(success, alternate) => {
            let success = import_interface_type_inner(success, types, interner, resolver)?;
            let alternate = import_interface_type_inner(alternate, types, interner, resolver)?;
            Ok(types.promissum_failable(success, alternate))
        }
        InterfaceTypeSnapshot::Cursor(inner) => {
            let inner = import_interface_type_inner(inner, types, interner, resolver)?;
            Ok(types.cursor(inner))
        }
        InterfaceTypeSnapshot::AsyncCursor(item, alternate) => {
            let item = import_interface_type_inner(item, types, interner, resolver)?;
            let alternate = import_interface_type_inner(alternate, types, interner, resolver)?;
            Ok(types.async_cursor(item, alternate))
        }
        InterfaceTypeSnapshot::Tensor(element, shape) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            let shape = import_interface_index(shape, types, interner)?;
            Ok(types.tensor_with_shape(element, shape))
        }
        InterfaceTypeSnapshot::Vector(element, width) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            let width = import_interface_index(width, types, interner)?;
            Ok(types.vector_with_width(element, width))
        }
        InterfaceTypeSnapshot::Matrix(element, shape) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            let shape = import_interface_index(shape, types, interner)?;
            Ok(types.matrix_with_shape(element, shape))
        }
        InterfaceTypeSnapshot::Sparsa(element, shape) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            let shape = import_interface_index(shape, types, interner)?;
            Ok(types.sparsa_with_shape(element, shape))
        }
        InterfaceTypeSnapshot::Atomic(element) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            Ok(types.atomic(element))
        }
        InterfaceTypeSnapshot::Intervallum(bound) => {
            let bound = import_interface_type_inner(bound, types, interner, resolver)?;
            Ok(types.intervallum(bound))
        }
        InterfaceTypeSnapshot::SizedNumeric(primitive, width) => {
            Ok(types.sized_numeric(*primitive, *width))
        }
        InterfaceTypeSnapshot::ModularWord(width) => Ok(types.intern(Type::ModularWord(*width))),
        InterfaceTypeSnapshot::SizedInstans(precision) => Ok(types.sized_instans(*precision)),
        InterfaceTypeSnapshot::Option(inner) => {
            let inner = import_interface_type_inner(inner, types, interner, resolver)?;
            Ok(types.option(inner))
        }
        InterfaceTypeSnapshot::Ref(mutability, inner) => {
            let inner = import_interface_type_inner(inner, types, interner, resolver)?;
            Ok(types.reference(*mutability, inner))
        }
        InterfaceTypeSnapshot::Func(callable) => {
            let sig = import_interface_callable_inner(callable, types, interner, resolver)?;
            Ok(types.intern(Type::Func(sig)))
        }
        InterfaceTypeSnapshot::Param(name) => Ok(types.intern(Type::Param(interner.intern(name)))),
        InterfaceTypeSnapshot::Applied(base, args) => {
            let base = import_interface_type_inner(base, types, interner, resolver)?;
            let args = args
                .iter()
                .map(|arg| {
                    import_interface_type_inner(arg, types, interner, resolver)
                        .map(radix_types::AppliedArg::Type)
                })
                .collect::<FileInterfaceResult<Vec<_>>>()?;
            Ok(types.intern(Type::Applied(base, args)))
        }
        InterfaceTypeSnapshot::Union(members) => {
            let members = members
                .iter()
                .map(|member| import_interface_type_inner(member, types, interner, resolver))
                .collect::<FileInterfaceResult<Vec<_>>>()?;
            Ok(types.intern(Type::Union(members)))
        }
        InterfaceTypeSnapshot::Tuple(elements) => {
            let elements = elements
                .iter()
                .map(|element| import_interface_type_inner(element, types, interner, resolver))
                .collect::<FileInterfaceResult<Vec<_>>>()?;
            Ok(types.tuple(elements))
        }
        InterfaceTypeSnapshot::Nominal(nominal) => {
            import_nominal_type(nominal, types, interner, resolver)
        }
    }
}

fn import_nominal_type(
    nominal: &InterfaceNominalType,
    types: &mut TypeTable,
    interner: &mut Interner,
    resolver: Option<&Resolver>,
) -> FileInterfaceResult<TypeId> {
    let Some(resolver) = resolver else {
        return Err(FileInterfaceError::NominalTypeUnsupported { kind: nominal.kind });
    };
    let name = interner.intern(&nominal.name);
    // Canonical nominal-identity lookup first: the nominal snapshot carries
    // its home module's qualified identity, and consumer imports register
    // nominals under that key. Same-name nominals from different module
    // identities therefore resolve to distinct semantic types (codex-gap S1
    // U2 negative proof: coincident raw DefIds must not unify). Identity-less
    // nominals keep the legacy name-keyed path below (ambiguity-guarded).
    //
    // The kind is tried flexibly (exact first, then the sibling kinds) ONLY
    // when the snapshot marks it as guessed — the AST export snapshot
    // defaults qualified/unknown names to Struct, while the consumer
    // registers the nominal under its true declared kind. Within one module
    // identity a name denotes exactly one nominal, so a kind-flexible
    // canonical lookup cannot over-unify (cds-u6 G4: `fn bar(...) →
    // visus.Visus` must not be export-skipped on a kind guess alone).
    // Authoritative semantic snapshots (kind_guessed = false) keep the
    // kind-exact fail-closed contract (council-11).
    if let Some(identity) = nominal.identity.as_ref() {
        if nominal.kind_guessed {
            for candidate in [
                nominal.kind,
                InterfaceNominalKind::Enum,
                InterfaceNominalKind::Struct,
                InterfaceNominalKind::Interface,
            ] {
                if let Some(type_id) =
                    resolver.imported_nominal_type_by_identity(candidate, Some(identity), name)
                {
                    return Ok(type_id);
                }
            }
        } else if let Some(type_id) =
            resolver.imported_nominal_type_by_identity(nominal.kind, Some(identity), name)
        {
            return Ok(type_id);
        }
        // Fail-closed (council-11 correct_before_next_phase): a snapshot that
        // carries a canonical identity must NOT fall through to the legacy
        // short-name / local-symbol resolution below — a canonical-table miss
        // means this consumer does not know the nominal's home module, and
        // name-based fallback could silently bind the unknown nominal to a
        // DIFFERENT consumer nominal. Legacy name-based resolution stays
        // available only for identity-less snapshots.
        return Err(FileInterfaceError::NominalIdentityUnknown {
            kind: nominal.kind,
            name: nominal.name.clone(),
        });
    }
    if resolver.imported_nominal_type_is_ambiguous(nominal.kind, name) {
        return Err(FileInterfaceError::NominalTypeAmbiguous {
            kind: nominal.kind,
            name: nominal.name.clone(),
        });
    }
    if let Some(type_id) = resolver.imported_nominal_type(nominal.kind, name) {
        return Ok(type_id);
    }
    // Kind-flexible legacy fallback: identity-less snapshots from the AST
    // export builder carry a guessed kind for names the exporting module does
    // not declare locally. Never pick an ambiguous name-keyed entry (that
    // would silently bind a colliding short name from another namespace).
    if nominal.kind_guessed {
        for candidate in [
            InterfaceNominalKind::Enum,
            InterfaceNominalKind::Struct,
            InterfaceNominalKind::Interface,
        ] {
            if candidate == nominal.kind
                || resolver.imported_nominal_type_is_ambiguous(candidate, name)
            {
                continue;
            }
            if let Some(type_id) = resolver.imported_nominal_type(candidate, name) {
                return Ok(type_id);
            }
        }
    }
    let Some(def_id) = resolver.lookup_type(name) else {
        return Err(FileInterfaceError::NominalTypeUnsupported { kind: nominal.kind });
    };
    let Some(symbol) = resolver.get_symbol(def_id) else {
        return Err(FileInterfaceError::NominalTypeUnsupported { kind: nominal.kind });
    };
    match (nominal.kind, symbol.kind) {
        (InterfaceNominalKind::Struct, SymbolKind::Struct) => Ok(symbol
            .ty
            .unwrap_or_else(|| types.intern(Type::Struct(def_id)))),
        (InterfaceNominalKind::Enum, SymbolKind::Enum) => Ok(symbol
            .ty
            .unwrap_or_else(|| types.intern(Type::Enum(def_id)))),
        (InterfaceNominalKind::Interface, SymbolKind::Interface) => Ok(symbol
            .ty
            .unwrap_or_else(|| types.intern(Type::Interface(def_id)))),
        _ => Err(FileInterfaceError::NominalTypeUnsupported { kind: nominal.kind }),
    }
}

// CALLABLE CONTRACT

/// Import a portable callable into a target [`FuncSig`].
///
/// # Errors
///
/// Returns [`FileInterfaceError`] when a parameter, return, error, or type-param
/// constraint type cannot be remapped.
pub fn import_interface_callable_with_resolver(
    callable: &InterfaceCallable,
    types: &mut TypeTable,
    interner: &mut Interner,
    resolver: &Resolver,
) -> FileInterfaceResult<FuncSig> {
    import_interface_callable_inner(callable, types, interner, Some(resolver))
}

fn import_interface_callable_inner(
    callable: &InterfaceCallable,
    types: &mut TypeTable,
    interner: &mut Interner,
    resolver: Option<&Resolver>,
) -> FileInterfaceResult<FuncSig> {
    let mut ret = import_interface_type_inner(&callable.ret, types, interner, resolver)?;
    let mut err = callable
        .err
        .as_ref()
        .map(|err| import_interface_type_inner(err, types, interner, resolver))
        .transpose()?;
    if callable.is_async
        && callable.is_generator
        && !matches!(types.get(ret), Type::AsyncCursor(_, _))
    {
        let alternate = err
            .take()
            .unwrap_or_else(|| types.primitive(Primitive::Numquam));
        ret = types.async_cursor(ret, alternate);
    } else if callable.is_async
        && !callable.is_generator
        && !matches!(
            types.get(ret),
            Type::Promissum(_) | Type::PromissumFailable(_, _)
        )
    {
        ret = if let Some(err_ty) = err.take() {
            types.promissum_failable(ret, err_ty)
        } else {
            types.promissum(ret)
        };
    }
    if callable.is_generator && !callable.is_async && !matches!(types.get(ret), Type::Array(_)) {
        ret = types.array(ret);
    }
    Ok(FuncSig {
        type_params: callable
            .type_params
            .iter()
            .map(|name| interner.intern(name))
            .collect(),
        type_param_constraints: callable
            .type_param_constraints
            .iter()
            .map(|constraint| import_type_param_constraint(constraint, types, interner))
            .collect::<FileInterfaceResult<Vec<_>>>()?,
        params: callable
            .params
            .iter()
            .map(|param| {
                Ok(ParamType {
                    ty: import_interface_type_inner(&param.ty, types, interner, resolver)?,
                    mode: param.mode,
                    optional: param.optional,
                })
            })
            .collect::<FileInterfaceResult<Vec<_>>>()?,
        ret,
        err,
        is_async: callable.is_async,
        is_generator: callable.is_generator,
    })
}

/// Snapshot a callable with nominal names resolved through the resolver.
///
/// # Errors
///
/// Returns [`FileInterfaceError`] when a parameter, return, error, or type-param
/// constraint type cannot be snapshotted.
pub fn snapshot_interface_callable_with_resolver(
    sig: &FuncSig,
    types: &TypeTable,
    interner: &Interner,
    resolver: &Resolver,
) -> FileInterfaceResult<InterfaceCallable> {
    snapshot_interface_callable_inner(sig, types, interner, Some(resolver), None)
}

/// Snapshot a callable with nominal names resolved through the resolver and
/// the canonical module-qualified identity of the snapshotting module
/// (codex-gap S1 U2 nominal identity for nested callable nominals).
///
/// # Errors
///
/// Returns [`FileInterfaceError`] when a parameter, return, error, or type-param
/// constraint type cannot be snapshotted.
pub fn snapshot_interface_callable_with_identity(
    sig: &FuncSig,
    types: &TypeTable,
    interner: &Interner,
    resolver: &Resolver,
    module_identity: Option<&InterfaceLibraryIdentity>,
) -> FileInterfaceResult<InterfaceCallable> {
    snapshot_interface_callable_inner(sig, types, interner, Some(resolver), module_identity)
}

fn snapshot_interface_callable_inner(
    sig: &FuncSig,
    types: &TypeTable,
    interner: &Interner,
    resolver: Option<&Resolver>,
    module_identity: Option<&InterfaceLibraryIdentity>,
) -> FileInterfaceResult<InterfaceCallable> {
    Ok(InterfaceCallable {
        type_params: sig
            .type_params
            .iter()
            .map(|param| interner.resolve(*param).to_owned())
            .collect(),
        type_param_constraints: sig
            .type_param_constraints
            .iter()
            .map(|constraint| snapshot_type_param_constraint(constraint, types, interner))
            .collect::<FileInterfaceResult<Vec<_>>>()?,
        params: sig
            .params
            .iter()
            .map(|param| {
                Ok(InterfaceParam {
                    ty: snapshot_interface_type_inner(
                        param.ty,
                        types,
                        interner,
                        resolver,
                        module_identity,
                    )?,
                    mode: param.mode,
                    optional: param.optional,
                })
            })
            .collect::<FileInterfaceResult<Vec<_>>>()?,
        ret: Box::new(snapshot_interface_type_inner(
            sig.ret,
            types,
            interner,
            resolver,
            module_identity,
        )?),
        err: sig
            .err
            .map(|err| {
                snapshot_interface_type_inner(err, types, interner, resolver, module_identity)
                    .map(Box::new)
            })
            .transpose()?,
        is_async: sig.is_async,
        is_generator: sig.is_generator,
    })
}

fn import_type_param_constraint(
    constraint: &InterfaceTypeParamConstraint,
    types: &mut TypeTable,
    interner: &mut Interner,
) -> FileInterfaceResult<TypeParamConstraint> {
    match constraint {
        InterfaceTypeParamConstraint::Any => Ok(TypeParamConstraint::Any),
        InterfaceTypeParamConstraint::OneOf(options) => options
            .iter()
            .map(|option| import_interface_type(option, types, interner))
            .collect::<FileInterfaceResult<Vec<_>>>()
            .map(TypeParamConstraint::OneOf),
    }
}

fn snapshot_type_param_constraint(
    constraint: &TypeParamConstraint,
    types: &TypeTable,
    interner: &Interner,
) -> FileInterfaceResult<InterfaceTypeParamConstraint> {
    match constraint {
        TypeParamConstraint::Any => Ok(InterfaceTypeParamConstraint::Any),
        TypeParamConstraint::OneOf(options) => options
            .iter()
            .map(|option| snapshot_interface_type(*option, types, interner))
            .collect::<FileInterfaceResult<Vec<_>>>()
            .map(InterfaceTypeParamConstraint::OneOf),
    }
}

fn snapshot_interface_index(
    index_id: IndexId,
    types: &TypeTable,
    interner: &Interner,
) -> FileInterfaceResult<InterfaceIndexSnapshot> {
    match types.get_index(index_id) {
        IndexExpr::Literal(value) => Ok(InterfaceIndexSnapshot::Literal(*value)),
        IndexExpr::Param(name) => Ok(InterfaceIndexSnapshot::Param(
            interner.resolve(*name).to_owned(),
        )),
        IndexExpr::Tuple(dims) => dims
            .iter()
            .map(|dim| snapshot_interface_index(*dim, types, interner))
            .collect::<FileInterfaceResult<Vec<_>>>()
            .map(InterfaceIndexSnapshot::Tuple),
        IndexExpr::Infer(_) => Err(FileInterfaceError::IndexInferenceUnsupported),
        IndexExpr::Unspecified => Ok(InterfaceIndexSnapshot::Unspecified),
    }
}

fn import_interface_index(
    snapshot: &InterfaceIndexSnapshot,
    types: &mut TypeTable,
    interner: &mut Interner,
) -> FileInterfaceResult<IndexId> {
    match snapshot {
        InterfaceIndexSnapshot::Literal(value) => {
            Ok(types.intern_index(IndexExpr::Literal(*value)))
        }
        InterfaceIndexSnapshot::Param(name) => {
            Ok(types.intern_index(IndexExpr::Param(interner.intern(name))))
        }
        InterfaceIndexSnapshot::Tuple(dims) => dims
            .iter()
            .map(|dim| import_interface_index(dim, types, interner))
            .collect::<FileInterfaceResult<Vec<_>>>()
            .map(IndexExpr::Tuple)
            .map(|tuple| types.intern_index(tuple)),
        InterfaceIndexSnapshot::Unspecified => Ok(types.unspecified_shape()),
    }
}
