//! Compiler-injected builtin genera and types.
//!
//! `gpu` registers device vector builtins used by MIR lowering and GPU probes;
//! registration order is forma → gpu so shared resolver state stays deterministic.

pub mod forma;
pub mod frame_types;
pub mod gpu;

use crate::{Resolver, TypeTable};
use radix_hir::{DefId, HirItem};
use radix_lexer::Interner;

/// Register builtin genera in the resolver before user declaration collection.
pub fn register(resolver: &mut Resolver, types: &mut TypeTable, interner: &mut Interner) {
    forma::register(resolver, types, interner);
    gpu::register(resolver, types, interner);
    register_vertex_shader_locals(resolver, types, interner);
}

/// Register implicit `@vertex` shader input/output identifiers so the
/// resolver does not emit `unknown_identifier` for them. Registration is
/// global; the MIR lowerer validates actual usage per-function stage.
fn register_vertex_shader_locals(
    resolver: &mut Resolver,
    types: &mut TypeTable,
    interner: &mut Interner,
) {
    use crate::{IndexExpr, NumericWidth, Primitive, SymbolKind};

    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let width3 = types.intern_index(IndexExpr::Literal(3));
    let width4 = types.intern_index(IndexExpr::Literal(4));
    let vec3_ty = types.vector_with_width(f32_ty, width3);
    let vec4_ty = types.vector_with_width(f32_ty, width4);
    let span = radix_lexer::Span::default();

    for (spelling, ty, mutable) in [
        ("position", vec3_ty, false),
        ("out_position", vec4_ty, true),
    ] {
        let name = interner.intern(spelling);
        if resolver.lookup(name).is_some() {
            continue;
        }
        let def_id = resolver.fresh_builtin_def_id();
        let _ = resolver.define(crate::Symbol {
            def_id,
            name,
            kind: SymbolKind::Local,
            ty: Some(ty),
            mutable,
            span,
        });
    }
}

/// Register frame stream builtins when the parsed program uses stream `ad`.
pub fn register_frame_types_if_needed(
    program: &radix_syntax::Program,
    resolver: &mut Resolver,
    types: &mut TypeTable,
    interner: &mut Interner,
) {
    if frame_types::program_needs_frame_types(program, interner) {
        frame_types::register(resolver, types, interner);
    }
}

/// Inject builtin HIR items ahead of user declarations.
pub fn inject_hir_items(
    items: &mut Vec<HirItem>,
    program: &radix_syntax::Program,
    resolver: &mut Resolver,
    types: &mut TypeTable,
    interner: &mut Interner,
) {
    if forma::program_uses_forma(program, resolver) {
        forma::inject_hir_item(items, resolver, types, interner);
    }
    if frame_types::program_needs_frame_types(program, interner) {
        frame_types::inject_hir_items(items, resolver, types, interner);
    }
}

/// Return the builtin `forma` definition id when registered.
#[must_use]
pub fn forma_def_id(resolver: &Resolver) -> Option<DefId> {
    forma::def_id(resolver)
}

/// Return whether a parsed program references captured `forma` templates.
#[must_use]
pub fn program_uses_forma(program: &radix_syntax::Program, resolver: &Resolver) -> bool {
    forma::program_uses_forma(program, resolver)
}
