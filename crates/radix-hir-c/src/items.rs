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
    DefId, HirConst, HirExpressionKind, HirFunction, HirItem, HirItemKind, HirModule, HirParamMode,
    HirStatementKind,
};
use rustc_hash::FxHashSet;

/// A function the program defines; a genus method is one with a receiver.
pub(crate) struct Function {
    pub(crate) name: String,
    pub(crate) params: Vec<Ty>,
    /// `None` for a function with no result (`void`).
    pub(crate) ret: Option<Ty>,
    /// The genus a method belongs to; its receiver follows the arena.
    pub(crate) receiver: Option<DefId>,
}

/// What a `return` in the body being written means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Returns {
    /// The entry block: `return 0;` ends the program successfully.
    Main,
    Void,
    Value(Ty),
}

/// One function's text before its storage class is known: whether it carries
/// `__attribute__((unused))` depends on calls written later (the entry block
/// and later functions), so the text is held without its `static` prefix.
pub(crate) struct FunctionText {
    pub(crate) def_id: DefId,
    /// `<ret> f_name(<unnamed params>)`, no terminator.
    prototype: String,
    /// `<ret> f_name(<named params>) {…}\n`.
    definition: String,
}

/// The file-scope text, in the order the translation unit needs it.
#[derive(Default)]
pub(crate) struct Items {
    pub(crate) genera: String,
    pub(crate) constants: String,
    /// Assignments to the module constants that cannot be initialised at file
    /// scope (they allocate in the arena `main` owns); they run first in `main`.
    pub(crate) initializers: String,
    functions: Vec<FunctionText>,
}

impl Items {
    /// One prototype per function. A function nothing calls carries
    /// `__attribute__((unused))`: library modules define many, and
    /// `-Wunused-function` is not undefined behavior.
    pub(crate) fn prototypes(&self, called: &FxHashSet<DefId>) -> String {
        self.functions
            .iter()
            .map(|function| {
                format!(
                    "{}{};\n",
                    storage(called, function.def_id),
                    function.prototype
                )
            })
            .collect()
    }

    /// Every function definition, separated by blank lines.
    pub(crate) fn definitions(&self, called: &FxHashSet<DefId>) -> String {
        self.functions
            .iter()
            .map(|function| {
                format!(
                    "{}{}\n",
                    storage(called, function.def_id),
                    function.definition
                )
            })
            .collect()
    }
}

fn storage(called: &FxHashSet<DefId>, def_id: DefId) -> &'static str {
    if called.contains(&def_id) {
        "static "
    } else {
        "static __attribute__((unused)) "
    }
}

