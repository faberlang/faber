//! FHIR **package** build and load (source-independent package reconstruction).
//!
//! This module is the compiler-side owner of the package envelope contract:
//! [`build_package`] assembles a [`FhirPackage`] from analyzed units plus
//! package metadata (the wire types and codec live in `radix-hir-fhir`), and
//! [`load_package`] reconstructs a [`LoadedHirPackage`] whose units are full
//! [`AnalyzedModule`]s — re-entering the compiler at the codegen boundary with
//! no lexer, parser, resolver, semantic-analysis, or lowering pass.
//!
//! [`load_package`] never treats unit-local `DefId`/`HirId`/`TypeId`/`Symbol`
//! values as package-global; cross-unit references travel as module/export
//! identities (the module table + serialized file interfaces inside each
//! unit's resolver snapshot).

use crate::driver::AnalyzedModule;
use crate::hir::{
    DefId, HirBlock, HirExpression, HirExpressionKind, HirFunction, HirItemKind, HirStatementKind,
    HirTypeParamConstraint, NucleumRole,
};
use crate::semantic::format_faber_type;
use crate::semantic::passes::fragment_definition::{DeviceEffectKind, FragmentEffectTable};
use radix_hir_fhir::{FhirPackageModule, SCHEMA_VERSION};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

pub use radix_hir_fhir::{
    decode_package, encode_package, DecodedFhirPackage, FhirPackage, FhirPackageError, LibraryImportWire,
    LocalLinkWire, PackageDependencyWire, PackageIdentityWire, PACKAGE_SCHEMA_VERSION,
};

/// Compiler version recorded on fragment body artifacts (radix-module crate).
pub const NUCLEUM_COMPILER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// ABI version recorded on fragment body artifacts and cache keys.
///
/// Matches [`crate::mir::HOST_CALL_ABI_VERSION`] when `mir-core` is on; kept as
/// a package-crate constant so FHIR-only builds can still serialize metadata.
pub const NUCLEUM_FRAGMENT_ABI_VERSION: u32 = 8;

/// One analyzed unit input for package assembly.
///
/// All strings are package-relative or provider-qualified — never absolute
/// checkout paths (contract gate).
pub struct PackageUnitInput<'a> {
    /// Package-root-relative canonical path (e.g. `src/main.fab`).
    pub relative_path: String,
    pub module_segments: Vec<String>,
    pub is_entry: bool,
    /// Public export names of this module (source-free surface for codegen
    /// assembly).
    pub export_names: Vec<String>,
    /// Local import links (binding → target module path), the explicit link
    /// table captured at analysis time.
    pub local_links: Vec<LocalLinkWire>,
    /// Library imports referenced by this module (binding/package/module).
    pub library_imports: Vec<LibraryImportWire>,
    /// SHA-256 hex of the original source bytes — also stored as the unit
    /// artifact's `SourceIdentity.content_hash` and re-checked on decode.
    pub content_hash: String,
    pub analysis: &'a AnalyzedModule,
}

/// Assemble a [`FhirPackage`] from analyzed units and package metadata.
///
/// Deterministic by construction: modules are sorted by canonical relative
/// path, export names and library imports are sorted, and dependencies are
/// sorted by name. Each unit is serialized through the existing unit path
/// (`crate::hir::serialize::from_analyzed` + `radix_hir_fhir::encode`), so
/// package work never duplicates the unit artifact contract.
///
/// # Errors
///
/// Returns [`FhirPackageError`] when package identity, module ordering,
/// dependency metadata, or unit serialization is invalid.
pub fn build_package(
    identity: PackageIdentityWire,
    entry_path: &str,
    entry_frontmatter: Option<String>,
    modules: Vec<PackageUnitInput<'_>>,
    dependencies: Vec<PackageDependencyWire>,
) -> Result<FhirPackage, FhirPackageError> {
    let (package, _) = build_package_with_nucleum(identity, entry_path, entry_frontmatter, modules, dependencies)?;
    Ok(package)
}

