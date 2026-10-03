//! Typed-HIR admission for iteration-independent `filum` bodies.
//!
//! The proof is private to this pass. Storage roots survive aliases, and view
//! coordinates are mapped before comparing accesses. Unknown provenance or
//! effects decline admission; ordinary iteration is not checked.

use crate::{SemanticError, SemanticErrorKind, Type, TypeId, TypeTable};
use radix_hir::visit::{HirVisitor, walk_expr, walk_local, walk_stmt};
use radix_hir::{
    DefId, HirArrayElement, HirBlock, HirExpression, HirExpressionKind, HirFunction, HirLocal,
    HirModule, HirObjectKey, HirStatement, HirStatementKind,
};
use radix_lexer::{Interner, Span, Symbol};
use rustc_hash::{FxHashMap, FxHashSet};

const ISSUE: &str = "filum_independence_unproven";

#[derive(Clone, Debug, PartialEq, Eq)]
enum Transform {
    Transpose,
    Slice(i128, i128),
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Storage {
    root: DefId,
    transforms: Vec<Transform>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Facts {
    storage: Vec<Storage>,
    unknown: bool,
}

impl Facts {
    fn fresh(root: DefId) -> Self {
        Self {
            storage: vec![Storage {
                root,
                transforms: Vec::new(),
            }],
            unknown: false,
        }
    }

    fn unknown() -> Self {
        Self {
            storage: Vec::new(),
            unknown: true,
        }
    }

    fn merge(&mut self, other: Self) {
        self.unknown |= other.unknown;
        for storage in other.storage {
            if !self.storage.contains(&storage) {
                self.storage.push(storage);
            }
        }
    }
}

/// Flow-insensitive unions intentionally include stores after reads and every
/// branch. Rebinding cannot make an earlier alias disappear from the proof.
#[derive(Default)]
struct Aliases {
    bindings: Vec<HirLocal>,
    stores: Vec<(HirExpression, HirExpression)>,
    values: FxHashMap<DefId, Facts>,
    fields: FxHashMap<(DefId, Symbol), Facts>,
    uncertain: bool,
}

impl Aliases {
    fn build(body: &HirBlock, types: &TypeTable, interner: &Interner) -> Self {
        let mut effects = UnknownEffects {
            types,
            interner,
            found: false,
        };
        effects.visit_block(body);
        let mut declarations = Declarations::default();
        declarations.visit_block(body);
        let mut result = Self {
            bindings: declarations.bindings,
            stores: declarations.stores,
            ..Self::default()
        };
        // Acyclic propagation needs at most one round per equation. A cycle
        // that keeps transforming views has no finite address certificate.
        let rounds = 2 * (result.bindings.len() + result.stores.len() + 1);
        for _ in 0..rounds {
            let before = (result.values.clone(), result.fields.clone());
            for local in result.bindings.clone() {
                let facts = if local.ty.is_some_and(|ty| types.is_copy_view(ty)) {
                    Facts::fresh(local.def_id)
                } else if let Some(init) = &local.init {
                    result.initializer(init, local.def_id, types, interner)
                } else {
                    Facts::unknown()
                };
                result.values.entry(local.def_id).or_default().merge(facts);
            }
            for (target, source) in result.stores.clone() {
                let facts = result.facts(&source, types, interner);
                match &target.kind {
                    HirExpressionKind::Path(def) => {
                        let copied = result.bindings.iter().any(|local| {
                            local.def_id == *def
                                && local.ty.is_some_and(|ty| types.is_copy_view(ty))
                        });
                        result.values.entry(*def).or_default().merge(if copied {
                            Facts::fresh(*def)
                        } else {
                            facts
                        });
                    }
                    HirExpressionKind::Field(object, field) => {
                        for storage in result.facts(object, types, interner).storage {
                            result
                                .fields
                                .entry((storage.root, *field))
                                .or_default()
                                .merge(facts.clone());
                        }
                    }
                    _ => {}
                }
            }
            if before == (result.values.clone(), result.fields.clone()) {
                result.uncertain |= effects.found;
                return result;
            }
        }
        result.uncertain = true;
        result
    }

    fn initializer(
        &mut self,
        expr: &HirExpression,
        root: DefId,
        types: &TypeTable,
        interner: &Interner,
    ) -> Facts {
        match &expr.kind {
            HirExpressionKind::Vacua | HirExpressionKind::Array(_) => Facts::fresh(root),
            HirExpressionKind::Struct(_, fields) => {
                for (name, value) in fields {
                    let facts = self.facts(value, types, interner);
                    self.fields.entry((root, *name)).or_default().merge(facts);
                }
                Facts::fresh(root)
            }
            HirExpressionKind::Verte {
                entries: Some(entries),
                ..
            } => {
                for entry in entries {
                    let HirObjectKey::Ident(name) = entry.key else {
                        return Facts::unknown();
                    };
                    let Some(value) = &entry.value else {
                        return Facts::unknown();
                    };
                    let facts = self.facts(value, types, interner);
                    self.fields.entry((root, name)).or_default().merge(facts);
                }
                Facts::fresh(root)
            }
            _ => self.facts(expr, types, interner),
        }
    }

    fn facts(&self, expr: &HirExpression, types: &TypeTable, interner: &Interner) -> Facts {
        if self.uncertain {
            return Facts::unknown();
        }
        match &expr.kind {
            HirExpressionKind::Path(def) => self.values.get(def).cloned().unwrap_or_default(),
            HirExpressionKind::Field(object, field) => {
                let parent = self.facts(object, types, interner);
                let mut result = Facts {
                    unknown: parent.unknown,
                    ..Facts::default()
                };
                for storage in parent.storage {
                    result.merge(
                        self.fields
                            .get(&(storage.root, *field))
                            .cloned()
                            .unwrap_or_else(Facts::unknown),
                    );
                }
                result
            }
            HirExpressionKind::NonNull(inner, _)
            | HirExpressionKind::Deref(inner)
            | HirExpressionKind::Ref(_, inner)
            | HirExpressionKind::Verte {
                source: inner,
                entries: None,
                ..
            } => self.facts(inner, types, interner),
            HirExpressionKind::MethodCall(receiver, name, _, args, _) => {
                if !is_access_intrinsic(receiver, *name, args.len(), types, interner) {
                    return Facts::unknown();
                }
                let transform = match interner.resolve(*name) {
                    "transpone" | "__faber_glyph_transpose" => Transform::Transpose,
                    "sectio" if args.len() >= 2 => {
                        let start = super::const_int_value(&args[0].expr);
                        let step = args
                            .get(2)
                            .map_or(Some(1), |arg| super::const_int_value(&arg.expr));
                        match (start, step) {
                            (Some(start), Some(step)) if step != 0 => Transform::Slice(start, step),
                            _ => Transform::Unknown,
                        }
                    }
                    "forma" | "expande" => Transform::Unknown,
                    _ => return Facts::unknown(),
                };
                let mut result = self.facts(receiver, types, interner);
                for storage in &mut result.storage {
                    storage.transforms.push(transform.clone());
                }
                result
            }
            _ => Facts::unknown(),
        }
    }
}

fn is_access_intrinsic(
    receiver: &HirExpression,
    name: Symbol,
    argc: usize,
    types: &TypeTable,
    interner: &Interner,
) -> bool {
    receiver.ty.is_some_and(|ty| {
        radix_intrinsics::lookup_method_for_type_id(ty, types, interner.resolve(name), argc)
            .is_some()
    })
}

struct UnknownEffects<'a> {
    types: &'a TypeTable,
    interner: &'a Interner,
    found: bool,
}
impl HirVisitor for UnknownEffects<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Call(..)
            | HirExpressionKind::Ad { .. }
            | HirExpressionKind::CopyInto { .. }
            | HirExpressionKind::Reddet(_)
            | HirExpressionKind::Tacebit(_)
            | HirExpressionKind::Conversio { route: Some(_), .. } => self.found = true,
            HirExpressionKind::MethodCall(receiver, name, _, args, _) => {
                if !is_access_intrinsic(receiver, *name, args.len(), self.types, self.interner) {
                    self.found = true;
                }
            }
            _ => {}
        }
        walk_expr(self, expr);
    }
}

