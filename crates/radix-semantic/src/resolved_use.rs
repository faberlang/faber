//! Target-neutral resolved definition and import-use facts.
//!
//! These facts feed package graphs and target emission needs so backends do not
//! rediscover uses by scanning HIR or rendered text independently.

use crate::qualified_identity::QualifiedIdentityTable;
use crate::{Type, TypeId, TypeTable};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::visit::{HirVisitor, walk_expr, walk_local, walk_pattern};
use radix_hir::{
    DefId, HirExpression, HirExpressionKind, HirItemKind, HirLocal, HirModule, HirParam, HirPattern,
};
use serde::{Deserialize, Serialize};

/// One resolved use of a definition or portable import.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResolvedUseKey {
    /// Unit-local definition.
    Local(DefId),
    /// Portable import identity display key.
    Portable(String),
}

/// Use-site classification for downstream needs collectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResolvedUseKind {
    Path,
    Call,
    MethodCall,
    TypeRef,
    ImportBinding,
}

/// One recorded use.
///
/// F9: included in wire shape because codegen backends consume the
/// resolved-use list to decide which imports and type paths need
/// to be emitted without re-scanning HIR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedUse {
    pub key: ResolvedUseKey,
    pub kind: ResolvedUseKind,
    /// Enclosing function/method def, when the use is inside a body.
    pub enclosing: Option<DefId>,
    /// Unit-local definition the site resolves to, when one exists.
    ///
    /// Import-binding records leave this `None` (a binding site is never
    /// itself a local use), so a loaded index can rebuild `by_local` exactly
    /// like direct analysis: a portable key still names its local binding
    /// here, and no import is counted as used merely for being declared.
    pub local: Option<DefId>,
}

/// Index of resolved uses for one analyzed unit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResolvedUseIndex {
    uses: Vec<ResolvedUse>,
    by_local: FxHashMap<DefId, Vec<usize>>,
    by_portable: FxHashMap<String, Vec<usize>>,
}

impl ResolvedUseIndex {
    #[must_use]
    pub fn uses(&self) -> &[ResolvedUse] {
        &self.uses
    }

    pub fn uses_of_local(&self, def_id: DefId) -> impl Iterator<Item = &ResolvedUse> {
        self.by_local
            .get(&def_id)
            .into_iter()
            .flat_map(|idxs| idxs.iter().map(|&i| &self.uses[i]))
    }

    pub fn uses_of_portable(&self, key: &str) -> impl Iterator<Item = &ResolvedUse> {
        self.by_portable
            .get(key)
            .into_iter()
            .flat_map(|idxs| idxs.iter().map(|&i| &self.uses[i]))
    }

    /// Return local definition identities used by non-binding sites.
    ///
    /// This remains local-identity based even when the corresponding
    /// [`ResolvedUse`] key is portable. Backends need the local `DefId` to match
    /// uses back to HIR import bindings before rendering target paths.
    #[must_use]
    pub fn locals_used(&self) -> FxHashSet<DefId> {
        self.by_local.keys().copied().collect()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.uses.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.uses.is_empty()
    }

    /// Rebuild lookup indexes from a pre-serialized uses vector (F9, F20).
    ///
    /// Reconstructs the `by_local` and `by_portable` hash maps that were
    /// excluded from the serialized wire shape (CTM E.2). The `uses` vector
    /// is set directly.
    #[must_use]
    pub fn from_snapshot(uses: Vec<ResolvedUse>) -> Self {
        let mut by_local: FxHashMap<DefId, Vec<usize>> = FxHashMap::default();
        let mut by_portable: FxHashMap<String, Vec<usize>> = FxHashMap::default();

        for (idx, use_site) in uses.iter().enumerate() {
            if let ResolvedUseKey::Portable(key) = &use_site.key {
                by_portable.entry(key.clone()).or_default().push(idx);
            }
            // ImportBinding records are declaration facts, not uses: direct
            // analysis indexes them only under `by_portable` (their `local`
            // is `None`), so the rebuilt index must not count every import
            // as used. A use site carries its local def even when its key
            // is portable; a local key without a carried def names the same
            // definition directly.
            let local = use_site.local.or(match &use_site.key {
                ResolvedUseKey::Local(def) if use_site.kind != ResolvedUseKind::ImportBinding => {
                    Some(*def)
                }
                _ => None,
            });
            if let Some(def) = local {
                by_local.entry(def).or_default().push(idx);
            }
        }

        Self {
            uses,
            by_local,
            by_portable,
        }
    }