/// Assemble a FHIR envelope together with nucleum program metadata.
///
/// Fragment exports fail closed when they have no body. The concrete fragment
/// call graph is required to be acyclic. Only [`NucleumRole::Entry`] items
/// enter [`NucleumProgramMetadata::dispatch_and_reflection_names`].
///
/// # Errors
///
/// Returns [`FhirPackageError`] when the envelope is invalid or a fragment
/// export is signature-only / cyclic.
pub fn build_package_with_nucleum(
    identity: PackageIdentityWire,
    entry_path: &str,
    entry_frontmatter: Option<String>,
    mut modules: Vec<PackageUnitInput<'_>>,
    mut dependencies: Vec<PackageDependencyWire>,
) -> Result<(FhirPackage, NucleumProgramMetadata), FhirPackageError> {
    modules.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let nucleum = nucleum_metadata_from_unit_inputs(&modules).map_err(NucleumPackageError::into_fhir)?;

    let mut wire_modules = Vec::with_capacity(modules.len());
    for input in modules {
        let mut artifact =
            crate::hir::serialize::from_analyzed(input.analysis, &input.content_hash, &input.relative_path);
        // Package artifacts are deterministic interchange: `analysis_stamp`
        // is a live-process run counter (monotonic per process), not content.
        // Reset it so rebuilding the same source yields byte-identical
        // packages — required for stable package content hashes in the store
        // and the deterministic-serialization contract gate. Loaded units
        // reconstruct the stamp via `AnalysisStamp::from_persisted(0)`; the
        // stamp is only meaningful while fields of one run are assembled
        // in-process, which package loading does not do.
        artifact.analysis_stamp = 0;
        let mut export_names = input.export_names;
        export_names.sort();
        export_names.dedup();
        let mut local_links = input.local_links;
        local_links.sort_by(|left, right| left.binding.cmp(&right.binding));
        let mut library_imports = input.library_imports;
        library_imports.sort_by(|left, right| {
            (&left.binding, &left.package, &left.module).cmp(&(&right.binding, &right.package, &right.module))
        });
        wire_modules.push(FhirPackageModule {
            path: input.relative_path,
            module_segments: input.module_segments,
            is_entry: input.is_entry,
            export_names,
            local_links,
            library_imports,
            source_hash: input.content_hash,
            unit_schema_version: SCHEMA_VERSION,
            unit: radix_hir_fhir::encode(&artifact).map_err(FhirPackageError::Malformed)?,
        });
    }

    dependencies.sort_by(|left, right| left.name.cmp(&right.name));

    Ok((
        FhirPackage {
            package_schema_version: PACKAGE_SCHEMA_VERSION,
            identity,
            entry_path: entry_path.to_owned(),
            entry_frontmatter,
            modules: wire_modules,
            dependencies,
        },
        nucleum,
    ))
}

/// One module of a loaded package: envelope metadata plus a fully
/// reconstructed [`AnalyzedModule`].
pub struct LoadedHirModule {
    pub relative_path: String,
    pub module_segments: Vec<String>,
    pub is_entry: bool,
    pub export_names: Vec<String>,
    pub local_links: Vec<LocalLinkWire>,
    pub library_imports: Vec<LibraryImportWire>,
    pub unit: AnalyzedModule,
}

/// A source-independently loaded FHIR package.
pub struct LoadedHirPackage {
    pub identity: PackageIdentityWire,
    pub entry_path: String,
    pub entry_frontmatter: Option<String>,
    pub modules: Vec<LoadedHirModule>,
    pub dependencies: Vec<PackageDependencyWire>,
    /// Nucleum role/body metadata reconstructed from loaded units.
    pub nucleum: NucleumProgramMetadata,
}

