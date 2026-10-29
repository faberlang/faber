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
use crate::file_interface::{
    InterfaceBodyChannel, InterfaceCanonicalBody, InterfaceDeviceContract, InterfaceDeviceRole,
    InterfaceDeviceVisibility,
};
use crate::hir::{
    DefId, HirBlock, HirExpression, HirExpressionKind, HirFunction, HirItemKind, HirStatementKind,
    HirTypeParamConstraint, NucleumRole,
};
use crate::semantic::format_faber_type;
use crate::semantic::passes::fragment_definition::{DeviceEffectKind, FragmentEffectTable};
use handroll::FxHashMap;
use radix_hir_fhir::{FhirPackageModule, SCHEMA_VERSION};
use serde::{Deserialize, Serialize};

pub use radix_hir_fhir::{
    DecodedFhirPackage, FhirPackage, FhirPackageError, LEGACY_PACKAGE_SCHEMA_VERSION, LibraryImportWire, LocalLinkWire,
    PACKAGE_SCHEMA_VERSION, PackageDependencyWire, PackageIdentityWire, decode_package, decode_package_msgpack,
    encode_package, encode_package_msgpack,
};

/// Compiler version recorded on device body artifacts (radix-module crate).
pub const NUCLEUM_COMPILER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// ABI version recorded on device body artifacts and cache keys.
///
/// Matches [`crate::mir::HOST_CALL_ABI_VERSION`] when `mir-core` is on; kept as
/// a package-crate constant so FHIR-only builds can still serialize metadata.
/// DFV2-5 adds artifact fields without touching the body ABI this version
/// describes — no ABI bump.
pub const NUCLEUM_DEVICE_ABI_VERSION: u32 = 8;

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
/// DFV2-5 v2 contract: every device function (`@ nucleum`, either role) gets
/// a body artifact carrying its constraints/effect/toolchain identity, and
/// only `@ nucleum @ publica` items enter
/// [`NucleumProgramMetadata::dispatch_and_reflection_names`] — visibility is
/// the launch surface, the role is not. Importable items without any body
/// fail closed, and the concrete device call graph must be acyclic.
///
/// # Errors
///
/// Returns [`FhirPackageError`] when the envelope is invalid or an importable
/// device function is signature-only / the device call graph is cyclic.
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

/// Load a FHIR package from `.fhirpkg` bytes.
///
/// FPF-5 switch: the live wire is the `MessagePack` envelope (package schema
/// v2); the format crate's [`decode_package_msgpack`] runs the legacy
/// postcard rejection (named legacy error, no fallback decode), the package
/// version gate, the per-unit gates (unit schema version + F3 + U2.3
/// referential integrity + source-hash consistency), and the package-level
/// module/dependency checks. This function then re-enters the unit codec for
/// each module's unit bytes and reconstructs arenas via
/// [`crate::hir::artifact::reconstruct_unit`] (U2.2), the same path the
/// single-unit loader uses. Never panics on the load path (F7) and never
/// invokes frontend phases.
///
/// # Errors
///
/// Returns [`FhirPackageError`] when package decoding, validation, or unit
/// reconstruction fails — including
/// [`FhirPackageError::LegacyPostcardPackage`] for retired v1 postcard
/// envelopes.
pub fn load_package(bytes: &[u8]) -> Result<LoadedHirPackage, FhirPackageError> {
    let pkg = decode_package_msgpack(bytes).map_err(FhirPackageError::from)?;
    let mut modules = Vec::with_capacity(pkg.modules.len());
    for module in pkg.modules {
        let artifact = radix_hir_fhir::decode(&module.unit).map_err(|error| FhirPackageError::MalformedUnit {
            module: module.path.clone(),
            detail: error.to_string(),
        })?;
        let unit = crate::hir::artifact::reconstruct_unit(artifact).map_err(|error| {
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
        identity: pkg.identity,
        entry_path: pkg.entry_path,
        entry_frontmatter: pkg.entry_frontmatter,
        modules,
        dependencies: pkg.dependencies,
        nucleum,
    })
}

// ---------------------------------------------------------------------------
// Nucleum device package metadata (NF-8; v2 device bodies DFV2-5)
// ---------------------------------------------------------------------------

/// Structured failure for nucleum package metadata / import checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NucleumPackageError {
    /// An importable device function has no body at all (signature-only).
    SignatureOnlyImport { name: String, module: String },
    /// Concrete device call graph contains a cycle after import resolution.
    CyclicCallGraph { cycle: String },
    /// A device function is importable but has no canonical MIR body bytes.
    MirBodyMissing { name: String, module: String },
    /// The recorded body hash does not match the serialized body bytes.
    BodyHashMismatch {
        name: String,
        module: String,
        expected: String,
        found: String,
    },
    /// The artifact was produced by another compiler/ABI generation.
    IncompatibleBodyVersion {
        name: String,
        module: String,
        compiler_version: String,
        abi_version: u32,
    },
}