#[derive(Default)]
struct Declarations {
    bindings: Vec<HirLocal>,
    stores: Vec<(HirExpression, HirExpression)>,
}

impl HirVisitor for Declarations {
    fn visit_local(&mut self, local: &HirLocal) {
        self.bindings.push(local.clone());
        walk_local(self, local);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Assign(target, source)
        | HirExpressionKind::ConversioAssign { target, source, .. } = &expr.kind
        {
            self.stores.push(((**target).clone(), (**source).clone()));
        }
        walk_expr(self, expr);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Coordinate {
    variable: Option<DefId>,
    scale: i128,
    offset: i128,
}

fn coordinates(expr: &HirExpression) -> Option<Vec<Coordinate>> {
    match &expr.kind {
        HirExpressionKind::Array(elements) => elements
            .iter()
            .map(|element| match element {
                HirArrayElement::Expr(expr) => coordinate(expr),
                _ => None,
            })
            .collect(),
        HirExpressionKind::Tuple(elements, _) => elements.iter().map(coordinate).collect(),
        _ => Some(vec![coordinate(expr)?]),
    }
}

fn coordinate(expr: &HirExpression) -> Option<Coordinate> {
    if let Some(offset) = super::const_int_value(expr) {
        return Some(Coordinate {
            variable: None,
            scale: 0,
            offset,
        });
    }
    match &expr.kind {
        HirExpressionKind::Path(def) => Some(Coordinate {
            variable: Some(*def),
            scale: 1,
            offset: 0,
        }),
        HirExpressionKind::Widen { operand, .. } => coordinate(operand),
        HirExpressionKind::Conversio {
            source,
            implicit: Some(_),
            ..
        } => coordinate(source),
        _ => None,
    }
}

fn mapped(storage: &Storage, mut coords: Vec<Coordinate>) -> Option<Vec<Coordinate>> {
    // A view maps local coordinates back through each parent, in reverse
    // construction order. Transpose swaps only the trailing pair of axes.
    for transform in storage.transforms.iter().rev() {
        match transform {
            Transform::Transpose if coords.len() >= 2 => {
                let n = coords.len();
                coords.swap(n - 2, n - 1);
            }
            Transform::Slice(start, step) if !coords.is_empty() => {
                coords[0].scale = coords[0].scale.checked_mul(*step)?;
                coords[0].offset = coords[0].offset.checked_mul(*step)?.checked_add(*start)?;
            }
            _ => return None,
        }
    }
    Some(coords)
}

struct Access {
    facts: Facts,
    coords: Option<Vec<Coordinate>>,
    write: bool,
    span: Span,
}

struct BodyProof<'a> {
    aliases: &'a Aliases,
    types: &'a TypeTable,
    interner: &'a Interner,
    owned: FxHashSet<DefId>,
    accesses: Vec<Access>,
    errors: Vec<SemanticError>,
    suppress_reads: bool,
}

impl BodyProof<'_> {
    fn decline(&mut self, span: Span, reason: &str) {
        self.errors.push(
            SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
                .with_arg("issue", ISSUE)
                .with_arg("reason", reason),
        );
    }