/// Load a FHIR package from `postcard` bytes.
///
/// The format crate's [`decode_package`] runs the package version gate, the
/// per-unit gates (unit schema version + F3 + U2.3 referential integrity +
/// source-hash consistency), and the package-level module/dependency checks;
/// this function only reconstructs arenas per unit via
/// [`crate::hir::artifact::reconstruct_unit`] (U2.2), the same path the
/// single-unit loader uses. Never panics on the load path (F7) and never
/// invokes frontend phases.
///
/// # Errors
///
/// Returns [`FhirPackageError`] when package decoding, validation, or unit
/// reconstruction fails.
pub fn load_package(bytes: &[u8]) -> Result<LoadedHirPackage, FhirPackageError> {
    let decoded = decode_package(bytes)?;
    let mut modules = Vec::with_capacity(decoded.modules.len());
    for module in decoded.modules {
        let unit = crate::hir::artifact::reconstruct_unit(module.unit).map_err(|error| {
            FhirPackageError::MalformedUnit { module: module.path.clone(), detail: error.to_string() }
        })?;
        modules.push(LoadedHirModule {
            relative_path: module.path,
            module_segments: module.module_segments,
            is_entry: module.is_entry,
            export_names: module.export_names,
            local_links: module.local_links,
            library_imports: module.library_imports,
            unit,
        });
    }
    let nucleum = nucleum_metadata_from_loaded_modules(&modules).map_err(NucleumPackageError::into_fhir)?;
    Ok(LoadedHirPackage {
        identity: decoded.identity,
        entry_path: decoded.entry_path,
        entry_frontmatter: decoded.entry_frontmatter,
        modules,
        dependencies: decoded.dependencies,
        nucleum,
    })
}

// ---------------------------------------------------------------------------
// Nucleum fragment package metadata (NF-8)
// ---------------------------------------------------------------------------

/// Structured failure for nucleum package metadata / import checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NucleumPackageError {
    /// A `Fragment` export has no body artifact (signature-only interface).
    SignatureOnlyImport { name: String, module: String },
    /// Concrete fragment call graph contains a cycle after import resolution.
    CyclicCallGraph { cycle: String },
    /// A fragment is importable but has no canonical MIR body bytes.
    MirBodyMissing { name: String, module: String },
}

impl NucleumPackageError {
    fn into_fhir(self) -> FhirPackageError {
        match self {
            Self::SignatureOnlyImport { name, module } => FhirPackageError::MalformedUnit {
                module,
                detail: format!("signature-only nucleum fragment `{name}` cannot be imported"),
            },
            Self::CyclicCallGraph { cycle } => FhirPackageError::MalformedUnit {
                module: String::new(),
                detail: format!("nucleum fragment call cycle: {cycle}"),
            },
            Self::MirBodyMissing { name, module } => FhirPackageError::MalformedUnit {
                module,
                detail: format!("nucleum fragment `{name}` is missing a canonical MIR body artifact"),
            },
        }
    }

    /// Diagnostic issue identifier (`nucleum_fragment_effect` / `_cycle`).
    #[must_use]
    pub fn issue(&self) -> &'static str {
        match self {
            Self::SignatureOnlyImport { .. } | Self::MirBodyMissing { .. } => "nucleum_fragment_effect",
            Self::CyclicCallGraph { .. } => "nucleum_fragment_cycle",
        }
    }

    /// Catalog `reason` argument.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::SignatureOnlyImport { .. } | Self::MirBodyMissing { .. } => "missing_body",
            Self::CyclicCallGraph { .. } => "cycle",
        }
    }
}

impl std::fmt::Display for NucleumPackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SignatureOnlyImport { name, module } => {
                write!(f, "signature-only nucleum fragment `{name}` in `{module}` cannot be imported")
            }
            Self::CyclicCallGraph { cycle } => write!(f, "nucleum fragment call cycle: {cycle}"),
            Self::MirBodyMissing { name, module } => {
                write!(f, "nucleum fragment `{name}` in `{module}` is missing a canonical MIR body")
            }
        }
    }
}

impl std::error::Error for NucleumPackageError {}

/// One type or shape constraint on a fragment definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FragmentConstraint {
    pub param: String,
    pub binding: String,
}

/// One visible device effect recorded on a fragment body artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FragmentEffectRecord {
    pub kind: String,
}

/// Canonical specialized-MIR body artifact for an importable fragment (goal OQ2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FragmentBodyArtifact {
    /// Postcard-encoded [`crate::mir::MirFunction`] for the specialized body.
    pub mir: Vec<u8>,
    pub type_constraints: Vec<FragmentConstraint>,
    pub shape_constraints: Vec<FragmentConstraint>,
    pub effect_summary: Vec<FragmentEffectRecord>,
    pub compiler_version: String,
    pub abi_version: u32,
    pub body_hash: String,
}