impl NucleumPackageError {
    fn into_fhir(self) -> FhirPackageError {
        match self {
            Self::SignatureOnlyImport { name, module } => FhirPackageError::MalformedUnit {
                module,
                detail: format!("signature-only nucleum device function `{name}` cannot be imported"),
            },
            Self::CyclicCallGraph { cycle } => FhirPackageError::MalformedUnit {
                module: String::new(),
                detail: format!("nucleum device call cycle: {cycle}"),
            },
            Self::MirBodyMissing { name, module } => FhirPackageError::MalformedUnit {
                module,
                detail: format!("nucleum device function `{name}` is missing a canonical MIR body artifact"),
            },
            Self::BodyHashMismatch { name, module, expected, found } => FhirPackageError::MalformedUnit {
                module,
                detail: format!("nucleum device body `{name}` hash mismatch (recorded {expected}, computed {found})"),
            },
            Self::IncompatibleBodyVersion { name, module, compiler_version, abi_version } => {
                FhirPackageError::MalformedUnit {
                    module,
                    detail: format!(
                        "nucleum device body `{name}` was built by compiler {compiler_version} / ABI \
                         v{abi_version} (this compiler is {NUCLEUM_COMPILER_VERSION} / ABI \
                         v{NUCLEUM_DEVICE_ABI_VERSION})"
                    ),
                }
            }
        }
    }

    /// Diagnostic issue identifier (`nucleum_fragment_effect` / `_cycle`).
    #[must_use]
    pub fn issue(&self) -> &'static str {
        match self {
            Self::SignatureOnlyImport { .. }
            | Self::MirBodyMissing { .. }
            | Self::BodyHashMismatch { .. }
            | Self::IncompatibleBodyVersion { .. } => "nucleum_fragment_effect",
            Self::CyclicCallGraph { .. } => "nucleum_fragment_cycle",
        }
    }

    /// Catalog `reason` argument.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::SignatureOnlyImport { .. } | Self::MirBodyMissing { .. } => "missing_body",
            Self::BodyHashMismatch { .. } => "body_hash_mismatch",
            Self::IncompatibleBodyVersion { .. } => "stale_body_version",
            Self::CyclicCallGraph { .. } => "cycle",
        }
    }
}

impl std::fmt::Display for NucleumPackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SignatureOnlyImport { name, module } => {
                write!(
                    f,
                    "signature-only nucleum device function `{name}` in `{module}` cannot be imported"
                )
            }
            Self::CyclicCallGraph { cycle } => write!(f, "nucleum device call cycle: {cycle}"),
            Self::MirBodyMissing { name, module } => {
                write!(
                    f,
                    "nucleum device function `{name}` in `{module}` is missing a canonical MIR body"
                )
            }
            Self::BodyHashMismatch { name, module, expected, found } => write!(
                f,
                "nucleum device body `{name}` in `{module}` hash mismatch (recorded {expected}, computed {found})"
            ),
            Self::IncompatibleBodyVersion { name, module, compiler_version, abi_version } => write!(
                f,
                "nucleum device body `{name}` in `{module}` was built by compiler {compiler_version} / ABI \
                 v{abi_version} (this compiler is {NUCLEUM_COMPILER_VERSION} / ABI v{NUCLEUM_DEVICE_ABI_VERSION})"
            ),
        }
    }
}