    fn is_storage(&self, ty: Option<TypeId>) -> bool {
        let Some(ty) = ty else {
            return false;
        };
        let (_, ty) = self.types.peel_access_marker(ty);
        self.types.is_aliasing_container(ty)
            || matches!(
                self.types.get(ty),
                Type::Struct(_)
                    | Type::Interface(_)
                    | Type::Applied(..)
                    | Type::Param(_)
                    | Type::Record(_)
                    | Type::Tuple(_)
                    | Type::Option(_)
                    | Type::Union(_)
                    | Type::Primitive(
                        crate::Primitive::Ignotum
                            | crate::Primitive::Valor
                            | crate::Primitive::Json
                    )
            )
    }

    fn access(
        &mut self,
        object: &HirExpression,
        index: Option<&HirExpression>,
        write: bool,
        span: Span,
    ) {
        self.accesses.push(Access {
            facts: self.aliases.facts(object, self.types, self.interner),
            coords: index.and_then(coordinates),
            write,
            span,
        });
    }

    fn receiver(&mut self, object: &HirExpression) {
        let before = self.suppress_reads;
        self.suppress_reads = true;
        self.visit_expr(object);
        self.suppress_reads = before;
    }

    fn write(&mut self, target: &HirExpression) {
        match &target.kind {
            HirExpressionKind::Index(object, index) => {
                self.access(object, Some(index), true, target.span);
                self.receiver(object);
                self.visit_expr(index);
            }
            HirExpressionKind::Path(def) => {
                let borrowed = self.aliases.bindings.iter().any(|local| {
                    local.def_id == *def
                        && local.ty.is_some_and(|ty| {
                            self.types.is_mut_view(ty) || self.types.is_ref_view(ty)
                        })
                });
                let facts = self.aliases.facts(target, self.types, self.interner);
                if !self.owned.contains(def)
                    || borrowed
                        && (facts.unknown
                            || facts.storage.is_empty()
                            || facts.storage.iter().any(|s| !self.owned.contains(&s.root)))
                {
                    self.decline(target.span, "outer_local_mutation");
                }
            }
            HirExpressionKind::Field(object, _) => {
                let facts = self.aliases.facts(object, self.types, self.interner);
                if facts.unknown
                    || facts.storage.is_empty()
                    || facts.storage.iter().any(|s| !self.owned.contains(&s.root))
                {
                    self.decline(target.span, "outer_storage_mutation");
                }
                self.receiver(object);
            }
            _ => self.decline(target.span, "unknown_write_address"),
        }
    }
}