/// One nucleum item recorded in program metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NucleumItemMetadata {
    pub name: String,
    pub relative_path: String,
    pub def_id: u32,
    pub role: NucleumRole,
    /// HIR body present in the exporting unit.
    pub has_hir_body: bool,
    /// Canonical MIR artifact; required for an importable `Fragment`.
    pub body: Option<FragmentBodyArtifact>,
    /// Direct fragment callees (names) for the post-import call graph.
    pub fragment_callees: Vec<String>,
}

/// Serialized program metadata for nucleum roles and fragment bodies.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NucleumProgramMetadata {
    pub compiler_version: String,
    pub abi_version: u32,
    pub items: Vec<NucleumItemMetadata>,
}

impl NucleumProgramMetadata {
    /// Empty metadata stamped with this compiler/ABI version.
    #[must_use]
    pub fn new() -> Self {
        Self {
            compiler_version: NUCLEUM_COMPILER_VERSION.to_owned(),
            abi_version: NUCLEUM_FRAGMENT_ABI_VERSION,
            items: Vec::new(),
        }
    }

    /// Names that enter device dispatch and reflection tables (`Entry` only).
    #[must_use]
    pub fn dispatch_and_reflection_names(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|item| item.role == NucleumRole::Entry)
            .map(|item| item.name.clone())
            .collect()
    }

    /// Fragment names omitted from target symbol tables after composition.
    #[must_use]
    pub fn omitted_target_symbol_names(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|item| item.role == NucleumRole::Fragment)
            .map(|item| item.name.clone())
            .collect()
    }

    /// Merge `other` into this metadata (import resolution).
    pub fn merge(&mut self, other: Self) {
        self.items.extend(other.items);
        self.items
            .sort_by(|left, right| (&left.relative_path, &left.name).cmp(&(&right.relative_path, &right.name)));
    }

    /// Fail closed when any `Fragment` is signature-only.
    ///
    /// # Errors
    ///
    /// Returns [`NucleumPackageError::SignatureOnlyImport`] when a fragment has
    /// no HIR body and no MIR artifact.
    pub fn require_importable_fragment_bodies(&self) -> Result<(), NucleumPackageError> {
        for item in &self.items {
            if item.role != NucleumRole::Fragment {
                continue;
            }
            let has_mir = item.body.as_ref().is_some_and(|body| !body.mir.is_empty());
            if item.has_hir_body || has_mir {
                continue;
            }
            return Err(NucleumPackageError::SignatureOnlyImport {
                name: item.name.clone(),
                module: item.relative_path.clone(),
            });
        }
        Ok(())
    }

    /// Fail closed when an importable fragment has no canonical MIR bytes.
    ///
    /// # Errors
    ///
    /// Returns [`NucleumPackageError::MirBodyMissing`] when a fragment artifact
    /// is missing postcard MIR.
    pub fn require_canonical_mir(&self) -> Result<(), NucleumPackageError> {
        for item in &self.items {
            if item.role != NucleumRole::Fragment {
                continue;
            }
            if item.body.as_ref().is_some_and(|body| !body.mir.is_empty()) {
                continue;
            }
            return Err(NucleumPackageError::MirBodyMissing {
                name: item.name.clone(),
                module: item.relative_path.clone(),
            });
        }
        Ok(())
    }

    /// Fail closed when the concrete fragment call graph has a cycle.
    ///
    /// # Errors
    ///
    /// Returns [`NucleumPackageError::CyclicCallGraph`] with the cycle path.
    pub fn require_acyclic(&self) -> Result<(), NucleumPackageError> {
        if let Some(cycle) = fragment_cycle(self) {
            return Err(NucleumPackageError::CyclicCallGraph { cycle });
        }
        Ok(())
    }

    /// Attach postcard MIR bodies from a lowered unit (goal OQ2 canonical MIR).
    #[cfg(feature = "mir-core")]
    pub fn attach_canonical_mir(&mut self, program: &crate::mir::MirProgram, relative_path: &str) {
        for item in &mut self.items {
            if item.relative_path != relative_path || item.role != NucleumRole::Fragment {
                continue;
            }
            let Some(function) = program
                .functions
                .iter()
                .find(|function| function.source == Some(DefId(item.def_id)))
            else {
                continue;
            };
            let Ok(mir) = postcard::to_allocvec(function) else {
                continue;
            };
            let body_hash = content_hash(&mir);
            if let Some(body) = &mut item.body {
                body.mir = mir;
                body.body_hash = body_hash;
            }
        }
    }
}