    /// Return the wire-format snapshot: the `uses` vector only.
    ///
    /// F9 / F10: `by_local` and `by_portable` lookup maps are excluded from
    /// the serialized wire shape. They are rebuilt on load by
    /// `from_snapshot()` (U2.2).
    #[must_use]
    pub fn to_snapshot(&self) -> Vec<ResolvedUse> {
        self.uses.clone()
    }

    fn push(&mut self, use_site: ResolvedUse) {
        self.push_with_local(None, use_site);
    }

    fn push_local(&mut self, local_def: DefId, use_site: ResolvedUse) {
        self.push_with_local(Some(local_def), use_site);
    }

    fn push_with_local(&mut self, local_def: Option<DefId>, use_site: ResolvedUse) {
        let idx = self.uses.len();
        // `push` only receives `ImportBinding` uses and `push_local` always
        // supplies a `DefId`, so the caller's value is authoritative: a
        // binding site is never itself a local use.
        if let Some(def) = local_def {
            self.by_local.entry(def).or_default().push(idx);
        }
        if let ResolvedUseKey::Portable(key) = &use_site.key {
            self.by_portable.entry(key.clone()).or_default().push(idx);
        }
        let mut use_site = use_site;
        use_site.local = local_def;
        self.uses.push(use_site);
    }
}

/// Build the resolved-use index from typed HIR and qualified import identities.
#[must_use]
pub fn analyze_resolved_uses(
    hir: &HirModule,
    types: &TypeTable,
    qualified: &QualifiedIdentityTable,
) -> ResolvedUseIndex {
    let mut index = ResolvedUseIndex::default();

    // Import bindings are uses of portable identity at the declaration site.
    for item in &hir.items {
        if let HirItemKind::Import(import) = &item.kind {
            for import_item in &import.items {
                let key = match qualified.get(import_item.def_id) {
                    Some(identity) => ResolvedUseKey::Portable(identity.display_key()),
                    None => ResolvedUseKey::Local(import_item.def_id),
                };
                index.push(ResolvedUse {
                    key,
                    kind: ResolvedUseKind::ImportBinding,
                    enclosing: None,
                    local: None,
                });
            }
        }
    }

    let mut visitor = UseVisitor {
        index: &mut index,
        enclosing: None,
        qualified,
        types,
        visited_types: FxHashSet::default(),
    };

    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(func) => {
                visitor.enclosing = Some(item.def_id);
                visitor.record_function_signature(func);
                if let Some(body) = &func.body {
                    visitor.visit_block(body);
                }
            }
            HirItemKind::Struct(strukt) => {
                visitor.enclosing = Some(item.def_id);
                for field in &strukt.fields {
                    visitor.record_type(field.ty);
                    if let Some(init) = &field.init {
                        visitor.visit_expr(init);
                    }
                }
                for method in &strukt.methods {
                    visitor.enclosing = Some(method.def_id);
                    visitor.record_function_signature(&method.func);
                    if let Some(body) = &method.func.body {
                        visitor.visit_block(body);
                    }
                }
            }
            HirItemKind::Enum(enum_item) => {
                visitor.enclosing = Some(item.def_id);
                for variant in &enum_item.variants {
                    for field in &variant.fields {
                        visitor.record_type(field.ty);
                    }
                }
            }
            HirItemKind::Interface(interface) => {
                visitor.enclosing = Some(item.def_id);
                for method in &interface.methods {
                    for param in &method.params {
                        visitor.visit_param(param);
                    }
                    if let Some(ret) = method.ret_ty {
                        visitor.record_type(ret);
                    }
                    if let Some(err) = method.err_ty {
                        visitor.record_type(err);
                    }
                }
            }
            HirItemKind::TypeAlias(alias) => {
                visitor.enclosing = Some(item.def_id);
                visitor.record_type(alias.ty);
            }
            HirItemKind::Schema(schema) => {
                visitor.enclosing = Some(item.def_id);
                for column in &schema.columns {
                    visitor.record_type(column.ty);
                }
            }
            HirItemKind::Constant(konst) => {
                visitor.enclosing = None;
                if let Some(ty) = konst.ty {
                    visitor.record_type(ty);
                }
                visitor.visit_expr(&konst.value);
            }
            HirItemKind::Import(_) => {}
        }
    }

    if let Some(entry) = &hir.entry {
        visitor.enclosing = None;
        visitor.visit_block(entry);
    }

    index
}