impl HirVisitor for BodyProof<'_> {
    fn visit_local(&mut self, local: &HirLocal) {
        walk_local(self, local);
    }

    fn visit_stmt(&mut self, stmt: &HirStatement) {
        if let HirStatementKind::IncDec(inc) = &stmt.kind {
            self.write(&inc.target);
            self.visit_expr(&inc.target);
        } else {
            walk_stmt(self, stmt);
        }
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Assign(target, source) => {
                self.write(target);
                self.visit_expr(source);
            }
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
            } => {
                self.write(target);
                self.visit_expr(source);
                if let Some(recovery) = recovery {
                    self.visit_expr(recovery);
                }
                self.decline(expr.span, "unproved_conversion_effect");
            }
            HirExpressionKind::Index(object, index) => {
                self.access(object, Some(index), false, expr.span);
                self.receiver(object);
                self.visit_expr(index);
            }
            HirExpressionKind::Path(_) | HirExpressionKind::Field(_, _)
                if !self.suppress_reads && self.is_storage(expr.ty) =>
            {
                self.access(expr, None, false, expr.span);
                if let HirExpressionKind::Field(object, _) = &expr.kind {
                    self.receiver(object);
                }
            }
            HirExpressionKind::MethodCall(receiver, name, _, args, _) => {
                let intrinsic =
                    is_access_intrinsic(receiver, *name, args.len(), self.types, self.interner);
                let name = self.interner.resolve(*name);
                if !intrinsic {
                    self.decline(expr.span, "unproved_call_effect");
                    walk_expr(self, expr);
                    return;
                }
                match name {
                    "accipe" if args.len() == 1 => {
                        self.access(receiver, Some(&args[0].expr), false, expr.span)
                    }
                    "ponde" if args.len() == 2 => {
                        self.access(receiver, Some(&args[0].expr), true, expr.span)
                    }
                    "transpone" | "__faber_glyph_transpose" | "sectio" | "forma" | "expande" => {}
                    _ => self.decline(expr.span, "unproved_call_effect"),
                }
                self.receiver(receiver);
                let before = self.suppress_reads;
                self.suppress_reads = false;
                for arg in args {
                    self.visit_expr(&arg.expr);
                }
                self.suppress_reads = before;
            }
            HirExpressionKind::Call(..)
            | HirExpressionKind::Ad { .. }
            | HirExpressionKind::CopyInto { .. }
            | HirExpressionKind::Gradient { .. }
            | HirExpressionKind::Reddet(_)
            | HirExpressionKind::Tacebit(_)
            | HirExpressionKind::Cede(_)
            | HirExpressionKind::Yield(_)
            | HirExpressionKind::Conversio { route: Some(_), .. } => {
                self.decline(expr.span, "unproved_call_effect");
                walk_expr(self, expr);
            }
            HirExpressionKind::Itera { .. }
            | HirExpressionKind::IteraLockstep { .. }
            | HirExpressionKind::IteraRange { .. }
            | HirExpressionKind::Summa { .. }
            | HirExpressionKind::Extremum { .. } => {
                self.decline(expr.span, "nested_or_reduction_integration_pending");
            }
            HirExpressionKind::Clausura(..)
            | HirExpressionKind::Handled { .. }
            | HirExpressionKind::Trap(_)
            | HirExpressionKind::CallDefault { .. }
            | HirExpressionKind::OptionalChain(..) => {
                self.decline(expr.span, "unproved_body_form");
            }
            _ => walk_expr(self, expr),
        }
    }
}

