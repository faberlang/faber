//! Portable qualified identity for imports, exports, and annotation contracts.
//!
//! Local `DefId` values never cross unit boundaries. Portable identity carries
//! provider/package/module/export components so package graphs and annotation
//! handoff remain coherent across files.

use handroll::FxHashMap;
use radix_hir::{DefId, LibraryIdentity, LibraryItemKind, LibraryProvider, LibraryRegistry};
use radix_lexer::Interner;
use std::collections::BTreeMap;

/// Declaration kind carried on a portable export identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QualifiedDeclKind {
    Function,
    TypeAlias,
    Struct,
    Enum,
    Interface,
    Const,
    AnnotationContract,
}

impl From<LibraryItemKind> for QualifiedDeclKind {
    fn from(kind: LibraryItemKind) -> Self {
        match kind {
            LibraryItemKind::Function => Self::Function,
            LibraryItemKind::TypeAlias => Self::TypeAlias,
            LibraryItemKind::Struct => Self::Struct,
            LibraryItemKind::Enum => Self::Enum,
            LibraryItemKind::Interface => Self::Interface,
            LibraryItemKind::Const => Self::Const,
        }
    }
}

/// Target-neutral portable identity for one export or import binding.
///
/// Local `DefId` is never part of this identity. Aliases do not change it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QualifiedIdentity {
    pub provider: String,
    pub package: Option<String>,
    pub module_path: Vec<String>,
    pub export_name: String,
    pub kind: QualifiedDeclKind,
}

impl QualifiedIdentity {
    pub fn from_library_identity(
        identity: &LibraryIdentity,
        export_name: impl Into<String>,
        kind: QualifiedDeclKind,
    ) -> Self {
        let (provider, package) = match &identity.provider {
            LibraryProvider::Builtin(name) => (format!("builtin:{name}"), None),
            LibraryProvider::Package(name) => ("package".to_owned(), Some(name.clone())),
        };
        Self {
            provider,
            package,
            module_path: identity.module_path.clone(),
            export_name: export_name.into(),
            kind,
        }
    }

    /// Stable display key for diagnostics and tests.
    #[must_use]
    pub fn display_key(&self) -> String {
        let mut parts = Vec::new();
        parts.push(self.provider.clone());
        if let Some(package) = &self.package {
            parts.push(package.clone());
        }
        parts.extend(self.module_path.iter().cloned());
        parts.push(self.export_name.clone());
        parts.join("::")
    }
}

/// Side table mapping unit-local definitions to portable identities.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QualifiedIdentityTable {
    by_local_def: FxHashMap<DefId, QualifiedIdentity>,
    by_identity: BTreeMap<String, DefId>,
}

impl QualifiedIdentityTable {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, local_def: DefId, identity: QualifiedIdentity) {
        let key = identity.display_key();
        self.by_identity.insert(key, local_def);
        self.by_local_def.insert(local_def, identity);
    }

    #[must_use]
    pub fn get(&self, local_def: DefId) -> Option<&QualifiedIdentity> {
        self.by_local_def.get(&local_def)
    }

    #[must_use]
    pub fn local_for_key(&self, key: &str) -> Option<DefId> {
        self.by_identity.get(key).copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = (DefId, &QualifiedIdentity)> {
        self.by_local_def.iter().map(|(def, id)| (*def, id))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_local_def.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_local_def.is_empty()
    }
}

/// Build portable identities from the unit's library provenance registry.
#[must_use]
pub fn build_qualified_identities(
    libraries: &LibraryRegistry,
    _interner: &Interner,
) -> QualifiedIdentityTable {
    let mut table = QualifiedIdentityTable::new();

    for (def_id, item) in &libraries.items {
        let identity = QualifiedIdentity::from_library_identity(
            &item.identity,
            item.exported_name.clone(),
            item.kind.clone().into(),
        );
        table.insert(*def_id, identity);
    }

    for (def_id, binding) in &libraries.bindings {
        // Namespace bindings use the last module segment as the export name when
        // present; otherwise the local symbol name.
        let export_name = binding
            .identity
            .module_path
            .last()
            .cloned()
            .unwrap_or_else(|| format!("binding-{}", def_id.0));
        let identity = QualifiedIdentity::from_library_identity(
            &binding.identity,
            export_name,
            QualifiedDeclKind::Interface,
        );
        if table.get(*def_id).is_none() {
            table.insert(*def_id, identity);
        }
    }

    table
}
