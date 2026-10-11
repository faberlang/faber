//! Transport field-default expressions with their own portable type and text slots.
//!
//! HIR arena indices cannot cross a file interface unchanged. The snapshot
//! rewrites every admitted reference into a snapshot-local slot; import rebuilds
//! those references in the consumer arenas before exposing HIR to lowering.

use crate::file_interface::{
    FileInterfaceError, FileInterfaceResult, InterfaceLibraryIdentity, InterfaceTypeSnapshot,
    import_interface_type_with_resolver, snapshot_interface_type_with_identity,
};
use crate::{Resolver, TypeId, TypeTable};
use radix_hir::{HirArrayElement, HirExpression, HirExpressionKind, HirLiteral, HirObjectKey};
use radix_lexer::{Interner, Symbol};

#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceFieldDefault {
    expression: HirExpression,
    types: Vec<InterfaceTypeSnapshot>,
    text: Vec<String>,
}

impl InterfaceFieldDefault {
    /// Snapshot the definition-free compile-time expression surface.
    ///
    /// # Errors
    /// Returns an error for unsupported expressions or nonportable types.
    pub fn snapshot(
        expression: &HirExpression,
        types: &TypeTable,
        interner: &Interner,
        resolver: &Resolver,
        identity: Option<&InterfaceLibraryIdentity>,
    ) -> FileInterfaceResult<Self> {
        let mut expression = expression.clone();
        let mut snapshots = Vec::new();
        let mut text = Vec::new();
        rewrite_default(
            &mut expression,
            &mut |ty| {
                let snapshot = snapshot_interface_type_with_identity(
                    *ty, types, interner, resolver, identity,
                )?;
                *ty = TypeId(snapshots.len() as u32);
                snapshots.push(snapshot);
                Ok(())
            },
            &mut |symbol| {
                let value = interner.resolve(*symbol).to_owned();
                *symbol = Symbol(text.len() as u32);
                text.push(value);
            },
        )?;
        Ok(Self {
            expression,
            types: snapshots,
            text,
        })
    }

    /// Rebuild a default in the consumer's type and text arenas.
    ///
    /// # Errors
    /// Returns an error if a referenced nominal cannot be resolved.
    pub fn import(
        &self,
        types: &mut TypeTable,
        interner: &mut Interner,
        resolver: &Resolver,
    ) -> FileInterfaceResult<HirExpression> {
        let imported_types = self
            .types
            .iter()
            .map(|snapshot| {
                import_interface_type_with_resolver(snapshot, types, interner, resolver)
            })
            .collect::<FileInterfaceResult<Vec<_>>>()?;
        let imported_text: Vec<_> = self.text.iter().map(|text| interner.intern(text)).collect();
        let mut expression = self.expression.clone();
        rewrite_default(
            &mut expression,
            &mut |ty| {
                *ty = imported_types[ty.0 as usize];
                Ok(())
            },
            &mut |symbol| *symbol = imported_text[symbol.0 as usize],
        )?;
        Ok(expression)
    }
}

/// Only definition-free forms may travel without a linked definition map.
/// The same walker snapshots and imports, so an embedded target or text symbol
/// cannot be carried in one direction but forgotten in the other.
fn rewrite_default(
    expression: &mut HirExpression,
    ty: &mut impl FnMut(&mut TypeId) -> FileInterfaceResult<()>,
    text: &mut impl FnMut(&mut Symbol),
) -> FileInterfaceResult<()> {
    if let Some(expression_ty) = &mut expression.ty {
        ty(expression_ty)?;
    }
    match &mut expression.kind {
        HirExpressionKind::Literal(literal) => rewrite_literal(literal, text),
        HirExpressionKind::Vacua => {}
        HirExpressionKind::Unary(_, inner) => rewrite_default(inner, ty, text)?,
        HirExpressionKind::Binary(_, left, right) => {
            rewrite_default(left, ty, text)?;
            rewrite_default(right, ty, text)?;
        }
        HirExpressionKind::Array(items) => {
            for item in items {
                match item {
                    HirArrayElement::Expr(inner) | HirArrayElement::Spread(inner) => {
                        rewrite_default(inner, ty, text)?;
                    }
                    HirArrayElement::Infer(_) => {
                        return Err(FileInterfaceError::FieldDefaultUnsupported);
                    }
                }
            }
        }
        HirExpressionKind::Tuple(items, arguments) => {
            for item in items {
                rewrite_default(item, ty, text)?;
            }
            if let Some(arguments) = arguments {
                for argument in arguments {
                    ty(argument)?;
                }
            }
        }
        HirExpressionKind::Scriptum(template, items) => {
            text(template);
            for item in items {
                rewrite_default(item, ty, text)?;
            }
        }
        HirExpressionKind::Verte {
            source,
            target,
            entries,
        } => {
            ty(target)?;
            rewrite_default(source, ty, text)?;
            if let Some(entries) = entries {
                for entry in entries {
                    match &mut entry.key {
                        HirObjectKey::Ident(name) | HirObjectKey::String(name) => text(name),
                        HirObjectKey::Computed(inner) | HirObjectKey::Spread(inner) => {
                            rewrite_default(inner, ty, text)?;
                        }
                    }
                    if let Some(value) = &mut entry.value {
                        rewrite_default(value, ty, text)?;
                    }
                }
            }
        }
        _ => return Err(FileInterfaceError::FieldDefaultUnsupported),
    }
    Ok(())
}

fn rewrite_literal(literal: &mut HirLiteral, text: &mut impl FnMut(&mut Symbol)) {
    match literal {
        HirLiteral::String(value) | HirLiteral::Ascii(value) | HirLiteral::Octeti(value) => {
            text(value);
        }
        HirLiteral::FloatText { text: value, .. } => text(value),
        HirLiteral::Regex(pattern, flags) => {
            text(pattern);
            if let Some(flags) = flags {
                text(flags);
            }
        }
        HirLiteral::Int(_)
        | HirLiteral::BigInt(_)
        | HirLiteral::Float(_)
        | HirLiteral::JsonValor(_)
        | HirLiteral::Bool(_)
        | HirLiteral::Nil => {}
    }
}