#[derive(Default)]
struct Owned {
    defs: FxHashSet<DefId>,
}
impl HirVisitor for Owned {
    fn visit_def(&mut self, def: DefId, _: Symbol) {
        self.defs.insert(def);
    }
}

fn prove(
    body: &HirBlock,
    source: &HirExpression,
    coords: &[(DefId, Symbol)],
    binding: DefId,
    aliases: &Aliases,
    types: &TypeTable,
    interner: &Interner,
) -> Vec<SemanticError> {
    let mut owned = Owned::default();
    owned.visit_block(body);
    owned.defs.insert(binding);
    let mut proof = BodyProof {
        aliases,
        types,
        interner,
        owned: owned.defs,
        accesses: Vec::new(),
        errors: Vec::new(),
        suppress_reads: false,
    };
    proof.visit_block(body);
    let own: Vec<_> = coords
        .iter()
        .map(|(def, _)| Coordinate {
            variable: Some(*def),
            scale: 1,
            offset: 0,
        })
        .collect();
    let source_facts = aliases.facts(source, types, interner);
    // The iteration's element fetch is a read even when its binder is unused.
    proof.accesses.push(Access {
        facts: source_facts.clone(),
        coords: Some(own.clone()),
        write: false,
        span: source.span,
    });
    let mut writes: FxHashMap<DefId, Vec<Coordinate>> = FxHashMap::default();
    for access in proof.accesses.iter().filter(|a| a.write) {
        if access.facts.unknown || access.facts.storage.is_empty() {
            proof
                .errors
                .push(issue(access.span, "unknown_storage_identity"));
        }
        for storage in &access.facts.storage {
            if proof.owned.contains(&storage.root) {
                continue;
            }
            let physical = access
                .coords
                .clone()
                .and_then(|coords| mapped(storage, coords));
            let expected: Vec<_> = source_facts
                .storage
                .iter()
                .filter(|s| s.root == storage.root)
                .map(|s| mapped(s, own.clone()))
                .collect();
            let valid = !own.is_empty()
                && physical.is_some()
                && if expected.is_empty() {
                    physical == Some(own.clone())
                } else {
                    !source_facts.unknown && expected.iter().all(|e| e.is_some() && *e == physical)
                };
            if !valid {
                proof
                    .errors
                    .push(issue(access.span, "write_not_own_coordinate"));
            }
            if let Some(physical) = physical {
                if writes
                    .get(&storage.root)
                    .is_some_and(|old| *old != physical)
                {
                    proof
                        .errors
                        .push(issue(access.span, "inconsistent_written_coordinate"));
                }
                writes.insert(storage.root, physical);
            }
        }
    }
    for access in proof.accesses.iter().filter(|a| !a.write) {
        if writes.is_empty() {
            continue;
        }
        if access.facts.unknown || access.facts.storage.is_empty() {
            proof
                .errors
                .push(issue(access.span, "unknown_read_storage_identity"));
        }
        for storage in &access.facts.storage {
            if let Some(written) = writes.get(&storage.root) {
                let physical = access
                    .coords
                    .clone()
                    .and_then(|coords| mapped(storage, coords));
                if physical.as_ref() != Some(written) {
                    proof
                        .errors
                        .push(issue(access.span, "written_storage_neighbor_read"));
                }
            }
        }
    }
    proof.errors
}