/// Deterministic cache key for a specialized fragment instance.
///
/// Includes body hash, compiler/ABI version, target capabilities, and the
/// concrete type/shape bindings (design §8).
#[must_use]
pub fn fragment_cache_key(
    artifact: &FragmentBodyArtifact,
    target_capabilities: &[&str],
    type_bindings: &[&str],
    shape_bindings: &[&str],
) -> String {
    let mut caps = target_capabilities.to_vec();
    caps.sort_unstable();
    let mut types = type_bindings.to_vec();
    types.sort_unstable();
    let mut shapes = shape_bindings.to_vec();
    shapes.sort_unstable();
    let mut canonical = Vec::new();
    push_field(&mut canonical, "compiler", artifact.compiler_version.as_bytes());
    push_field(&mut canonical, "abi", &artifact.abi_version.to_le_bytes());
    push_field(&mut canonical, "body", artifact.body_hash.as_bytes());
    push_field(&mut canonical, "caps", caps.join(",").as_bytes());
    push_field(&mut canonical, "types", types.join(",").as_bytes());
    push_field(&mut canonical, "shapes", shapes.join(",").as_bytes());
    content_hash(&canonical)
}

/// Postcard-encode nucleum program metadata (sidecar next to `.fhirpkg`).
///
/// # Errors
///
/// Returns the postcard codec error when the metadata cannot be encoded.
pub fn encode_nucleum_metadata(metadata: &NucleumProgramMetadata) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(metadata)
}

/// Decode nucleum program metadata from postcard bytes.
///
/// # Errors
///
/// Returns the postcard codec error when the payload is not valid metadata.
pub fn decode_nucleum_metadata(bytes: &[u8]) -> Result<NucleumProgramMetadata, postcard::Error> {
    postcard::from_bytes(bytes)
}

/// Collect nucleum metadata from analyzed package-unit inputs.
///
/// # Errors
///
/// Returns [`NucleumPackageError`] when a fragment is signature-only or the
/// combined call graph is cyclic.
pub fn nucleum_metadata_from_unit_inputs(
    modules: &[PackageUnitInput<'_>],
) -> Result<NucleumProgramMetadata, NucleumPackageError> {
    let mut metadata = NucleumProgramMetadata::new();
    for input in modules {
        metadata.merge(collect_nucleum_metadata(input.analysis, &input.relative_path)?);
    }
    metadata.require_importable_fragment_bodies()?;
    metadata.require_acyclic()?;
    Ok(metadata)
}

/// Collect nucleum metadata from loaded package modules.
///
/// # Errors
///
/// Returns [`NucleumPackageError`] when a fragment is signature-only or the
/// combined call graph is cyclic.
pub fn nucleum_metadata_from_loaded_modules(
    modules: &[LoadedHirModule],
) -> Result<NucleumProgramMetadata, NucleumPackageError> {
    let mut metadata = NucleumProgramMetadata::new();
    for module in modules {
        metadata.merge(collect_nucleum_metadata(&module.unit, &module.relative_path)?);
    }
    metadata.require_importable_fragment_bodies()?;
    metadata.require_acyclic()?;
    Ok(metadata)
}

/// Collect nucleum role/body metadata from one analyzed unit.
///
/// # Errors
///
/// Returns [`NucleumPackageError::SignatureOnlyImport`] for a fragment without
/// a HIR body or MIR artifact.
pub fn collect_nucleum_metadata(
    analysis: &AnalyzedModule,
    relative_path: &str,
) -> Result<NucleumProgramMetadata, NucleumPackageError> {
    let index = collect_nucleum_index(analysis);
    let effects = crate::semantic::passes::fragment_definition::check(
        &analysis.hir,
        &analysis.resolver,
        &analysis.types,
        &analysis.interner,
    )
    .summaries;
    let mut metadata = NucleumProgramMetadata::new();
    for item in &analysis.hir.items {
        match &item.kind {
            HirItemKind::Function(function) => {
                if let Some(record) =
                    nucleum_item_from_function(item.def_id, function, relative_path, analysis, &index, &effects)?
                {
                    metadata.items.push(record);
                }
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    if let Some(record) = nucleum_item_from_function(
                        method.def_id,
                        &method.func,
                        relative_path,
                        analysis,
                        &index,
                        &effects,
                    )? {
                        metadata.items.push(record);
                    }
                }
            }
            _ => {}
        }
    }
    metadata
        .items
        .sort_by(|left, right| left.name.cmp(&right.name));
    metadata.require_importable_fragment_bodies()?;
    metadata.require_acyclic()?;
    Ok(metadata)
}