struct UseVisitor<'a> {
    index: &'a mut ResolvedUseIndex,
    enclosing: Option<DefId>,
    qualified: &'a QualifiedIdentityTable,
    types: &'a TypeTable,
    visited_types: FxHashSet<TypeId>,
}

impl UseVisitor<'_> {
    fn record_local(&mut self, def_id: DefId, kind: ResolvedUseKind) {
        let key = match self.qualified.get(def_id) {
            Some(identity) => ResolvedUseKey::Portable(identity.display_key()),
            None => ResolvedUseKey::Local(def_id),
        };
        self.index.push_local(
            def_id,
            ResolvedUse {
                key,
                kind,
                enclosing: self.enclosing,
                // `push_with_local` stamps the local def; a portable key
                // still names this unit-local binding.
                local: None,
            },
        );
    }

    fn record_function_signature(&mut self, function: &radix_hir::HirFunction) {
        for param in &function.params {
            self.visit_param(param);
        }
        if let Some(cli_args) = &function.cli_args {
            self.visit_param(cli_args);
        }
        if let Some(ret) = function.ret_ty {
            self.record_type(ret);
        }
        if let Some(err) = function.err_ty {
            self.record_type(err);
        }
    }

    fn record_type(&mut self, ty: TypeId) {
        if !self.visited_types.insert(ty) {
            return;
        }

        let children = match self.types.get(ty).clone() {
            Type::Struct(def_id) | Type::Enum(def_id) | Type::Interface(def_id) => {
                self.record_local(def_id, ResolvedUseKind::TypeRef);
                Vec::new()
            }
            Type::Alias(def_id, inner) => {
                self.record_local(def_id, ResolvedUseKind::TypeRef);
                vec![inner]
            }
            Type::Array(inner)
            | Type::Set(inner)
            | Type::Promissum(inner)
            | Type::Cursor(inner)
            | Type::Queue(inner)
            | Type::Stack(inner)
            | Type::Atomic(inner)
            | Type::Intervallum(inner, _)
            | Type::Option(inner)
            | Type::Ref(_, inner) => vec![inner],
            Type::BoundedArray { element, .. }
            | Type::RangedArray { element, .. }
            | Type::BoundedQueue { element, .. }
            | Type::BoundedStack { element, .. } => vec![element],
            Type::PromissumFailable(success, alternate) | Type::AsyncCursor(success, alternate) => {
                vec![success, alternate]
            }
            Type::Tensor(inner, _)
            | Type::Vector(inner, _)
            | Type::Matrix(inner, _)
            | Type::Sparsa(inner, _) => {
                vec![inner]
            }
            Type::Map(key, value) | Type::Ledger(key, value) => vec![key, value],
            Type::Record(fields) => fields.into_values().collect(),
            Type::Func(sig) => {
                let mut children = sig
                    .params
                    .into_iter()
                    .map(|param| param.ty)
                    .collect::<Vec<_>>();
                children.push(sig.ret);
                if let Some(err) = sig.err {
                    children.push(err);
                }
                children
            }
            Type::Applied(base, args) => {
                let mut children = vec![base];
                children.extend(args.into_iter().filter_map(|arg| match arg {
                    radix_types::AppliedArg::Type(id) => Some(id),
                    radix_types::AppliedArg::Size(_) => None,
                }));
                children
            }
            Type::Union(members) | Type::Intersection(members) => members,
            Type::Primitive(_)
            | Type::SizedNumeric(_, _)
            | Type::Bits(_)
            | Type::ModularWord(_)
            | Type::SaturatingWord(_)
            | Type::RangedInt { .. }
            | Type::PolicyFloat(_, _)
            | Type::SizedInstans(_)
            | Type::BoundedTextus { .. }
            | Type::BoundedAscii { .. }
            | Type::BoundedOcteti { .. }
            | Type::TensorEdgePolicy(_)
            | Type::Tuple(_)
            | Type::Param(_)
            | Type::Infer(_)
            | Type::InferUnion(_)
            | Type::MarkerHole { .. }
            | Type::Error => Vec::new(),
        };

        for child in children {
            self.record_type(child);
        }
    }
}

