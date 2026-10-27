//! Portable FHIR **package** envelope: wire types, codec, and validation.
//!
//! A FHIR package is a deterministic envelope around ordered per-unit
//! [`HirArtifact`] records plus package identity, a module table, entry
//! metadata, dependency coordinates, and local library-import references.
//! Contract: `docs/factory/hir-artifact-fhir/fhir-package-contract.md`.
//!
//! The package envelope is a **separate schema** from the unit artifact:
//! [`PACKAGE_SCHEMA_VERSION`] gates the envelope while each embedded unit
//! keeps its own [`SCHEMA_VERSION`] check (fail closed on both, no
//! compatibility decoders).
//!
//! FPF-5 switch (package schema v1→2): the live `.fhirpkg` wire is the
//! `MessagePack` envelope ([`crate::msgpack::encode_package_msgpack`] /
//! [`crate::msgpack::decode_package_msgpack`], stamped
//! [`PACKAGE_SCHEMA_VERSION`]). The postcard envelope below is the **legacy
//! v1 codec** ([`encode_package`] / [`decode_package`]): it stays pinned to
//! [`LEGACY_PACKAGE_SCHEMA_VERSION`] for migration reads and the equivalence
//! proof only — the product load path never falls back to it, and v1
//! postcard bytes handed to the `MessagePack` decoder fail closed with
//! [`FhirPackageError::LegacyPostcardPackage`].
//!
//! Wire discipline mirrors the unit artifact:
//! - `package_schema_version` is the first wire field (version prefix gate);
//! - modules are sorted by canonical package-root-relative path and unique;
//! - dependencies are sorted by name and unique;
//! - no field carries an absolute checkout path (module paths are
//!   package-root-relative; library imports carry binding/package/module only);
//! - unit-local `DefId`/`HirId`/`TypeId`/`Symbol` are never package-global.
//!
//! [`encode_package`] is a pure serializer; [`decode_package`] runs the
//! version gates, per-unit decode (reusing the unit decode gates), and the
//! package-level checks, and returns the validated, decoded envelope.

use crate::artifact::{HirArtifact, SCHEMA_VERSION};
use crate::decode::decode as decode_unit;
use serde::{Deserialize, Serialize};

/// Schema version of the package envelope itself (distinct from the unit
/// artifact [`SCHEMA_VERSION`]). Ratchets on breaking package-format changes;
/// there is no compatibility decoder.
///
/// FPF-5 ratchet (OQ-1): `2` is the first `MessagePack` envelope — the map
/// value itself is the MessagePack-marked prefix, so retired v1 postcard
/// bytes fail the envelope gate with
/// [`FhirPackageError::LegacyPostcardPackage`]. No magic bytes.
pub const PACKAGE_SCHEMA_VERSION: u32 = 2;

/// Schema version of the retired postcard package envelope (pre-FPF-5
/// artifacts). The legacy postcard codec accepts exactly this version; the
/// live `MessagePack` codec rejects it.
pub const LEGACY_PACKAGE_SCHEMA_VERSION: u32 = 1;

/// File extension for FHIR package artifacts.
pub const FHIR_PACKAGE_EXTENSION: &str = "fhirpkg";

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

/// Package identity: name, version, edition. No build-host metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageIdentityWire {
    pub name: String,
    pub version: String,
    pub edition: String,
}

/// One dependency coordinate. The envelope records exact references only;
/// closure acquisition is the store's job (Cista). A loader that
/// cannot satisfy a record fails closed with `MissingDependencyArtifact`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageDependencyWire {
    pub name: String,
    pub version: String,
    pub lock_identity: String,
    pub checksum: Option<String>,
}

/// Serialized library-import reference (the portable form of faber's
/// `LibraryImportBinding`). Deliberately carries no `interface_path` or any
/// absolute checkout path — only the import binding, provider package name,
/// and module path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryImportWire {
    pub binding: String,
    pub package: String,
    pub module: Vec<String>,
}