fn nucleum_item_from_function(
    def_id: DefId,
    function: &HirFunction,
    relative_path: &str,
    analysis: &AnalyzedModule,
    index: &NucleumIndex,
    effects: &FragmentEffectTable,
) -> Result<Option<NucleumItemMetadata>, NucleumPackageError> {
    let Some(role) = function.nucleum_role() else {
        return Ok(None);
    };
    let name = analysis.interner.resolve(function.name).to_owned();
    let has_hir_body = function.body.is_some();
    let body = (role == NucleumRole::Fragment).then(|| {
        fragment_body_from_hir(
            function,
            analysis,
            effects
                .get(def_id)
                .map(|summary| summary.effects.as_slice()),
        )
    });
    let fragment_callees = function
        .body
        .as_ref()
        .map(|block| collect_fragment_callees(block, index))
        .unwrap_or_default();
    Ok(Some(NucleumItemMetadata {
        name,
        relative_path: relative_path.to_owned(),
        def_id: def_id.0,
        role,
        has_hir_body,
        body,
        fragment_callees,
    }))
}

fn fragment_body_from_hir(
    function: &HirFunction,
    analysis: &AnalyzedModule,
    effects: Option<&[crate::semantic::passes::fragment_definition::DeviceEffect]>,
) -> FragmentBodyArtifact {
    let mut type_constraints = Vec::new();
    let mut shape_constraints = Vec::new();
    for param in &function.type_params {
        let name = analysis.interner.resolve(param.name).to_owned();
        let binding = match &param.constraint {
            HirTypeParamConstraint::Any => "any".to_owned(),
            HirTypeParamConstraint::OneOf(types) => types
                .iter()
                .map(|ty| format_constraint_type(*ty, analysis))
                .collect::<Vec<_>>()
                .join("|"),
        };
        if param.is_typus() {
            type_constraints.push(FragmentConstraint { param: name, binding });
        } else {
            shape_constraints.push(FragmentConstraint { param: name, binding });
        }
    }
    for param in &function.params {
        type_constraints.push(FragmentConstraint {
            param: analysis.interner.resolve(param.name).to_owned(),
            binding: format_constraint_type(param.ty, analysis),
        });
    }
    let effect_summary = effects
        .unwrap_or(&[])
        .iter()
        .map(|effect| FragmentEffectRecord { kind: effect_kind_name(effect.kind).to_owned() })
        .collect::<Vec<_>>();
    let mut fingerprint = Vec::new();
    push_field(&mut fingerprint, "name", analysis.interner.resolve(function.name).as_bytes());
    for constraint in &type_constraints {
        push_field(
            &mut fingerprint,
            "type",
            format!("{}={}", constraint.param, constraint.binding).as_bytes(),
        );
    }
    for constraint in &shape_constraints {
        push_field(
            &mut fingerprint,
            "shape",
            format!("{}={}", constraint.param, constraint.binding).as_bytes(),
        );
    }
    for effect in &effect_summary {
        push_field(&mut fingerprint, "effect", effect.kind.as_bytes());
    }
    FragmentBodyArtifact {
        mir: Vec::new(),
        type_constraints,
        shape_constraints,
        effect_summary,
        compiler_version: NUCLEUM_COMPILER_VERSION.to_owned(),
        abi_version: NUCLEUM_FRAGMENT_ABI_VERSION,
        body_hash: content_hash(&fingerprint),
    }
}

