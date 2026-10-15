//! File-scope items: functions and module constants.
//!
//! Every function signature is collected before any body is written, and each
//! function gets a prototype, so calls and recursion work in any source order.
//! C has one global namespace shared with the C library, so item names take a
//! prefix the way bindings do (`f_` for functions, `k_` for constants; locals
//! and parameters are `l_` / `p_`), and nothing C reserves starts with one.

use crate::error::unsupported;
use crate::expr::Value;
use crate::names::Namer;
use crate::ty::{Ty, classify, returns};
use crate::{Emitter, Local};
use radix_codegen_shared::CodegenError;
use radix_codegen_shared::names::prefixed_binding_name;
use radix_hir::{
    DefId, HirConst, HirFunction, HirItem, HirItemKind, HirModule, HirParamMode, HirStatementKind,
};

/// A function the program defines.
pub(crate) struct Function {
    pub(crate) name: String,
    pub(crate) params: Vec<Ty>,
    /// `None` for a function with no result (`void`).
    pub(crate) ret: Option<Ty>,
}

/// What a `return` in the body being written means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Returns {
    /// The entry block: `return 0;` ends the program successfully.
    Main,
    Void,
    Value(Ty),
}

/// The C declaration of one parameter or prototype slot.
fn c_decl(ty: Ty, name: &str) -> String {
    let c_type = ty.c_type();
    if c_type.ends_with('*') {
        format!("{c_type}{name}")
    } else {
        format!("{c_type} {name}")
    }
}