/// One local (package-internal) import link: the import binding used in the
/// importing module and the package-root-relative path of the target module.
///
/// This is the explicit link table (delivery contract): the loader never
/// re-derives local imports from the filesystem, and validation requires
/// every target to exist in the module table (`DanglingModuleRef`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalLinkWire {
    pub binding: String,
    pub target: String,
}

/// One module record in the package module table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FhirPackageModule {
    /// Package-root-relative canonical path (e.g. `src/main.fab`).
    pub path: String,
    /// Module segments derived from the path (e.g. `["main"]`).
    pub module_segments: Vec<String>,
    pub is_entry: bool,
    /// Sorted public export names of this module (source-free surface used by
    /// package codegen assembly).
    pub export_names: Vec<String>,
    /// Local import links (binding → target module path), sorted by binding.
    pub local_links: Vec<LocalLinkWire>,
    /// Library imports referenced by this module (binding → package/module).
    pub library_imports: Vec<LibraryImportWire>,
    /// SHA-256 hex of the original source bytes (identical to the unit
    /// artifact's `SourceIdentity.content_hash`; checked on decode).
    pub source_hash: String,
    /// Manifest copy of the embedded unit's schema version for fast fail
    /// with module attribution.
    pub unit_schema_version: u32,
    /// Postcard-encoded [`HirArtifact`] for this unit (reuses the unit codec).
    pub unit: Vec<u8>,
}

/// The FHIR package envelope (wire form).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FhirPackage {
    /// First wire field (version prefix gate).
    pub package_schema_version: u32,
    pub identity: PackageIdentityWire,
    /// Package-root-relative entry path.
    pub entry_path: String,
    /// Entry frontmatter as TOML text (parsed by the caller's `toml` dep;
    /// this format crate stays TOML-free).
    pub entry_frontmatter: Option<String>,
    /// Sorted by `path`, unique.
    pub modules: Vec<FhirPackageModule>,
    /// Sorted by `name`, unique.
    pub dependencies: Vec<PackageDependencyWire>,
}

/// One module record after per-unit decode (units validated and decoded).
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedFhirPackageModule {
    pub path: String,
    pub module_segments: Vec<String>,
    pub is_entry: bool,
    pub export_names: Vec<String>,
    pub local_links: Vec<LocalLinkWire>,
    pub library_imports: Vec<LibraryImportWire>,
    pub unit: HirArtifact,
}

/// The validated, decoded package envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedFhirPackage {
    pub identity: PackageIdentityWire,
    pub entry_path: String,
    pub entry_frontmatter: Option<String>,
    pub modules: Vec<DecodedFhirPackageModule>,
    pub dependencies: Vec<PackageDependencyWire>,
}

// ---------------------------------------------------------------------------
// FhirPackageError
// ---------------------------------------------------------------------------

