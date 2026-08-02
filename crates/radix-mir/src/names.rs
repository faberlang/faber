//! Shared MIR naming ledger for `llvm_text`, `wasm_text`, and `sexp_text`.
//!
//! TARGET: `llvm_text`, `wasm_text`, `sexp_text`
//! Logical names use one ASCII letter plus the MIR namespace index. Functions keep
//! sanitized source names (`factorial`, `incipit`) with `f{id}` fallbacks.
//!
//! GRAMMAR:
//!   local  :← 'l' decimal
//!   temp   :← 't' decimal
//!   value  :← 'v' decimal
//!   block  :← 'b' decimal
//!   dispatch :← "p0" (wasm/sexp block cursor)

use std::fmt::Write;

use crate::{
    MirBlockId, MirExternalFunctionIdentity, MirFunction, MirFunctionId, MirLocalId, MirProgram,
    MirTempId, MirType, MirValueId,
};
use radix_lexer::{Interner, Symbol};
use radix_types::{Primitive, Type, TypeTable};
use rustc_hash::FxHashMap;

/// Wasm/sexp dispatch local holding the active `b{N}` block id.
pub const DISPATCH: &str = "p0";

/// Wasm loop label for block-dispatch functions.
pub const WASM_DISPATCH_LOOP: &str = "dispatch";

/// Fallback name for synthetic entry functions with no source symbol.
pub const ENTRY_FUNCTION_NAME: &str = "incipit";

const LITERAL_IDENTITY_EMPTY_SUFFIX: &str = "_0_empty";
const IMPORT_COMPONENT_ANON: &str = "anon";
const EXTERNAL_IMPORT_ROOT_MODULE: &str = "root";

pub struct MirNames<'a> {
    program: &'a MirProgram,
    types: &'a TypeTable,
    interner: &'a Interner,
    /// Precomputed function spellings: O(F) once at construction, O(1) per query.
    function_names: FxHashMap<MirFunctionId, String>,
}

impl<'a> MirNames<'a> {
    #[must_use]
    pub fn new(program: &'a MirProgram, types: &'a TypeTable, interner: &'a Interner) -> Self {
        let mut names = Self {
            program,
            types,
            interner,
            function_names: FxHashMap::default(),
        };
        // Precompute all spellings once so `function()` never re-scans the program.
        let function_names = program
            .functions
            .iter()
            .map(|function| (function.id, names.compute_function_name(function)))
            .collect();
        names.function_names = function_names;
        names
    }

    #[must_use]
    pub fn local(id: MirLocalId) -> String {
        format!("l{}", id.0)
    }

    #[must_use]
    pub fn temp(id: MirTempId) -> String {
        format!("t{}", id.0)
    }

    #[allow(dead_code)] // WHY: symmetric id formatter; LLVM/Wasm backends keep per-function value maps today.
    #[must_use]
    pub fn value(id: MirValueId) -> String {
        format!("v{}", id.0)
    }

    #[must_use]
    pub fn block(id: MirBlockId) -> String {
        format!("b{}", id.0)
    }

    #[must_use]
    pub fn function_fallback(id: MirFunctionId) -> String {
        format!("f{}", id.0)
    }

    #[must_use]
    pub fn function(&self, function: &MirFunction) -> String {
        self.function_names
            .get(&function.id)
            .cloned()
            .unwrap_or_else(|| Self::function_fallback(function.id))
    }

    fn compute_function_name(&self, function: &MirFunction) -> String {
        if self.is_entry_function(function) {
            return ENTRY_FUNCTION_NAME.to_owned();
        }
        let base = self.function_base_name(function);
        let entry_uses_base = base == ENTRY_FUNCTION_NAME
            && self
                .program
                .functions
                .iter()
                .any(|other| self.is_entry_function(other));
        let duplicate_user_names = self
            .program
            .functions
            .iter()
            .filter(|other| {
                !self.is_entry_function(other) && self.function_base_name(other) == base
            })
            .count();
        if entry_uses_base || duplicate_user_names > 1 {
            if let Some(name) = self.monomorphized_function_name(function, &base) {
                return name;
            }
            format!("{base}_f{}", function.id.0)
        } else {
            base
        }
    }

    #[must_use]
    pub fn is_entry_function(&self, function: &MirFunction) -> bool {
        function.source.is_none()
            && function.name.is_none()
            && function.params.is_empty()
            && self.is_vacuum_ty(function.return_ty)
    }