impl HirVisitor for UseVisitor<'_> {
    fn visit_param(&mut self, param: &HirParam) {
        self.record_type(param.ty);
        if let Some(default) = &param.default {
            self.visit_expr(default);
        }
    }

    fn visit_local(&mut self, local: &HirLocal) {
        if let Some(ty) = local.ty {
            self.record_type(ty);
        }
        walk_local(self, local);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        if let Some(ty) = expr.ty {
            self.record_type(ty);
        }
        match &expr.kind {
            HirExpressionKind::Path(def_id) => {
                self.record_local(*def_id, ResolvedUseKind::Path);
                walk_expr(self, expr);
            }
            HirExpressionKind::Call(callee, _, _, _) => {
                if let HirExpressionKind::Path(def_id) = &callee.kind {
                    self.record_local(*def_id, ResolvedUseKind::Call);
                }
                // Full walk records nested expression uses (including callee path as Path).
                walk_expr(self, expr);
            }
            HirExpressionKind::MethodCall(_, _, _, _, _) => {
                // Method resolution identity is effect-table owned; still walk nested exprs.
                walk_expr(self, expr);
            }
            HirExpressionKind::Struct(def_id, fields) => {
                self.record_local(*def_id, ResolvedUseKind::TypeRef);
                for (_, field_expr) in fields {
                    self.visit_expr(field_expr);
                }
            }
            HirExpressionKind::Verte { target, .. } => {
                self.record_type(*target);
                walk_expr(self, expr);
            }
            HirExpressionKind::Conversio { target, route, .. } => {
                if let radix_hir::HirConversioTarget::Type(target) = target {
                    self.record_type(*target);
                }
                // D9.3: the registered `@ conversio` function is called.
                if let Some(radix_hir::HirConversioRoute::Function { def_id, .. }) = route {
                    self.record_local(*def_id, ResolvedUseKind::Call);
                }
                walk_expr(self, expr);
            }
            HirExpressionKind::TypeCheck {
                expr: source, ty, ..
            } => {
                self.record_type(*ty);
                self.visit_expr(source);
            }
            _ => walk_expr(self, expr),
        }
    }

    fn visit_pattern(&mut self, pattern: &HirPattern) {
        if let HirPattern::Variant(def_id, _) = pattern {
            // Importer-local union/enum variant paths are resolved references.
            // NameCatalog does not visit them; emit must register them or
            // resolve_def reports CODEGEN001.
            self.record_local(*def_id, ResolvedUseKind::Path);
        }
        walk_pattern(self, pattern);
    }
}
