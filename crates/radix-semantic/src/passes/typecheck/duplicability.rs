//! `copy` over a value that holds something non-duplicable (ruling 7b25889f
//! R5: `copy` stays deep; a value containing something that cannot be
//! duplicated must be refused at compile time, never silently shared).
//!
//! A deep duplicate duplicates every member, so the law is transitive: the
//! walk looks through fields, collection elements, wrapper payloads, unions
//! and tuple members, and refuses when it reaches a member with identity —
//! the conversation family (`sermo`, `meus`, `tuus`; a file or other host
//! resource enters the type system as one of these through `ad`) or a genus
//! that declares `implet Closable` (D8.3: a closable type has identity).
//! The refusal names the member path, e.g. `Pocket.s`.
//!
//! The generic carriers (`Type::Param`, an unsolved hole) stay unchecked:
//! duplicability is decided per instantiation at the call site
//! ([`TypeChecker::refuse_copy_argument_of_nonduplicable`]).

use handroll::{FxHashMap, FxHashSet};
use radix_hir::DefId;
use radix_lexer::Span;
use radix_types::{AccessMarker, AppliedArg, Type, TypeId};

use super::TypeChecker;
use crate::builtins::frame_types;
use crate::locale::{FrameTypeKind, LocalePack, TypeBindingKind};
use crate::{ISSUE_COPY_NONDUPLICABLE_MEMBER, SemanticErrorKind};

/// The deepest walk through one type. Mirrors the closable-cycle walk's cap.
const MAX_DEPTH: u32 = 64;

/// Path segment for an unnamed member: a collection element, a wrapper
/// payload, a union member, or a tuple position.
const ANONYMOUS_SEGMENT: &str = "[…]";