impl std::error::Error for NucleumPackageError {}

/// Launch visibility recorded with each nucleum item (v2 DFV2-1/DFV2-5).
///
/// Visibility is the launch surface: `@ nucleum @ publica` is the only
/// dispatch/reflection entry; an unmarked `@ nucleum` function is a
/// module-internal composable helper. Mirrors the interface transport's
/// [`InterfaceDeviceVisibility`]; kept as a package-local serde type so the
/// postcard sidecar stays self-contained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NucleumVisibility {
    /// `@ nucleum @ publica` — dispatch/reflection entry in any consumer.
    Public,
    /// Unmarked `@ nucleum` — composable helper, never a launch entry.
    Private,
}

/// The `@ publica` annotation family: the v2 launch-surface gate.
///
/// Mirrors `radix-mir`'s `is_public_entry` (keyword and reader-locale
/// spellings lower to the same canonical family); duplicated here because
/// package metadata must stay constructible in FHIR-only builds without the
/// MIR leaf.
const PUBLICA_FAMILY: &str = "publica";

/// Launch visibility of one HIR device function.
fn nucleum_visibility(
    annotations: &[radix_hir::HirAnnotation],
    interner: &crate::lexer::Interner,
) -> NucleumVisibility {
    let public = annotations
        .iter()
        .any(|annotation| interner.resolve(annotation.family) == PUBLICA_FAMILY);
    if public {
        NucleumVisibility::Public
    } else {
        NucleumVisibility::Private
    }
}

/// One type or shape constraint on a device body artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceBodyConstraint {
    pub param: String,
    pub binding: String,
}

/// One visible device effect recorded on a device body artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceEffectRecord {
    pub kind: String,
}

/// Canonical specialized-MIR body artifact for a device function (DFV2-5).
///
/// Carries the identity contract of design §9: canonical body bytes, the
/// type/shape constraints the body was checked under, the device-effect
/// summary, the producing compiler/ABI versions, and the body hash over the
/// serialized bytes. `mir`/`body_hash` are empty until
/// [`NucleumProgramMetadata::attach_canonical_mir`] supplies the lowered
/// body — a pre-attach artifact is build state, never a durable import
/// subject, and [`NucleumItemMetadata::canonical_template`] rejects it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceBodyArtifact {
    /// Postcard-encoded [`crate::mir::MirFunction`] for the specialized body.
    pub mir: Vec<u8>,
    pub type_constraints: Vec<DeviceBodyConstraint>,
    pub shape_constraints: Vec<DeviceBodyConstraint>,
    pub effect_summary: Vec<DeviceEffectRecord>,
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
    /// Launch visibility (`@ publica` gate) — the v2 launch surface.
    pub visibility: NucleumVisibility,
    /// HIR body present in the exporting unit.
    pub has_hir_body: bool,
    /// Canonical MIR body artifact; every device function carries one, and
    /// importable items must have usable bytes (fail closed otherwise).
    pub body: Option<DeviceBodyArtifact>,
    /// Direct device callees (names) for the post-import call graph.
    pub device_callees: Vec<String>,
}

impl NucleumItemMetadata {
    /// Whether this item is a launchable device entry (`@ nucleum @ publica`).
    ///
    /// The v2 dispatch/reflection gate: role `Entry` alone is not enough —
    /// a private `@ nucleum` function is a composable helper with no launch
    /// ABI, and a `Fragment` is never a dispatch stage.
    #[must_use]
    pub fn is_launch_entry(&self) -> bool {
        self.role == NucleumRole::Entry && self.visibility == NucleumVisibility::Public
    }