fn format_constraint_type(ty: crate::semantic::TypeId, analysis: &AnalyzedModule) -> String {
    format_faber_type(ty, &analysis.types, &analysis.interner, &|def_id| {
        analysis.resolver.get_symbol(def_id).map_or_else(
            || format!("def{}", def_id.0),
            |symbol| analysis.interner.resolve(symbol.name).to_owned(),
        )
    })
}

fn effect_kind_name(kind: DeviceEffectKind) -> &'static str {
    match kind {
        DeviceEffectKind::ArgumentRead => "argument_read",
        DeviceEffectKind::ArgumentWrite => "argument_write",
        DeviceEffectKind::FragmentCall => "fragment_call",
        DeviceEffectKind::DeviceIntrinsic => "device_intrinsic",
    }
}

struct NucleumIndex {
    roles: FxHashMap<DefId, NucleumRole>,
    names: FxHashMap<DefId, String>,
}

fn collect_nucleum_index(analysis: &AnalyzedModule) -> NucleumIndex {
    let mut index = NucleumIndex { roles: FxHashMap::default(), names: FxHashMap::default() };
    for item in &analysis.hir.items {
        match &item.kind {
            HirItemKind::Function(function) => {
                index
                    .names
                    .insert(item.def_id, analysis.interner.resolve(function.name).to_owned());
                if let Some(role) = function.nucleum_role() {
                    index.roles.insert(item.def_id, role);
                }
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    index
                        .names
                        .insert(method.def_id, analysis.interner.resolve(method.func.name).to_owned());
                    if let Some(role) = method.func.nucleum_role() {
                        index.roles.insert(method.def_id, role);
                    }
                }
            }
            _ => {}
        }
    }
    index
}

fn collect_fragment_callees(block: &HirBlock, index: &NucleumIndex) -> Vec<String> {
    let mut names = Vec::new();
    walk_block_callees(block, index, &mut names);
    names.sort();
    names.dedup();
    names
}

fn walk_block_callees(block: &HirBlock, index: &NucleumIndex, names: &mut Vec<String>) {
    for stmt in &block.statements {
        match &stmt.kind {
            HirStatementKind::Local(local) => {
                if let Some(init) = &local.init {
                    walk_expr_callees(init, index, names);
                }
            }
            HirStatementKind::Redde(Some(expr)) | HirStatementKind::Expr(expr) => {
                walk_expr_callees(expr, index, names);
            }
            HirStatementKind::Custodi(custodi) => {
                for clause in &custodi.clauses {
                    walk_expr_callees(&clause.cond, index, names);
                    walk_block_callees(&clause.body, index, names);
                }
            }
            HirStatementKind::IncDec(inc) => walk_expr_callees(&inc.target, index, names),
            HirStatementKind::Redde(None)
            | HirStatementKind::Rumpe
            | HirStatementKind::Perge
            | HirStatementKind::Tacet => {}
        }
    }
    if let Some(expr) = &block.expr {
        walk_expr_callees(expr, index, names);
    }
}