impl TypeChecker<'_> {
    /// Refuse a `copy`-marked binding slot whose payload transitively holds a
    /// non-duplicable member. `slot_ty` is the declared type with its access
    /// marker; the refusal fires only for the `copy` marker (`own` takes the
    /// resource over, it never duplicates).
    pub(super) fn refuse_copy_slot_of_nonduplicable(&mut self, slot_ty: TypeId, span: Span) {
        if slot_ty == self.error_type {
            return;
        }
        let resolved = self.resolve_inferred_type(slot_ty);
        let (marker, payload) = self.types.peel_access_marker(resolved);
        if marker != Some(AccessMarker::Copy) {
            return;
        }
        self.refuse_copy_of_nonduplicable(payload, span);
    }

    /// Refuse a `copy` acquisition over `payload` when it transitively holds
    /// a non-duplicable member. `payload` is the copied value's type past the
    /// `copy` marker; `span` points at the acquisition. The reported member
    /// path is rooted at the copied value's own type (`Pocket.s`).
    pub(super) fn refuse_copy_of_nonduplicable(&mut self, payload: TypeId, span: Span) {
        let root = self.root_path_segment(payload);
        let Some((path, head)) =
            self.nonduplicable_member(payload, root.clone(), &mut FxHashSet::default(), 0)
        else {
            return;
        };
        let mut args: Vec<(&'static str, String)> = vec![("head", head)];
        // A nominal root repeats the carrier's name in every field segment;
        // drop it so the path reads `Pocket.s`, not `Pocket.Pocket.s`.
        let reported = if path == root { String::new() } else { path };
        if !reported.is_empty() {
            args.push(("path", reported));
        }
        self.error_issue_with_args(
            SemanticErrorKind::CopyNonduplicable,
            ISSUE_COPY_NONDUPLICABLE_MEMBER,
            span,
            &args,
        );
    }

    /// The path root for one copied value: its nominal name when the value
    /// is a genus or enum, empty for every other shape.
    fn root_path_segment(&self, payload: TypeId) -> String {
        match self.types.get(self.resolve_inferred_type(payload)).clone() {
            Type::Struct(def) | Type::Enum(def) => self.def_surface_name(def),
            Type::Applied(base, _) => match self.types.get(base) {
                Type::Struct(def) | Type::Enum(def) => self.def_surface_name(*def),
                _ => String::new(),
            },
            _ => String::new(),
        }
    }

    /// Whether `ty` is an unsolved generic carrier a declaration cannot
    /// decide duplicability for. The call site uses this on the callee's
    /// *declared* parameter payload: a declared concrete payload was refused
    /// at its declaration, a declared generic one decides at the argument.
    pub(super) fn type_is_generic_carrier(&self, ty: TypeId) -> bool {
        self.type_contains_generic_carrier(ty, &mut FxHashSet::default(), 0)
    }

    fn type_contains_generic_carrier(
        &self,
        ty: TypeId,
        seen: &mut FxHashSet<TypeId>,
        depth: u32,
    ) -> bool {
        if depth > MAX_DEPTH {
            return false;
        }
        let resolved = self.resolve_inferred_type(ty);
        if !seen.insert(resolved) {
            return false;
        }
        let (_, payload) = self.types.peel_access_marker(resolved);
        match self.types.get(payload) {
            Type::Param(_) | Type::Infer(_) | Type::InferUnion(_) | Type::MarkerHole { .. } => true,
            Type::Alias(_, inner) => self.type_contains_generic_carrier(*inner, seen, depth + 1),
            Type::Union(members) | Type::Tuple(members) | Type::Intersection(members) => members
                .iter()
                .any(|member| self.type_contains_generic_carrier(*member, seen, depth + 1)),
            other => {
                let mut children = Vec::new();
                collect_member_children(other, &mut children);
                children
                    .into_iter()
                    .any(|child| self.type_contains_generic_carrier(child, seen, depth + 1))
            }
        }
    }

    /// Depth-first search for a member with identity. Returns the member path
    /// (empty for the value itself) and its locale surface name.
    fn nonduplicable_member(
        &mut self,
        ty: TypeId,
        path: String,
        seen: &mut FxHashSet<DefId>,
        depth: u32,
    ) -> Option<(String, String)> {
        if depth > MAX_DEPTH {
            return None;
        }
        let resolved = self.resolve_inferred_type(ty);
        let (_, payload) = self.types.peel_access_marker(resolved);
        match self.types.get(payload).clone() {
            // A lowerer-level refusal already reported this acquisition.
            Type::Error => None,
            Type::Struct(def) => {
                if let Some(head) = self.conversation_head(def) {
                    return Some((path, head));
                }
                if self.def_is_closable(def) {
                    return Some((path, self.def_surface_name(def)));
                }
                if !seen.insert(def) {
                    return None;
                }
                let found = self.search_struct_fields(def, &path, seen, depth, None);
                seen.remove(&def);
                found
            }
            Type::Enum(def) => {
                if !seen.insert(def) {
                    return None;
                }
                let found = self.search_enum_variants(def, &path, seen, depth, None);
                seen.remove(&def);
                found
            }
            Type::Interface(def) => {
                // A value typed as the closing contract itself holds a
                // resource with identity.
                (self.types.closing_contract == Some(def))
                    .then(|| (path, self.def_surface_name(def)))
            }
            Type::Applied(base, args) => match self.types.get(base).clone() {
                Type::Struct(def) => {
                    if let Some(head) = self.conversation_head(def) {
                        return Some((path, head));
                    }
                    if self.def_is_closable(def) {
                        return Some((path, self.def_surface_name(def)));
                    }
                    if !seen.insert(def) {
                        return None;
                    }
                    let found = self.search_struct_fields(def, &path, seen, depth, Some(&args));
                    seen.remove(&def);
                    found
                }
                Type::Enum(def) => {
                    if !seen.insert(def) {
                        return None;
                    }
                    let found = self.search_enum_variants(def, &path, seen, depth, Some(&args));
                    seen.remove(&def);
                    found
                }
                _ => self.search_children(
                    args.iter().filter_map(|arg| match arg {
                        AppliedArg::Type(inner) => Some(*inner),
                        AppliedArg::Size(_) => None,
                    }),
                    &path,
                    seen,
                    depth,
                ),
            },
            Type::Alias(_, inner) => self.nonduplicable_member(inner, path, seen, depth + 1),
            Type::Union(members) | Type::Tuple(members) | Type::Intersection(members) => {
                self.search_children(members.into_iter(), &path, seen, depth)
            }
            other => {
                let mut children = Vec::new();
                collect_member_children(&other, &mut children);
                self.search_children(children.into_iter(), &path, seen, depth)
            }
        }
    }

    /// Walk one genus's instance fields in declaration order (span order,
    /// because the field table is a hash map).
    fn search_struct_fields(
        &mut self,
        def: DefId,
        path: &str,
        seen: &mut FxHashSet<DefId>,
        depth: u32,
        applied_args: Option<&[AppliedArg]>,
    ) -> Option<(String, String)> {
        let info = self.structs.get(&def)?;
        let mut fields: Vec<_> = info
            .fields
            .iter()
            .filter(|(_, field)| field.modifier != Some(radix_syntax::FieldModifier::Const))
            .map(|(name, field)| (*name, *field))
            .collect();
        fields.sort_by_key(|(_, field)| field.span.start);
        let parameters = self
            .type_params_by_def
            .get(&def)
            .cloned()
            .unwrap_or_default();
        let mut variables = FxHashMap::default();
        let mut shape_variables = FxHashMap::default();
        if let Some(applied_args) = applied_args {
            for (parameter, argument) in parameters.iter().zip(applied_args) {
                match argument {
                    AppliedArg::Type(witness) => {
                        variables.insert(*parameter, *witness);
                    }
                    AppliedArg::Size(witness) => {
                        shape_variables.insert(*parameter, *witness);
                    }
                }
            }
        }
        for (name, field) in fields {
            let segment = self
                .interner
                .map(|interner| interner.resolve(name).to_owned())
                .unwrap_or_default();
            let child_path = join_field(path, &segment);
            let field_ty = if applied_args.is_some() {
                self.substitute_type_params(field.ty, &mut variables, Some(&mut shape_variables))
            } else {
                field.ty
            };
            if let Some(found) = self.nonduplicable_member(field_ty, child_path, seen, depth + 1) {
                return Some(found);
            }
        }
        None
    }

    /// Walk one enum's variants (declaration order: variant `DefId`s are
    /// allocated in declaration order) with their payload fields.
    fn search_enum_variants(
        &mut self,
        def: DefId,
        path: &str,
        seen: &mut FxHashSet<DefId>,
        depth: u32,
        applied_args: Option<&[AppliedArg]>,
    ) -> Option<(String, String)> {
        let mut variants: Vec<DefId> = self
            .variant_parent
            .iter()
            .filter(|(_, parent)| **parent == def)
            .map(|(variant, _)| *variant)
            .collect();
        variants.sort_by_key(|variant| variant.0);
        let parameters = self
            .type_params_by_def
            .get(&def)
            .cloned()
            .unwrap_or_default();
        let mut variables = FxHashMap::default();
        let mut shape_variables = FxHashMap::default();
        if let Some(applied_args) = applied_args {
            for (parameter, argument) in parameters.iter().zip(applied_args) {
                match argument {
                    AppliedArg::Type(witness) => {
                        variables.insert(*parameter, *witness);
                    }
                    AppliedArg::Size(witness) => {
                        shape_variables.insert(*parameter, *witness);
                    }
                }
            }
        }
        for variant in variants {
            let Some(fields) = self.variant_fields.get(&variant).cloned() else {
                continue;
            };
            let names = self
                .variant_field_names
                .get(&variant)
                .cloned()
                .unwrap_or_default();
            for (index, field_ty) in fields.into_iter().enumerate() {
                let segment = names
                    .get(index)
                    .and_then(|name| {
                        self.interner
                            .map(|interner| interner.resolve(*name).to_owned())
                    })
                    .unwrap_or_else(|| ANONYMOUS_SEGMENT.to_owned());
                let child_path = join_anonymous(path, &segment);
                let field_ty = if applied_args.is_some() {
                    self.substitute_type_params(
                        field_ty,
                        &mut variables,
                        Some(&mut shape_variables),
                    )
                } else {
                    field_ty
                };
                if let Some(found) =
                    self.nonduplicable_member(field_ty, child_path, seen, depth + 1)
                {
                    return Some(found);
                }
            }
        }
        None
    }

    /// Walk every child under one anonymous segment.
    fn search_children(
        &mut self,
        children: impl Iterator<Item = TypeId>,
        path: &str,
        seen: &mut FxHashSet<DefId>,
        depth: u32,
    ) -> Option<(String, String)> {
        let child_path = join_anonymous(path, ANONYMOUS_SEGMENT);
        for child in children {
            if let Some(found) =
                self.nonduplicable_member(child, child_path.clone(), seen, depth + 1)
            {
                return Some(found);
            }
        }
        None
    }

    /// The locale surface of a conversation-family def, `None` for others.
    fn conversation_head(&self, def: DefId) -> Option<String> {
        let kind = if Some(def) == frame_types::sermo_def_id(self.resolver) {
            FrameTypeKind::Sermo
        } else if Some(def) == frame_types::meus_def_id(self.resolver) {
            FrameTypeKind::Meus
        } else if Some(def) == frame_types::tuus_def_id(self.resolver) {
            FrameTypeKind::Tuus
        } else {
            return None;
        };
        let surface = self.locale_pack.map_or(kind.latin(), |pack: &LocalePack| {
            pack.type_surface(TypeBindingKind::Frame(kind))
        });
        Some(surface.to_owned())
    }

    /// A def's source name; builtins without a pack keep the Latin identity.
    fn def_surface_name(&self, def: DefId) -> String {
        self.function_display_name(def)
            .unwrap_or_else(|| "sermo".to_owned())
    }
}