    #[must_use]
    pub fn explicit_entry_function(&self) -> Option<&'a MirFunction> {
        self.program
            .functions
            .iter()
            .filter(|function| self.is_entry_function(function))
            .max_by_key(|function| function.id.0)
    }

    #[must_use]
    pub fn entry_function(&self) -> Option<&'a MirFunction> {
        let incipit = self.explicit_entry_function();
        if incipit.is_some() {
            return incipit;
        }
        let vacuum = self.types.primitive(Primitive::Vacuum);
        self.program
            .functions
            .iter()
            .filter(|function| {
                function.params.is_empty() && function.return_ty.semantic_id() == vacuum
            })
            .max_by_key(|function| function.id.0)
    }

    fn function_base_name(&self, function: &MirFunction) -> String {
        function.name.map_or_else(
            || Self::function_fallback(function.id),
            |symbol| sanitize_name(self.interner.resolve(symbol)),
        )
    }

    fn monomorphized_function_name(&self, function: &MirFunction, base: &str) -> Option<String> {
        let source = function.source?;
        let same_source_duplicate = self
            .program
            .functions
            .iter()
            .filter(|other| other.source == Some(source) && self.function_base_name(other) == base)
            .count()
            > 1;
        if !same_source_duplicate {
            return None;
        }
        let mut parts = function
            .params
            .iter()
            .map(|param| param.ty.semantic_id().0.to_string())
            .collect::<Vec<_>>();
        parts.push(function.return_ty.semantic_id().0.to_string());
        if let Some(error_ty) = function.error_ty {
            parts.push(error_ty.semantic_id().0.to_string());
        }
        Some(format!("{base}__t{}", parts.join("_")))
    }

    fn is_vacuum_ty(&self, ty: MirType) -> bool {
        match self.types.get(ty.semantic_id()) {
            Type::Primitive(Primitive::Vacuum) => true,
            Type::Alias(_, target) => self.is_vacuum_ty(MirType::semantic(*target)),
            _ => false,
        }
    }
}

#[must_use]
pub fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[must_use]
pub fn llvm_local_slot(id: MirLocalId) -> String {
    format!("%{}.addr", MirNames::local(id))
}

#[must_use]
pub fn llvm_temp_slot(id: MirTempId) -> String {
    format!("%{}.addr", MirNames::temp(id))
}

pub fn regex_literal_flags_suffix(flags: Option<Symbol>) -> String {
    flags.map_or_else(String::new, |flags| format!("_flags_{}", flags.0))
}

#[must_use]
pub fn octeti_literal_identity(bytes: &[u8]) -> String {
    literal_bytes_identity("octeti", bytes)
}

#[must_use]
pub fn text_literal_identity(text: &str) -> String {
    literal_bytes_identity("text", text.as_bytes())
}

#[must_use]
pub fn ascii_literal_identity(decoded: &str) -> String {
    literal_bytes_identity("ascii", decoded.as_bytes())
}

#[must_use]
pub fn regex_pattern_literal_identity(pattern: &str) -> String {
    literal_bytes_identity("regex_pattern", pattern.as_bytes())
}

#[must_use]
pub fn regex_flags_literal_identity(flags: &str) -> String {
    literal_bytes_identity("regex_flags", flags.as_bytes())
}

fn literal_bytes_identity(family: &str, bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return format!("{family}{LITERAL_IDENTITY_EMPTY_SUFFIX}");
    }
    let mut identity = format!("{family}_{}", bytes.len());
    for byte in bytes {
        identity.push('_');
        write!(identity, "{byte:02x}").unwrap();
    }
    identity
}

#[must_use]
pub fn external_function_import_name(identity: &MirExternalFunctionIdentity) -> String {
    let mut parts = vec![
        "external".to_owned(),
        "product".to_owned(),
        import_component(&identity.product),
        "module".to_owned(),
    ];
    if identity.module_path.is_empty() {
        parts.push(EXTERNAL_IMPORT_ROOT_MODULE.to_owned());
    } else {
        parts.extend(
            identity
                .module_path
                .iter()
                .map(|part| import_component(part)),
        );
    }
    parts.push("func".to_owned());
    parts.push(import_component(&identity.name));
    parts.join("_")
}

fn import_component(component: &str) -> String {
    let sanitized = sanitize_name(component);
    if sanitized.is_empty() {
        IMPORT_COMPONENT_ANON.to_owned()
    } else {
        sanitized
    }
}

#[must_use]
pub fn llvm_block_ref(id: MirBlockId) -> String {
    format!("%{}", MirNames::block(id))
}
