//! Rebuild a `praefixum` result as a HIR literal of the declared type
//! (DECISIONS D8.6).
//!
//! The literal surface is the one every backend already emits for a static:
//! scalar literals, `textus`/`octeti` payloads, list literals, tuples, and
//! typed construction (`Verte` with entries) for `tabula` and `genus`
//! values. Anything else — closures, handles, cursors, non-finite floats,
//! generic `genus` — is `praefixum_result_not_literal`.

use super::evaluation::invalid;
use crate::driver::AnalyzedModule;
use crate::semantic::SemanticError;
use crate::semantic::insere::{octeti_literal_payload, textus_literal_payload};
use radix_hir::{
    DefId, HirArrayElement, HirExpression, HirExpressionKind, HirId, HirItemKind, HirLiteral, HirObjectField,
    HirObjectKey, HirUnOp,
};
use radix_lexer::Span;
use radix_mir_runner::Value;
use radix_mir_runner::value::MapKey;
use radix_types::{Primitive, Type, TypeId};

pub(super) struct LiteralBuilder<'u> {
    unit: &'u mut AnalyzedModule,
    span: Span,
    next_id: u32,
}

impl<'u> LiteralBuilder<'u> {
    pub(super) fn new(unit: &'u mut AnalyzedModule, span: Span) -> Self {
        let next_id = crate::semantic::passes::statica::max_hir_id(&unit.hir) + 1;
        Self { unit, span, next_id }
    }

    fn not_literal(&self) -> SemanticError {
        invalid("praefixum_result_not_literal", self.span)
    }

    fn node(&mut self, kind: HirExpressionKind, ty: TypeId) -> HirExpression {
        let id = HirId(self.next_id);
        self.next_id += 1;
        HirExpression { id, kind, ty: Some(ty), span: self.span }
    }

    fn literal(&mut self, literal: HirLiteral, ty: TypeId) -> HirExpression {
        self.node(HirExpressionKind::Literal(literal), ty)
    }

    fn negated(&mut self, inner: HirExpression, ty: TypeId) -> HirExpression {
        self.node(HirExpressionKind::Unary(HirUnOp::Neg, Box::new(inner)), ty)
    }

    fn resolve(&self, mut ty: TypeId) -> TypeId {
        while let Type::Alias(_, inner) = self.unit.types.get(ty) {
            ty = *inner;
        }
        ty
    }