    /// The canonical body template bytes, verified against the artifact's
    /// recorded hash and toolchain identity (the DFV2-5 import gate).
    ///
    /// # Errors
    ///
    /// Returns [`NucleumPackageError::MirBodyMissing`] when the artifact or
    /// its MIR bytes are absent, [`NucleumPackageError::BodyHashMismatch`]
    /// when the recorded hash does not match the serialized bytes, and
    /// [`NucleumPackageError::IncompatibleBodyVersion`] when the artifact was
    /// produced by another compiler/ABI generation.
    pub fn canonical_template(&self) -> Result<Vec<u8>, NucleumPackageError> {
        let Some(body) = &self.body else {
            return Err(NucleumPackageError::MirBodyMissing {
                name: self.name.clone(),
                module: self.relative_path.clone(),
            });
        };
        if body.mir.is_empty() {
            return Err(NucleumPackageError::MirBodyMissing {
                name: self.name.clone(),
                module: self.relative_path.clone(),
            });
        }
        let computed = content_hash(&body.mir);
        if body.body_hash != computed {
            return Err(NucleumPackageError::BodyHashMismatch {
                name: self.name.clone(),
                module: self.relative_path.clone(),
                expected: body.body_hash.clone(),
                found: computed,
            });
        }
        if body.compiler_version != NUCLEUM_COMPILER_VERSION || body.abi_version != NUCLEUM_DEVICE_ABI_VERSION {
            return Err(NucleumPackageError::IncompatibleBodyVersion {
                name: self.name.clone(),
                module: self.relative_path.clone(),
                compiler_version: body.compiler_version.clone(),
                abi_version: body.abi_version,
            });
        }
        Ok(body.mir.clone())
    }

    /// The v2 device contract for this item on the DFV2-3 import transport.
    ///
    /// Produces exactly what [`crate::file_interface::InterfaceCallable`]'s
    /// `device` channel carries: role, launch visibility, and the body
    /// channel. The body is [`InterfaceBodyChannel::Canonical`] only when the
    /// artifact passes [`Self::canonical_template`]; any missing, malformed,
    /// or stale body degrades to [`InterfaceBodyChannel::Missing`] — the
    /// transport's single fail-closed state — never to a signature-only
    /// callable.
    #[must_use]
    pub fn device_contract(&self) -> InterfaceDeviceContract {
        let role = match self.role {
            NucleumRole::Entry => InterfaceDeviceRole::Kernel,
            NucleumRole::Fragment => InterfaceDeviceRole::Fragment,
        };
        let visibility = match self.visibility {
            NucleumVisibility::Public => InterfaceDeviceVisibility::Public,
            NucleumVisibility::Private => InterfaceDeviceVisibility::Private,
        };
        let body = match self.canonical_template() {
            Ok(template) => InterfaceBodyChannel::Canonical(InterfaceCanonicalBody::new(template)),
            Err(_) => InterfaceBodyChannel::Missing,
        };
        InterfaceDeviceContract { role, visibility, body }
    }
}