impl Emitter<'_> {
    /// Collect constants and function signatures, then write the file-scope
    /// text: constants, prototypes, and function definitions.
    pub(crate) fn items(&mut self, hir: &HirModule) -> Result<String, CodegenError> {
        let mut constants = String::new();
        let mut functions = Vec::new();
        for item in &hir.items {
            if !item.annotations.is_empty() {
                return Err(unsupported("annotation"));
            }
            match &item.kind {
                HirItemKind::Function(function) => {
                    self.signature(item.def_id, function)?;
                    functions.push((item, function));
                }
                HirItemKind::Constant(constant) => {
                    let line = self.constant(item, constant)?;
                    constants.push_str(&line);
                }
                HirItemKind::Struct(_) => return Err(unsupported("genus declaration")),
                HirItemKind::Enum(_) => return Err(unsupported("enum declaration")),
                HirItemKind::Interface(_) => return Err(unsupported("interface declaration")),
                HirItemKind::TypeAlias(_) => return Err(unsupported("type alias")),
                HirItemKind::Schema(_) => return Err(unsupported("schema declaration")),
                HirItemKind::Import(_) => return Err(unsupported("import (library modules)")),
            }
        }
        let mut out = String::new();
        if !constants.is_empty() {
            out.push_str(&constants);
            out.push('\n');
        }
        let mut prototypes = String::new();
        let mut definitions = String::new();
        for (item, function) in functions {
            prototypes.push_str(&self.prototype(item.def_id)?);
            prototypes.push_str(";\n");
            definitions.push_str(&self.definition(item.def_id, function)?);
            definitions.push('\n');
        }
        if !prototypes.is_empty() {
            out.push_str(&prototypes);
            out.push('\n');
            out.push_str(&definitions);
        }
        Ok(out)
    }

    /// A module constant: `static const` with its folded value, registered so
    /// reads of an integer constant fold too.
    fn constant(&mut self, item: &HirItem, constant: &HirConst) -> Result<String, CodegenError> {
        if constant.statica.is_none() || constant.mutable || constant.is_await {
            return Err(unsupported("top-level store that is not a module constant"));
        }
        let Some(ty) = constant.ty.or(constant.value.ty) else {
            return Err(unsupported("module constant without a resolved type"));
        };
        let class = classify(self.types, ty)?;
        let value = self.expr(&constant.value)?;
        if value.value != Value::from(class) || (class == Ty::Int && value.constant.is_none()) {
            return Err(unsupported("module constant without a folded value"));
        }
        self.need_class(class);
        let raw = self.names.resolve_symbol(constant.name);
        let name = self.items_scope.claim(&format!("k_{raw}"));
        let line = format!("static {} = {};\n", class.declare(&name, false), value.text);
        self.locals.insert(
            item.def_id,
            Local {
                name,
                ty: class,
                constant: value.constant,
            },
        );
        Ok(line)
    }

    /// Check a function against the subset and record its C signature.
    fn signature(&mut self, def_id: DefId, function: &HirFunction) -> Result<(), CodegenError> {
        if !function.type_params.is_empty() {
            return Err(unsupported("generic function"));
        }
        if function.is_async || function.is_generator {
            return Err(unsupported("async or generator function"));
        }
        if function.is_kernel
            || function.nucleum_fragment
            || function.is_vertex
            || function.is_fragment
        {
            return Err(unsupported("device function"));
        }
        if function.err_ty.is_some() {
            return Err(unsupported("error channel function"));
        }
        if function.cli_args.is_some() || function.test.is_some() {
            return Err(unsupported("command or test function"));
        }
        if function.body.is_none() {
            return Err(unsupported("function without a body"));
        }
        let mut params = Vec::with_capacity(function.params.len());
        for param in &function.params {
            if param.mode != HirParamMode::Owned {
                return Err(unsupported("parameter access marker"));
            }
            if param.optional || param.default.is_some() {
                return Err(unsupported("optional parameter"));
            }
            params.push(classify(self.types, param.ty)?);
        }
        let Some(ret_ty) = function.ret_ty else {
            return Err(unsupported("function without a resolved return type"));
        };
        let ret = returns(self.types, ret_ty)?;
        let raw = self.names.resolve_symbol(function.name);
        let name = self.items_scope.claim(&format!("f_{raw}"));
        self.functions
            .insert(def_id, Function { name, params, ret });
        Ok(())
    }

    /// `static <ret> f_name(<params>)`, without the terminator. Parameter
    /// names come from the definition, so the prototype is claimed there.
    fn prototype(&self, def_id: DefId) -> Result<String, CodegenError> {
        let Some(function) = self.functions.get(&def_id) else {
            return Err(unsupported("function without a recorded signature"));
        };
        let ret = function.ret.map_or("void", Ty::c_type);
        let params = if function.params.is_empty() {
            "void".to_owned()
        } else {
            function
                .params
                .iter()
                .map(|ty| ty.c_type().to_owned())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let space = if ret.ends_with('*') { "" } else { " " };
        Ok(format!("static {ret}{space}{}({params})", function.name))
    }

    fn definition(
        &mut self,
        def_id: DefId,
        function: &HirFunction,
    ) -> Result<String, CodegenError> {
        let Some(signature) = self.functions.get(&def_id) else {
            return Err(unsupported("function without a recorded signature"));
        };
        let (name, param_types, ret) = (
            signature.name.clone(),
            signature.params.clone(),
            signature.ret,
        );
        self.scope = Namer::program_scope();
        self.loops = 0;
        self.returns = ret.map_or(Returns::Void, Returns::Value);
        let mut params = Vec::with_capacity(function.params.len());
        for (param, ty) in function.params.iter().zip(param_types) {
            let Some(kind) = self.names.binding_kind(param.def_id) else {
                return Err(unsupported("parameter the name catalog does not classify"));
            };
            let raw = self.names.resolve_symbol(param.name);
            let c_name = self.scope.claim(&prefixed_binding_name(kind, raw));
            self.need_class(ty);
            params.push(c_decl(ty, &c_name));
            self.locals.insert(
                param.def_id,
                Local {
                    name: c_name,
                    ty,
                    constant: None,
                },
            );
        }
        let mut body = String::new();
        let Some(block) = &function.body else {
            return Err(unsupported("function without a body"));
        };
        self.block(&mut body, block)?;
        // Typecheck proves every path of a function with a result returns; C
        // cannot always see that (`-Wreturn-type`), so a body that does not end
        // in `return` gets an unreachable trap rather than undefined behavior.
        let ends_in_return = block
            .statements
            .last()
            .is_some_and(|statement| matches!(statement.kind, HirStatementKind::Redde(_)));
        if ret.is_some() && !ends_in_return {
            self.support.need(crate::helpers::Helper::Trap);
            body.push_str("    faber_trap(\"function ended without a return\");\n");
        }
        let ret_text = ret.map_or("void", Ty::c_type);
        let space = if ret_text.ends_with('*') { "" } else { " " };
        let params = if params.is_empty() {
            "void".to_owned()
        } else {
            params.join(", ")
        };
        Ok(format!(
            "static {ret_text}{space}{name}({params}) {{\n{body}}}\n"
        ))
    }
}