    /// Build the literal for `value` at the declared type `ty`.
    pub(super) fn build(&mut self, value: &Value, ty: TypeId) -> Result<HirExpression, SemanticError> {
        let resolved = self.resolve(ty);
        match (self.unit.types.get(resolved).clone(), value) {
            (Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _), Value::Int(v)) => {
                let magnitude = self.literal(HirLiteral::Int(v.unsigned_abs()), ty);
                Ok(if *v < 0 { self.negated(magnitude, ty) } else { magnitude })
            }
            (Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _), Value::UInt(v)) => {
                Ok(self.literal(HirLiteral::Int(*v), ty))
            }
            // An `inf` beyond the 64-bit carriers is an exact big literal (ruling 34).
            (Type::SizedNumeric(Primitive::Numerus, _), Value::Magnus(v)) => {
                let magnitude = self.literal(HirLiteral::BigInt(v.abs().to_string().into()), ty);
                Ok(if v.is_negative() {
                    self.negated(magnitude, ty)
                } else {
                    magnitude
                })
            }
            (Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _), Value::Float(v))
                if v.is_finite() =>
            {
                let magnitude = self.literal(HirLiteral::Float(v.abs()), ty);
                Ok(if v.is_sign_negative() && *v != 0.0 {
                    self.negated(magnitude, ty)
                } else {
                    magnitude
                })
            }
            (Type::Primitive(Primitive::Bivalens), Value::Bool(b)) => Ok(self.literal(HirLiteral::Bool(*b), ty)),
            (Type::Primitive(Primitive::Textus), Value::Str(text)) => {
                let payload = self.unit.interner.intern(&textus_literal_payload(text));
                Ok(self.literal(HirLiteral::String(payload), ty))
            }
            (Type::Primitive(Primitive::Octeti), Value::Octeti(bytes)) => {
                let payload = self.unit.interner.intern(&octeti_literal_payload(bytes));
                Ok(self.literal(HirLiteral::Octeti(payload), ty))
            }
            (Type::Option(_) | Type::Union(_), Value::Nil) => Ok(self.literal(HirLiteral::Nil, ty)),
            (Type::Option(inner), value) => self.build(value, inner),
            (Type::Array(element), Value::Array(cells)) => {
                let cells = cells.borrow().clone();
                let elements = cells
                    .iter()
                    .map(|cell| self.build(cell, element).map(HirArrayElement::Expr))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(self.node(HirExpressionKind::Array(elements), ty))
            }
            (Type::Tuple(parts), Value::Tuple(values)) if parts.len() == values.len() => {
                let elements = values
                    .iter()
                    .zip(parts.iter())
                    .map(|(value, part)| self.build(value, *part))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(self.node(HirExpressionKind::Tuple(elements, Some(parts)), ty))
            }
            (Type::Map(key_ty, value_ty), Value::Map(cells)) => {
                let mut entries: Vec<(MapKey, Value)> = cells
                    .borrow()
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                // Maps are unordered; a stable literal keeps builds reproducible.
                entries.sort_by(|(a, _), (b, _)| map_key_order(a, b));
                let text_keys = matches!(self.unit.types.get(self.resolve(key_ty)), Type::Primitive(Primitive::Textus));
                let mut fields = Vec::with_capacity(entries.len());
                for (key, value) in &entries {
                    let key = match key {
                        MapKey::Str(text) if text_keys => {
                            HirObjectKey::String(self.unit.interner.intern(&textus_literal_payload(text)))
                        }
                        other => HirObjectKey::Computed(self.build(&other.to_value(), key_ty)?),
                    };
                    let value = self.build(value, value_ty)?;
                    fields.push(HirObjectField { key, value: Some(value) });
                }
                Ok(self.construction(fields, ty))
            }
            (Type::Struct(def_id), Value::Struct(value_def, cells)) if def_id == *value_def => {
                let fields = self
                    .struct_fields(def_id)
                    .ok_or_else(|| self.not_literal())?;
                let cells = cells.borrow().clone();
                let mut entries = Vec::with_capacity(fields.len());
                for (name, field_ty) in fields {
                    let Some(cell) = cells.get(&name) else {
                        continue;
                    };
                    let value = self.build(cell, field_ty)?;
                    entries.push(HirObjectField { key: HirObjectKey::Ident(name), value: Some(value) });
                }
                Ok(self.construction(entries, ty))
            }
            _ => Err(self.not_literal()),
        }
    }

    /// Typed construction `T { … }`: the placeholder-source `Verte` that
    /// lowering produces for an authored object literal.
    fn construction(&mut self, entries: Vec<HirObjectField>, ty: TypeId) -> HirExpression {
        let unit_ty = self.unit.types.intern(Type::Tuple(Vec::new()));
        let placeholder = self.node(HirExpressionKind::Tuple(Vec::new(), None), unit_ty);
        self.node(
            HirExpressionKind::Verte { source: Box::new(placeholder), target: ty, entries: Some(entries) },
            ty,
        )
    }

    /// Instance fields of a local, non-generic `genus`, in declaration order.
    fn struct_fields(&self, def_id: DefId) -> Option<Vec<(radix_lexer::Symbol, TypeId)>> {
        self.unit
            .hir
            .items
            .iter()
            .find_map(|item| match &item.kind {
                HirItemKind::Struct(strukt) if item.def_id == def_id && strukt.type_params.is_empty() => Some(
                    strukt
                        .fields
                        .iter()
                        .filter(|field| !field.is_static())
                        .map(|field| (field.name, field.ty))
                        .collect(),
                ),
                _ => None,
            })
    }
}

fn map_key_order(a: &MapKey, b: &MapKey) -> std::cmp::Ordering {
    fn rank(key: &MapKey) -> u8 {
        match key {
            MapKey::Int(_) | MapKey::UInt(_) | MapKey::Magnus(_) => 0,
            MapKey::Bool(_) => 1,
            MapKey::Str(_) => 2,
            MapKey::Instans(_, _) => 3,
            MapKey::Unit => 4,
            MapKey::Tuple(_) => 5,
            // Variant and genus keys tie within their own classes; the `_ =>`
            // fallthrough keeps ordering total and reproducible.
            MapKey::Enum(_, _) => 6,
            MapKey::Struct(_, _) => 7,
        }
    }
    match (a, b) {
        (MapKey::Int(x), MapKey::Int(y)) => x.cmp(y),
        (MapKey::Int(_), MapKey::UInt(_)) => std::cmp::Ordering::Less,
        (MapKey::UInt(_), MapKey::Int(_)) => std::cmp::Ordering::Greater,
        (MapKey::UInt(x), MapKey::UInt(y)) => x.cmp(y),
        // `inf` keys beyond the 64-bit carriers: exact order by value.
        (MapKey::Magnus(x), MapKey::Magnus(y)) => x.cmp(y),
        (MapKey::Magnus(x), MapKey::Int(_) | MapKey::UInt(_)) => {
            if x.is_negative() {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            }
        }
        (MapKey::Int(_) | MapKey::UInt(_), MapKey::Magnus(y)) => {
            if y.is_negative() {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Less
            }
        }
        (MapKey::Bool(x), MapKey::Bool(y)) => x.cmp(y),
        (MapKey::Str(x), MapKey::Str(y)) => x.cmp(y),
        (MapKey::Tuple(x), MapKey::Tuple(y)) => x
            .iter()
            .zip(y.iter())
            .map(|(p, q)| map_key_order(p, q))
            .find(|order| order.is_ne())
            .unwrap_or_else(|| x.len().cmp(&y.len())),
        _ => rank(a).cmp(&rank(b)),
    }
}