impl Emitter<'_> {
    /// Collect genera first (so their types classify and spell everywhere),
    /// then constants and every function and method signature, then write
    /// the file-scope text.
    pub(crate) fn items(&mut self, hir: &HirModule) -> Result<Items, CodegenError> {
        let mut order = Vec::new();
        for item in &hir.items {
            if let HirItemKind::Struct(genus) = &item.kind {
                if !item.annotations.is_empty() {
                    return Err(unsupported("annotation"));
                }
                self.collect_genus(item.def_id, genus)?;
                order.push(item.def_id);
            }
        }
        let mut items = Items::default();
        let mut functions: Vec<(DefId, &HirFunction)> = Vec::new();
        for item in &hir.items {
            if !item.annotations.is_empty() {
                return Err(unsupported("annotation"));
            }
            match &item.kind {
                HirItemKind::Function(function) => {
                    self.signature(item.def_id, function, None)?;
                    functions.push((item.def_id, &**function));
                }
                HirItemKind::Constant(constant) => {
                    let (declaration, initializer) = self.constant(item, constant)?;
                    items.constants.push_str(&declaration);
                    items.initializers.push_str(&initializer);
                }
                HirItemKind::Struct(genus) => {
                    for method in &genus.methods {
                        self.signature(method.def_id, &method.func, Some(item.def_id))?;
                        functions.push((method.def_id, &method.func));
                    }
                }
                HirItemKind::Enum(_) => return Err(unsupported("enum declaration")),
                HirItemKind::Interface(_) => return Err(unsupported("interface declaration")),
                HirItemKind::TypeAlias(_) => return Err(unsupported("type alias")),
                HirItemKind::Schema(_) => return Err(unsupported("schema declaration")),
                HirItemKind::Import(_) => return Err(unsupported("import (library modules)")),
            }
        }
        items.genera = self.genus_declarations(&order)?;
        for (def_id, function) in functions {
            items.functions.push(FunctionText {
                def_id,
                prototype: self.prototype(def_id)?,
                definition: self.definition(def_id, function)?,
            });
        }
        Ok(items)
    }

    /// A module constant: `static const` with its folded value, registered so
    /// reads of an integer constant fold too. Returns the file-scope
    /// declaration and the assignment `main` runs first, which is empty unless
    /// the value is built at run time: a list or genus, or a text, boolean or
    /// float expression that is not a literal, allocates or calls, and the
    /// arena that serves it belongs to `main`.
    fn constant(
        &mut self,
        item: &HirItem,
        constant: &HirConst,
    ) -> Result<(String, String), CodegenError> {
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
        let literal = value.constant.is_some()
            || matches!(constant.value.kind, HirExpressionKind::Literal(_));
        let (declaration, initializer) = if literal {
            (
                format!(
                    "static {} = {};\n",
                    self.declare(class, &name, false)?,
                    value.text
                ),
                String::new(),
            )
        } else {
            (
                format!("static {};\n", self.declare(class, &name, true)?),
                format!("    {name} = {};\n", value.text),
            )
        };
        self.locals.insert(
            item.def_id,
            Local {
                name,
                ty: class,
                constant: value.constant,
            },
        );
        Ok((declaration, initializer))
    }

    /// Check a function against the subset and record its C signature.
    fn signature(
        &mut self,
        def_id: DefId,
        function: &HirFunction,
        receiver: Option<DefId>,
    ) -> Result<(), CodegenError> {
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
            if param.optional || param.default.is_some() {
                return Err(unsupported("optional parameter"));
            }
            let class = classify(self.types, param.ty)?;
            self.need_class(class);
            // `ref` / `mut` on a list or genus passes the caller's pointer,
            // which is what an unmarked one passes too.
            let shared = matches!(class, Ty::List(_) | Ty::Genus(_));
            match param.mode {
                HirParamMode::Owned => {}
                HirParamMode::De | HirParamMode::In if shared => {}
                HirParamMode::De | HirParamMode::In | HirParamMode::Ex | HirParamMode::Axis => {
                    return Err(unsupported(
                        "parameter access marker on a scalar, own, or axis",
                    ));
                }
            }
            params.push(class);
        }
        let Some(ret_ty) = function.ret_ty else {
            return Err(unsupported("function without a resolved return type"));
        };
        let ret = returns(self.types, ret_ty)?;
        // The result type is spelled in the prototype, so a program whose
        // only use of `bool` or `int64_t` is a signature still needs the header.
        if let Some(class) = ret {
            self.need_class(class);
        }
        let raw = self.names.resolve_symbol(function.name).to_owned();
        let prefix = match receiver {
            Some(genus) => match self.genera.get(&genus) {
                Some(genus) => genus.c_name.clone(),
                None => return Err(unsupported("method of a genus outside the program")),
            },
            None => "f".to_owned(),
        };
        let name = self.items_scope.claim(&format!("{prefix}_{raw}"));
        self.functions.insert(
            def_id,
            Function {
                name,
                params,
                ret,
                receiver,
            },
        );
        Ok(())
    }

    /// `<ret> f_name(<params>)`, without storage class or terminator.
    /// Parameter names come from the definition, so the prototype is claimed
    /// there.
    fn prototype(&self, def_id: DefId) -> Result<String, CodegenError> {
        let Some(function) = self.functions.get(&def_id) else {
            return Err(unsupported("function without a recorded signature"));
        };
        let ret = match function.ret {
            Some(ty) => self.c_type(ty)?,
            None => "void".to_owned(),
        };
        let mut slots = Vec::with_capacity(function.params.len() + 2);
        if self.arena {
            slots.push("faber_arena *".to_owned());
        }
        if let Some(genus) = function.receiver {
            slots.push(self.c_type(Ty::Genus(genus))?);
        }
        for ty in &function.params {
            slots.push(self.c_type(*ty)?);
        }
        let params = if slots.is_empty() {
            "void".to_owned()
        } else {
            slots.join(", ")
        };
        let space = if ret.ends_with('*') { "" } else { " " };
        Ok(format!("{ret}{space}{}({params})", function.name))
    }

    fn definition(
        &mut self,
        def_id: DefId,
        function: &HirFunction,
    ) -> Result<String, CodegenError> {
        let Some(signature) = self.functions.get(&def_id) else {
            return Err(unsupported("function without a recorded signature"));
        };
        let (name, param_types, ret, receiver) = (
            signature.name.clone(),
            signature.params.clone(),
            signature.ret,
            signature.receiver,
        );
        self.scope = Namer::program_scope();
        self.loops = 0;
        self.current = Some(def_id);
        self.returns = ret.map_or(Returns::Void, Returns::Value);
        let mut params = Vec::with_capacity(function.params.len() + 2);
        if self.arena {
            params.push("faber_arena *faber_a".to_owned());
        }
        // `self` lowers to a path to the genus's own definition.
        if let Some(genus) = receiver {
            params.push(self.declare(Ty::Genus(genus), "faber_self", true)?);
            self.locals.insert(
                genus,
                Local {
                    name: "faber_self".to_owned(),
                    ty: Ty::Genus(genus),
                    constant: None,
                },
            );
        }
        for (param, ty) in function.params.iter().zip(param_types) {
            let Some(kind) = self.names.binding_kind(param.def_id) else {
                return Err(unsupported("parameter the name catalog does not classify"));
            };
            let raw = self.names.resolve_symbol(param.name);
            let c_name = self.scope.claim(&prefixed_binding_name(kind, raw));
            self.need_class(ty);
            params.push(self.declare(ty, &c_name, true)?);
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
        // Every function takes the arena; one that neither allocates nor calls
        // still declares it, so the parameter is marked used.
        if self.arena && !crate::mentions_arena(&body) {
            body.insert_str(0, "    (void)faber_a;\n");
        }
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
        let ret_text = match ret {
            Some(ty) => self.c_type(ty)?,
            None => "void".to_owned(),
        };
        let space = if ret_text.ends_with('*') { "" } else { " " };
        let params = if params.is_empty() {
            "void".to_owned()
        } else {
            params.join(", ")
        };
        Ok(format!("{ret_text}{space}{name}({params}) {{\n{body}}}\n"))
    }
}