/// Serialized program metadata for nucleum roles, launch visibility, and
/// canonical device bodies.
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
            abi_version: NUCLEUM_DEVICE_ABI_VERSION,
            items: Vec::new(),
        }
    }

    /// Names that enter device dispatch and reflection tables.
    ///
    /// DFV2-5 visibility gate: only public `@ nucleum` entries are launch
    /// rows. Private helpers and `Fragment` composition sugar never enter
    /// dispatch/reflection metadata.
    #[must_use]
    pub fn dispatch_and_reflection_names(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|item| item.is_launch_entry())
            .map(|item| item.name.clone())
            .collect()
    }

    /// Device names omitted from target symbol tables after composition.
    ///
    /// Everything that is not a public entry is composed into its callers
    /// (DFV2-4) and must not survive as a standalone target symbol: private
    /// compute helpers and every `Fragment` body.
    #[must_use]
    pub fn omitted_target_symbol_names(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|item| !item.is_launch_entry())
            .map(|item| item.name.clone())
            .collect()
    }

    /// Merge `other` into this metadata (import resolution).
    pub fn merge(&mut self, other: Self) {
        self.items.extend(other.items);
        self.items
            .sort_by(|left, right| (&left.relative_path, &left.name).cmp(&(&right.relative_path, &right.name)));
    }

    /// Fail closed when an importable device function is signature-only.
    ///
    /// Importable surface (v2): every `Fragment` (composition-only bodies,
    /// any visibility) plus every public device function (consumer-nameable
    /// exports). A private compute helper stays module-internal — its body
    /// is enforced at composition, not here.
    ///
    /// # Errors
    ///
    /// Returns [`NucleumPackageError::SignatureOnlyImport`] when an
    /// importable item has no HIR body and no MIR artifact.
    pub fn require_importable_fragment_bodies(&self) -> Result<(), NucleumPackageError> {
        for item in &self.items {
            if !is_importable_device_item(item) {
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
    /// Fragment-scoped subset (the production attach flow lowers
    /// fragment-bearing units); the full v2 import gate over every
    /// importable device body is
    /// [`Self::verify_importable_device_bodies`].
    ///
    /// # Errors
    ///
    /// Returns [`NucleumPackageError::MirBodyMissing`] when a fragment
    /// artifact is missing postcard MIR.
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

    /// Import-side identity gate (DFV2-5): every importable device body must
    /// be a complete, untampered, version-compatible canonical artifact.
    ///
    /// # Errors
    ///
    /// Returns [`NucleumPackageError::MirBodyMissing`] for absent body bytes,
    /// [`NucleumPackageError::BodyHashMismatch`] when the recorded hash does
    /// not match the serialized bytes, and
    /// [`NucleumPackageError::IncompatibleBodyVersion`] for artifacts from
    /// another compiler/ABI generation. No failure degrades to a
    /// signature-only import.
    pub fn verify_importable_device_bodies(&self) -> Result<(), NucleumPackageError> {
        for item in &self.items {
            if !is_importable_device_item(item) {
                continue;
            }
            item.canonical_template()?;
        }
        Ok(())
    }

    /// Fail closed when the concrete device call graph has a cycle.
    ///
    /// # Errors
    ///
    /// Returns [`NucleumPackageError::CyclicCallGraph`] with the cycle path.
    pub fn require_acyclic(&self) -> Result<(), NucleumPackageError> {
        if let Some(cycle) = device_call_cycle(self) {
            return Err(NucleumPackageError::CyclicCallGraph { cycle });
        }
        Ok(())
    }

    /// Attach postcard MIR bodies from a lowered unit (canonical device MIR).
    ///
    /// DFV2-5: attaches for every device item of `relative_path` that is
    /// present in the lowered program — entries, helpers, and fragments
    /// alike — recomputing the body hash over the serialized bytes. Bodies
    /// the lowering dropped (e.g. a private helper composed away with no
    /// remaining reference) keep their pre-attach empty artifact and fail
    /// the import gate rather than serializing a hollow body.
    #[cfg(feature = "mir-core")]
    pub fn attach_canonical_mir(&mut self, program: &crate::mir::MirProgram, relative_path: &str) {
        for item in &mut self.items {
            if item.relative_path != relative_path {
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

/// Whether a nucleum item is on the cross-package import surface.
///
/// `Fragment` items are importable at any visibility (they exist only to be
/// composed), and a public device function is a consumer-nameable export.
/// Private non-fragment device functions are module-internal helpers.
fn is_importable_device_item(item: &NucleumItemMetadata) -> bool {
    item.role == NucleumRole::Fragment || item.visibility == NucleumVisibility::Public
}

/// Deterministic cache key for one specialized device body (design §9).
///
/// Identity covers the body hash, compiler/ABI version, target capabilities,
/// and the concrete type/shape bindings. Meaningful for attached artifacts
/// (non-empty body hash); pre-attach artifacts carry an empty hash and are
/// build state, not cache subjects.
#[must_use]
pub fn device_body_cache_key(
    artifact: &DeviceBodyArtifact,
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
/// Returns [`NucleumPackageError`] when an importable device function is
/// signature-only or the combined device call graph is cyclic.
pub fn nucleum_metadata_from_unit_inputs(
    modules: &[PackageUnitInput<'_>],
) -> Result<NucleumProgramMetadata, NucleumPackageError> {
    merge_nucleum_units(
        modules
            .iter()
            .map(|input| (input.analysis, input.relative_path.as_str())),
    )
}

/// Collect nucleum metadata from loaded package modules.
///
/// # Errors
///
/// Returns [`NucleumPackageError`] when an importable device function is
/// signature-only or the combined device call graph is cyclic.
pub fn nucleum_metadata_from_loaded_modules(
    modules: &[LoadedHirModule],
) -> Result<NucleumProgramMetadata, NucleumPackageError> {
    merge_nucleum_units(
        modules
            .iter()
            .map(|module| (&module.unit, module.relative_path.as_str())),
    )
}

/// Merge per-unit metadata over analyzed units and run the import gates.
///
/// Both package entry points (build-side unit inputs, load-side modules)
/// share this pass; only the source of each `(analysis, relative_path)`
/// pair differs.
///
/// # Errors
///
/// Returns [`NucleumPackageError`] when a unit's metadata cannot be
/// collected, an importable device function is signature-only, or the
/// combined device call graph is cyclic.
fn merge_nucleum_units<'a>(
    units: impl Iterator<Item = (&'a AnalyzedModule, &'a str)>,
) -> Result<NucleumProgramMetadata, NucleumPackageError> {
    let mut metadata = NucleumProgramMetadata::new();
    for (analysis, relative_path) in units {
        metadata.merge(collect_nucleum_metadata(analysis, relative_path)?);
    }
    metadata.require_importable_fragment_bodies()?;
    metadata.require_acyclic()?;
    Ok(metadata)
}

/// Collect nucleum role/body metadata from one analyzed unit.
///
/// Every device function is recorded with its launch visibility and a body
/// artifact skeleton; dispatch/reflection filtering happens at read time
/// ([`NucleumProgramMetadata::dispatch_and_reflection_names`]).
///
/// # Errors
///
/// Returns [`NucleumPackageError::SignatureOnlyImport`] for an importable
/// device function without a HIR body or MIR artifact.
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
                if let Some(record) = nucleum_item_from_function(
                    item.def_id,
                    function,
                    &item.annotations,
                    relative_path,
                    analysis,
                    &index,
                    &effects,
                )? {
                    metadata.items.push(record);
                }
            }
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    if let Some(record) = nucleum_item_from_function(
                        method.def_id,
                        &method.func,
                        &method.annotations,
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
    annotations: &[radix_hir::HirAnnotation],
    relative_path: &str,
    analysis: &AnalyzedModule,
    index: &NucleumIndex,
    effects: &FragmentEffectTable,
) -> Result<Option<NucleumItemMetadata>, NucleumPackageError> {
    let Some(role) = function.nucleum_role() else {
        return Ok(None);
    };
    let name = analysis.interner.resolve(function.name).to_owned();
    let visibility = nucleum_visibility(annotations, &analysis.interner);
    let has_hir_body = function.body.is_some();
    // DFV2-5: every device function carries the v2 body artifact skeleton —
    // constraints, effect summary, and toolchain identity travel with both
    // roles; canonical bytes attach from the lowered unit.
    let body = device_body_from_hir(
        function,
        analysis,
        effects
            .get(def_id)
            .map(|summary| summary.effects.as_slice()),
    );
    let device_callees = function
        .body
        .as_ref()
        .map(|block| collect_device_callees(block, index))
        .unwrap_or_default();
    Ok(Some(NucleumItemMetadata {
        name,
        relative_path: relative_path.to_owned(),
        def_id: def_id.0,
        role,
        visibility,
        has_hir_body,
        body: Some(body),
        device_callees,
    }))
}

fn device_body_from_hir(
    function: &HirFunction,
    analysis: &AnalyzedModule,
    effects: Option<&[crate::semantic::passes::fragment_definition::DeviceEffect]>,
) -> DeviceBodyArtifact {
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
            // Device bodies record the contract definitions a bound names.
            HirTypeParamConstraint::Implet(bounds) => bounds
                .iter()
                .map(|bound| format!("implet#{}", bound.contract.0))
                .collect::<Vec<_>>()
                .join("&"),
        };
        if param.is_typus() {
            type_constraints.push(DeviceBodyConstraint { param: name, binding });
        } else {
            shape_constraints.push(DeviceBodyConstraint { param: name, binding });
        }
    }
    for param in &function.params {
        type_constraints.push(DeviceBodyConstraint {
            param: analysis.interner.resolve(param.name).to_owned(),
            binding: format_constraint_type(param.ty, analysis),
        });
    }
    let effect_summary = effects
        .unwrap_or(&[])
        .iter()
        .map(|effect| DeviceEffectRecord { kind: effect_kind_name(effect.kind).to_owned() })
        .collect::<Vec<_>>();
    DeviceBodyArtifact {
        // Pre-attach build state: the canonical MIR bytes and their hash are
        // supplied by `attach_canonical_mir`; the import gate rejects the
        // empty artifact instead of trusting a definition fingerprint.
        mir: Vec::new(),
        type_constraints,
        shape_constraints,
        effect_summary,
        compiler_version: NUCLEUM_COMPILER_VERSION.to_owned(),
        abi_version: NUCLEUM_DEVICE_ABI_VERSION,
        body_hash: String::new(),
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

fn collect_device_callees(block: &HirBlock, index: &NucleumIndex) -> Vec<String> {
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
            HirStatementKind::Redde(Some(expr))
            | HirStatementKind::ReddeFilum(Some(expr))
            | HirStatementKind::Expr(expr) => {
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
            | HirStatementKind::ReddeFilum(None)
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
        HirExpressionKind::Call(callee, _, args, _) => {
            if let HirExpressionKind::Path(def_id) = &callee.kind {
                // v2: every device-to-device call composes (entry or helper),
                // so every device callee is a call-graph edge.
                if index.roles.contains_key(def_id)
                    && let Some(name) = index.names.get(def_id)
                {
                    names.push(name.clone());
                }
            }
            walk_expr_callees(callee, index, names);
            for arg in args {
                walk_expr_callees(&arg.expr, index, names);
            }
        }
        HirExpressionKind::MethodCall(receiver, _, _, args, _) => {
            walk_expr_callees(receiver, index, names);
            for arg in args {
                walk_expr_callees(&arg.expr, index, names);
            }
        }
        HirExpressionKind::CopyInto { target, value } => {
            walk_expr_callees(value, index, names);
            walk_expr_callees(target, index, names);
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

/// One device function in the call graph: `(module path, name)`.
type DeviceNode<'a> = (&'a str, &'a str);

/// Detect a cycle in the concrete device call graph (all device roles).
///
/// A recorded callee is a device function of the caller's own module
/// (`collect_nucleum_index` is built per unit), so its node is the caller's
/// module path plus the callee name. Bare names would merge same-named
/// functions of different modules into one node and report a false cycle.
fn device_call_cycle(metadata: &NucleumProgramMetadata) -> Option<String> {
    let mut graph: FxHashMap<DeviceNode<'_>, Vec<DeviceNode<'_>>> = FxHashMap::default();
    for item in &metadata.items {
        let module = item.relative_path.as_str();
        graph
            .entry((module, item.name.as_str()))
            .or_default()
            .extend(
                item.device_callees
                    .iter()
                    .map(|callee| (module, callee.as_str())),
            );
    }
    let mut visiting = handroll::FxHashSet::default();
    let mut visited = handroll::FxHashSet::default();
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
    node: DeviceNode<'a>,
    graph: &FxHashMap<DeviceNode<'a>, Vec<DeviceNode<'a>>>,
    visiting: &mut handroll::FxHashSet<DeviceNode<'a>>,
    visited: &mut handroll::FxHashSet<DeviceNode<'a>>,
    stack: &mut Vec<DeviceNode<'a>>,
) -> Option<String> {
    if visited.contains(&node) {
        return None;
    }
    if !visiting.insert(node) {
        let start = stack.iter().position(|item| *item == node).unwrap_or(0);
        let mut cycle = stack[start..].to_vec();
        cycle.push(node);
        return Some(
            cycle
                .into_iter()
                .map(|(_, name)| name)
                .collect::<Vec<_>>()
                .join(" → "),
        );
    }
    stack.push(node);
    if let Some(edges) = graph.get(&node) {
        for next in edges {
            if let Some(cycle) = dfs_cycle(*next, graph, visiting, visited, stack) {
                return Some(cycle);
            }
        }
    }
    stack.pop();
    visiting.remove(&node);
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

#[cfg(test)]
#[path = "package_inf_test.rs"]
mod inf_tests;
