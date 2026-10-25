//! Compiler-owned GPU builtin call symbols.
//!
//! These names use ordinary call syntax, but they do not denote user
//! functions. Semantic analysis records their `DefId` so MIR lowering can emit
//! target-neutral device builtin reads.

use crate::{
    FuncSig, IndexExpr, NumericWidth, Primitive, Resolver, Symbol, SymbolKind, TypeId, TypeTable,
};
use radix_hir::DefId;
use radix_lexer::{Interner, Span};
use radix_types::MirKernelBuiltin;

/// Width of vector-valued GPU builtins (`global_invocation_id`, etc.).
const GPU_BUILTIN_VECTOR_WIDTH: u64 = MirKernelBuiltin::VECTOR_WIDTH as u64;

fn gpu_builtin_return_type(builtin: MirKernelBuiltin, types: &mut TypeTable) -> TypeId {
    if builtin.returns_vector() {
        return gpu_vector_builtin_return_type(types);
    }
    types.sized_numeric(Primitive::Numerus, NumericWidth::U32)
}

fn gpu_vector_builtin_return_type(types: &mut TypeTable) -> TypeId {
    let u32_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let width = types.intern_index(IndexExpr::Literal(GPU_BUILTIN_VECTOR_WIDTH));
    types.vector_with_width(u32_ty, width)
}

/// Register zero-argument GPU builtin functions in the value namespace.
///
/// # Panics
///
/// Panics if a builtin's name is already defined in the resolver.
pub fn register(resolver: &mut Resolver, types: &mut TypeTable, interner: &mut Interner) {
    for builtin in MirKernelBuiltin::ALL {
        let name = interner.intern(builtin.spelling());
        if resolver.lookup(name).is_some() {
            continue;
        }
        let def_id = resolver.fresh_builtin_def_id();
        let ret = gpu_builtin_return_type(builtin, types);
        let ty = types.function(FuncSig {
            type_params: Vec::new(),
            params: Vec::new(),
            ret,
            err: None,
            is_async: false,
            is_generator: false,
        });
        let symbol = Symbol {
            def_id,
            name,
            kind: SymbolKind::Function,
            ty: Some(ty),
            mutable: false,
            span: Span::default(),
        };
        resolver
            .define(symbol)
            .expect("GPU builtin names must be unique");
        resolver.register_gpu_builtin(def_id, builtin);
    }
}

/// Return registered GPU builtin call metadata keyed by definition id.
#[must_use]
pub fn builtin_calls(resolver: &Resolver) -> Vec<(DefId, MirKernelBuiltin)> {
    resolver.gpu_builtins().to_vec()
}