fn issue(span: Span, reason: &str) -> SemanticError {
    SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
        .with_arg("issue", ISSUE)
        .with_arg("reason", reason)
}

struct Loops<'a> {
    aliases: &'a Aliases,
    types: &'a TypeTable,
    interner: &'a Interner,
    errors: Vec<SemanticError>,
}
impl HirVisitor for Loops<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Itera {
            filum: Some(_),
            coordinates,
            binding,
            iter,
            body,
            ..
        } = &expr.kind
        {
            self.errors.extend(prove(
                body,
                iter,
                coordinates,
                *binding,
                self.aliases,
                self.types,
                self.interner,
            ));
        } else {
            walk_expr(self, expr);
        }
    }
}

#[derive(Default)]
struct HasFilum(bool);
impl HirVisitor for HasFilum {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if matches!(expr.kind, HirExpressionKind::Itera { filum: Some(_), .. }) {
            self.0 = true;
        }
        if !self.0 {
            walk_expr(self, expr);
        }
    }
}

struct Scopes<'a> {
    types: &'a TypeTable,
    interner: &'a Interner,
    errors: Vec<SemanticError>,
}
impl Scopes<'_> {
    fn check_body(&mut self, body: &HirBlock) {
        let mut finder = HasFilum::default();
        finder.visit_block(body);
        if !finder.0 {
            return;
        }
        let aliases = Aliases::build(body, self.types, self.interner);
        let mut loops = Loops {
            aliases: &aliases,
            types: self.types,
            interner: self.interner,
            errors: Vec::new(),
        };
        loops.visit_block(body);
        self.errors.extend(loops.errors);
    }
}
impl HirVisitor for Scopes<'_> {
    fn visit_block(&mut self, body: &HirBlock) {
        self.check_body(body);
    }

    fn visit_function(&mut self, function: &HirFunction) {
        for param in &function.params {
            if let Some(default) = &param.default {
                self.visit_expr(default);
            }
        }
        if let Some(body) = &function.body {
            self.check_body(body);
        }
    }
}

/// All targets share this admission before target-sensitive HIR rewrites.
pub(crate) fn check(hir: &HirModule, types: &TypeTable, interner: &Interner) -> Vec<SemanticError> {
    let mut scopes = Scopes {
        types,
        interner,
        errors: Vec::new(),
    };
    for item in &hir.items {
        scopes.visit_item(item);
    }
    if let Some(entry) = &hir.entry {
        scopes.check_body(entry);
    }
    scopes.errors
}