/// Structured error for FHIR package decode and validation failures.
/// Fail-closed: never panic on the load path.
#[derive(Debug)]
pub enum FhirPackageError {
    /// The binary payload is not valid `postcard`-encoded data.
    Malformed(postcard::Error),
    /// The payload is not a valid `MessagePack` package value (the live wire
    /// since the FPF-5 switch).
    MalformedMsgpack(String),
    /// The payload ends before a complete package could be consumed.
    TruncatedPackage,
    /// The envelope was produced by a package schema version this loader
    /// does not support (clean break — no v0 decoder).
    UnsupportedPackageSchemaVersion { found: u32, expected: u32 },
    /// The payload is a retired postcard-encoded envelope (FPF-5 ratchet:
    /// schema v1 postcard → v2 `MessagePack`). Named legacy rejection: no
    /// fallback decode, no dual writer. Rebuild the package.
    LegacyPostcardPackage {
        /// Legacy package schema version carried by the payload.
        found: u32,
        /// The live `MessagePack` package schema version.
        expected: u32,
    },
    /// An embedded unit declares a schema version this loader rejects.
    UnsupportedUnitSchemaVersion {
        module: String,
        found: u32,
        expected: u32,
    },
    /// An embedded unit was written with a serialized wire shape that differs
    /// from this loader's derived shape hash (a type changed without a
    /// `SCHEMA_VERSION` bump).
    UnsupportedUnitWireShape {
        module: String,
        found: u64,
        expected: u64,
    },
    /// An embedded unit failed its own decode/validation gates.
    MalformedUnit { module: String, detail: String },
    /// The module table contains the same canonical path twice.
    DuplicateModulePath { path: String },
    /// The module table has no module at the declared entry path (or the
    /// entry flag is inconsistent with the entry path).
    MissingEntry { entry: String },
    /// A local reference names a module/export that is not in the package
    /// (produced where resolution context exists — the faber adapter).
    DanglingModuleRef { from: String, to: String },
    /// A dependency record could not be satisfied from the store/cache.
    MissingDependencyArtifact { name: String, version: String },
    /// The declared source hash does not match the unit artifact's own
    /// `SourceIdentity.content_hash`.
    MismatchedContentHash {
        module: String,
        expected: String,
        actual: String,
    },
    /// The module table or dependency list is not in canonical sorted order.
    Unsorted { what: &'static str },
}

impl std::fmt::Display for FhirPackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(inner) => write!(f, "malformed FHIR package: {inner}"),
            Self::MalformedMsgpack(detail) => {
                write!(f, "malformed MessagePack FHIR package: {detail}")
            }
            Self::TruncatedPackage => write!(f, "truncated FHIR package: unexpected end of data"),
            Self::UnsupportedPackageSchemaVersion { found, expected } => write!(
                f,
                "unsupported FHIR package schema version {found} (expected {expected})"
            ),
            Self::LegacyPostcardPackage { found, expected } => write!(
                f,
                "legacy postcard-encoded FHIR package (schema v{found}) is no longer readable; \
                 the live wire is MessagePack package schema v{expected} (ratcheted in FPF-5); \
                 rebuild the package"
            ),
            Self::UnsupportedUnitSchemaVersion {
                module,
                found,
                expected,
            } => write!(
                f,
                "unsupported unit schema version {found} in module `{module}` (expected {expected})"
            ),
            Self::UnsupportedUnitWireShape {
                module,
                found,
                expected,
            } => write!(
                f,
                "unit in module `{module}` has wire shape {found:#018x} but this build reads {expected:#018x} \
                 (a serialized type changed without a schema version bump); rebuild the package"
            ),
            Self::MalformedUnit { module, detail } => {
                write!(f, "malformed unit `{module}` in FHIR package: {detail}")
            }
            Self::DuplicateModulePath { path } => {
                write!(f, "duplicate module path `{path}` in FHIR package")
            }
            Self::MissingEntry { entry } => {
                write!(f, "FHIR package entry `{entry}` is not in the module table")
            }
            Self::DanglingModuleRef { from, to } => write!(
                f,
                "FHIR package module `{from}` references missing module/export `{to}`"
            ),
            Self::MissingDependencyArtifact { name, version } => {
                write!(
                    f,
                    "FHIR package dependency `{name}@{version}` is not available"
                )
            }
            Self::MismatchedContentHash {
                module,
                expected,
                actual,
            } => write!(
                f,
                "FHIR package module `{module}` source hash mismatch (expected {expected}, artifact has {actual})"
            ),
            Self::Unsorted { what } => {
                write!(f, "FHIR package {what} is not in canonical sorted order")
            }
        }
    }
}

impl std::error::Error for FhirPackageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Malformed(inner) => Some(inner),
            _ => None,
        }
    }
}