fn walk_expr_callees(expr: &HirExpression, index: &NucleumIndex, names: &mut Vec<String>) {
    match &expr.kind {
        HirExpressionKind::Call(callee, _, args) => {
            if let HirExpressionKind::Path(def_id) = &callee.kind {
                if index.roles.get(def_id) == Some(&NucleumRole::Fragment) {
                    if let Some(name) = index.names.get(def_id) {
                        names.push(name.clone());
                    }
                }
            }
            walk_expr_callees(callee, index, names);
            for arg in args {
                walk_expr_callees(&arg.expr, index, names);
            }
        }
        HirExpressionKind::MethodCall(receiver, _, _, args) => {
            walk_expr_callees(receiver, index, names);
            for arg in args {
                walk_expr_callees(&arg.expr, index, names);
            }
        }
        // A transfer invokes its sink once per payload (EOT-3M/MIR contract),
        // so a fragment sink is a called fragment callee per payload — never
        // silently dropped by the catch-all.
        HirExpressionKind::Transfer { sink, payloads } => {
            let sink_fragment_def = match &sink.kind {
                HirExpressionKind::Path(def_id) if index.roles.get(def_id) == Some(&NucleumRole::Fragment) => {
                    Some(*def_id)
                }
                _ => None,
            };
            for payload in payloads {
                if let Some(def_id) = sink_fragment_def {
                    if let Some(name) = index.names.get(&def_id) {
                        names.push(name.clone());
                    }
                }
                walk_expr_callees(payload, index, names);
            }
            walk_expr_callees(sink, index, names);
        }
        HirExpressionKind::Binary(_, lhs, rhs)
        | HirExpressionKind::Assign(lhs, rhs)
        | HirExpressionKind::Index(lhs, rhs) => {
            walk_expr_callees(lhs, index, names);
            walk_expr_callees(rhs, index, names);
        }
        HirExpressionKind::Unary(_, operand)
        | HirExpressionKind::Field(operand, _)
        | HirExpressionKind::OptionalChain(operand, _)
        | HirExpressionKind::NonNull(operand, _)
        | HirExpressionKind::Ref(_, operand)
        | HirExpressionKind::Deref(operand)
        | HirExpressionKind::Praefixum(operand)
        | HirExpressionKind::Cede(operand)
        | HirExpressionKind::Yield(operand)
        | HirExpressionKind::Reddet(operand)
        | HirExpressionKind::Tacebit(operand)
        | HirExpressionKind::Throw(operand)
        | HirExpressionKind::Panic(operand) => walk_expr_callees(operand, index, names),
        HirExpressionKind::Block(block) | HirExpressionKind::Loop(block) => walk_block_callees(block, index, names),
        HirExpressionKind::Si { cond, then_block, else_block, .. } => {
            walk_expr_callees(cond, index, names);
            walk_block_callees(then_block, index, names);
            if let Some(else_block) = else_block {
                walk_block_callees(else_block, index, names);
            }
        }
        HirExpressionKind::Dum(cond, block) => {
            walk_expr_callees(cond, index, names);
            walk_block_callees(block, index, names);
        }
        _ => {}
    }
}

fn fragment_cycle(metadata: &NucleumProgramMetadata) -> Option<String> {
    let mut graph: FxHashMap<&str, Vec<&str>> = FxHashMap::default();
    for item in &metadata.items {
        if item.role != NucleumRole::Fragment {
            continue;
        }
        graph
            .entry(item.name.as_str())
            .or_default()
            .extend(item.fragment_callees.iter().map(String::as_str));
    }
    let mut visiting = rustc_hash::FxHashSet::default();
    let mut visited = rustc_hash::FxHashSet::default();
    let mut stack = Vec::new();
    let nodes = graph.keys().copied().collect::<Vec<_>>();
    for start in nodes {
        if let Some(cycle) = dfs_cycle(start, &graph, &mut visiting, &mut visited, &mut stack) {
            return Some(cycle);
        }
    }
    None
}

fn dfs_cycle<'a>(
    node: &'a str,
    graph: &FxHashMap<&'a str, Vec<&'a str>>,
    visiting: &mut rustc_hash::FxHashSet<&'a str>,
    visited: &mut rustc_hash::FxHashSet<&'a str>,
    stack: &mut Vec<&'a str>,
) -> Option<String> {
    if visited.contains(node) {
        return None;
    }
    if !visiting.insert(node) {
        let start = stack.iter().position(|item| *item == node).unwrap_or(0);
        let mut cycle = stack[start..].to_vec();
        cycle.push(node);
        return Some(cycle.join(" → "));
    }
    stack.push(node);
    if let Some(edges) = graph.get(node) {
        for next in edges {
            if let Some(cycle) = dfs_cycle(next, graph, visiting, visited, stack) {
                return Some(cycle);
            }
        }
    }
    stack.pop();
    visiting.remove(node);
    visited.insert(node);
    None
}

fn push_field(canonical: &mut Vec<u8>, name: &str, value: &[u8]) {
    canonical.extend_from_slice(name.as_bytes());
    canonical.push(0);
    let len = u32::try_from(value.len()).unwrap_or(u32::MAX);
    canonical.extend_from_slice(&len.to_le_bytes());
    canonical.extend_from_slice(value);
}

fn content_hash(bytes: &[u8]) -> String {
    format!("sha256:{}", handroll::sha256_hex(bytes))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "package_test.rs"]
mod tests;