/// Join a named field onto the path: `Pocket` + `s` is `Pocket.s`.
fn join_field(path: &str, segment: &str) -> String {
    let mut joined = String::with_capacity(path.len() + segment.len() + 1);
    joined.push_str(path);
    if !path.is_empty() {
        joined.push('.');
    }
    joined.push_str(segment);
    joined
}

/// Join an unnamed member onto the path: `Pocket.conns` + `[…]` is
/// `Pocket.conns[…]`.
fn join_anonymous(path: &str, segment: &str) -> String {
    let mut joined = String::with_capacity(path.len() + segment.len());
    joined.push_str(path);
    joined.push_str(segment);
    joined
}

/// The value-member children of a composite type. Function types are skipped:
/// a parameter is not a stored member, so `fn(sermo)` holds no conversation.
fn collect_member_children(ty: &Type, out: &mut Vec<TypeId>) {
    match ty {
        Type::Array(inner)
        | Type::Queue(inner)
        | Type::Stack(inner)
        | Type::Set(inner)
        | Type::Promissum(inner)
        | Type::Cursor(inner)
        | Type::Option(inner)
        | Type::Ref(_, inner)
        | Type::Atomic(inner) => out.push(*inner),
        Type::BoundedArray { element, .. }
        | Type::RangedArray { element, .. }
        | Type::BoundedQueue { element, .. }
        | Type::BoundedStack { element, .. } => out.push(*element),
        Type::Map(key, value)
        | Type::Ledger(key, value)
        | Type::PromissumFailable(key, value)
        | Type::AsyncCursor(key, value) => {
            out.push(*key);
            out.push(*value);
        }
        Type::Record(fields) => out.extend(fields.values().copied()),
        Type::Applied(_, args) => {
            out.extend(args.iter().filter_map(|arg| match arg {
                AppliedArg::Type(inner) => Some(*inner),
                AppliedArg::Size(_) => None,
            }));
        }
        _ => {}
    }
}