impl From<crate::msgpack::MsgpackError> for FhirPackageError {
    /// Maps a `MessagePack` package-codec failure onto the loader-facing
    /// package error surface (the FPF-5 load path).
    fn from(error: crate::msgpack::MsgpackError) -> Self {
        match error {
            crate::msgpack::MsgpackError::InvalidPackage(inner) => inner,
            crate::msgpack::MsgpackError::UnsupportedPackageSchemaVersion { found, expected } => {
                Self::UnsupportedPackageSchemaVersion { found, expected }
            }
            crate::msgpack::MsgpackError::Truncated => Self::TruncatedPackage,
            crate::msgpack::MsgpackError::Decode(detail)
            | crate::msgpack::MsgpackError::Encode(detail) => Self::MalformedMsgpack(detail),
            crate::msgpack::MsgpackError::Invalid(inner) => {
                Self::MalformedMsgpack(inner.to_string())
            }
            crate::msgpack::MsgpackError::UnsupportedSchemaVersion { found, expected } => {
                Self::MalformedMsgpack(format!(
                    "unsupported msgpack unit schema version {found} (expected {expected})"
                ))
            }
            crate::msgpack::MsgpackError::UnsupportedWireShape { found, expected } => {
                Self::MalformedMsgpack(format!(
                    "unsupported msgpack unit wire shape {found:#018x} (expected {expected:#018x})"
                ))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Codec
// ---------------------------------------------------------------------------

/// Serialize a [`FhirPackage`] to deterministic `postcard` bytes (**legacy
/// v1 codec**, FPF-5). The caller stamps
/// [`LEGACY_PACKAGE_SCHEMA_VERSION`]; the live writer is
/// [`crate::msgpack::encode_package_msgpack`].
///
/// Pure serializer: validity is enforced on [`decode_package`]. The builder
/// (`radix::hir::package::build_package`) is responsible for canonical order.
///
/// # Errors
///
/// Returns `postcard::Error` if serialization fails (e.g. unsupported data
/// shapes); serialization of a valid package is expected to succeed.
pub fn encode_package(pkg: &FhirPackage) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(pkg)
}

/// Deserialize and validate a FHIR package from `postcard` bytes (**legacy
/// v1 codec**, FPF-5): accepts only [`LEGACY_PACKAGE_SCHEMA_VERSION`]
/// envelopes — the wire format of pre-FPF-5 artifacts. The live wire is the
/// `MessagePack` codec ([`crate::msgpack::decode_package_msgpack`]).
///
/// Gates, in order (fail closed):
/// 1. package schema-version prefix (first wire field);
/// 2. full postcard decode of the envelope;
/// 3. per-unit manifest schema version + full unit [`decode_unit`]
///    (unit decode gates) and source-hash consistency;
/// 4. package-level checks: sorted/unique module table, sorted/unique
///    dependencies, entry presence and flag consistency.
///
/// No failure path invokes the lexer, parser, resolver, semantic analyzer,
/// or HIR lowering.
///
/// # Errors
///
/// Returns [`FhirPackageError::TruncatedPackage`] on truncated input,
/// [`FhirPackageError::Malformed`] on postcard decoding failures,
/// [`FhirPackageError::UnsupportedPackageSchemaVersion`] on a version
/// mismatch, [`FhirPackageError::UnsupportedUnitSchemaVersion`] on a unit
/// version mismatch, and the unit-decode / package-level validation errors
/// (missing entry, duplicate modules or dependencies).
pub fn decode_package(bytes: &[u8]) -> Result<DecodedFhirPackage, FhirPackageError> {
    let (schema_version, _rest) = postcard::take_from_bytes::<u32>(bytes).map_err(|e| {
        if matches!(e, postcard::Error::DeserializeUnexpectedEnd) {
            FhirPackageError::TruncatedPackage
        } else {
            FhirPackageError::Malformed(e)
        }
    })?;
    if schema_version != LEGACY_PACKAGE_SCHEMA_VERSION {
        return Err(FhirPackageError::UnsupportedPackageSchemaVersion {
            found: schema_version,
            expected: LEGACY_PACKAGE_SCHEMA_VERSION,
        });
    }

    let pkg: FhirPackage = postcard::from_bytes(bytes).map_err(|e| {
        if matches!(e, postcard::Error::DeserializeUnexpectedEnd) {
            FhirPackageError::TruncatedPackage
        } else {
            FhirPackageError::Malformed(e)
        }
    })?;

    let mut modules = Vec::with_capacity(pkg.modules.len());
    for module in &pkg.modules {
        if module.unit_schema_version != SCHEMA_VERSION {
            return Err(FhirPackageError::UnsupportedUnitSchemaVersion {
                module: module.path.clone(),
                found: module.unit_schema_version,
                expected: SCHEMA_VERSION,
            });
        }
        let unit = decode_unit(&module.unit).map_err(|error| FhirPackageError::MalformedUnit {
            module: module.path.clone(),
            detail: error.to_string(),
        })?;
        if unit.source_identity.content_hash != module.source_hash {
            return Err(FhirPackageError::MismatchedContentHash {
                module: module.path.clone(),
                expected: module.source_hash.clone(),
                actual: unit.source_identity.content_hash.clone(),
            });
        }
        modules.push(DecodedFhirPackageModule {
            path: module.path.clone(),
            module_segments: module.module_segments.clone(),
            is_entry: module.is_entry,
            export_names: module.export_names.clone(),
            local_links: module.local_links.clone(),
            library_imports: module.library_imports.clone(),
            unit,
        });
    }

    validate_module_table(&pkg, &modules)?;
    validate_dependencies(&pkg)?;
    validate_local_links(&pkg)?;

    Ok(DecodedFhirPackage {
        identity: pkg.identity,
        entry_path: pkg.entry_path,
        entry_frontmatter: pkg.entry_frontmatter,
        modules,
        dependencies: pkg.dependencies,
    })
}

// ---------------------------------------------------------------------------
// Package-level validation
// ---------------------------------------------------------------------------

/// Module table checks: canonical sorted order, uniqueness, entry presence,
/// and entry-flag consistency. Shared with the FPF-3 `MessagePack` package
/// codec ([`crate::msgpack::decode_package_msgpack`]).
pub(crate) fn validate_module_table(
    pkg: &FhirPackage,
    decoded: &[DecodedFhirPackageModule],
) -> Result<(), FhirPackageError> {
    for pair in pkg.modules.windows(2) {
        if pair[0].path >= pair[1].path {
            if pair[0].path == pair[1].path {
                return Err(FhirPackageError::DuplicateModulePath {
                    path: pair[0].path.clone(),
                });
            }
            return Err(FhirPackageError::Unsorted {
                what: "module table",
            });
        }
    }

    // Exactly one module may claim the entry flag, and it must be the entry path.
    let flagged: Vec<_> = decoded
        .iter()
        .filter(|module| module.is_entry)
        .map(|module| module.path.as_str())
        .collect();
    if flagged.len() != 1 || flagged[0] != pkg.entry_path {
        return Err(FhirPackageError::MissingEntry {
            entry: pkg.entry_path.clone(),
        });
    }
    Ok(())
}

/// Dependency checks: canonical sorted order and uniqueness by name.
pub(crate) fn validate_dependencies(pkg: &FhirPackage) -> Result<(), FhirPackageError> {
    for pair in pkg.dependencies.windows(2) {
        if pair[0].name >= pair[1].name {
            if pair[0].name == pair[1].name {
                return Err(FhirPackageError::MissingDependencyArtifact {
                    name: pair[0].name.clone(),
                    version: pair[0].version.clone(),
                });
            }
            return Err(FhirPackageError::Unsorted {
                what: "dependencies",
            });
        }
    }
    Ok(())
}

/// Link-table checks: sorted/unique bindings per module, and every link
/// target present in the module table (no dangling local references).
pub(crate) fn validate_local_links(pkg: &FhirPackage) -> Result<(), FhirPackageError> {
    let paths = pkg
        .modules
        .iter()
        .map(|module| module.path.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    for module in &pkg.modules {
        for pair in module.local_links.windows(2) {
            if pair[0].binding >= pair[1].binding {
                if pair[0].binding == pair[1].binding {
                    return Err(FhirPackageError::DanglingModuleRef {
                        from: module.path.clone(),
                        to: pair[0].binding.clone(),
                    });
                }
                return Err(FhirPackageError::Unsorted {
                    what: "local links",
                });
            }
        }
        for link in &module.local_links {
            if !paths.contains(link.target.as_str()) {
                return Err(FhirPackageError::DanglingModuleRef {
                    from: module.path.clone(),
                    to: link.target.clone(),
                });
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "package_test.rs"]
mod tests;
