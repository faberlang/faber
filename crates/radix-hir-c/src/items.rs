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
use crate::ty::{Inner, Ty, classify, returns};
use crate::{Emitter, Local};
use handroll::FxHashSet;
use radix_codegen_shared::CodegenError;
use radix_codegen_shared::names::prefixed_binding_name;
use radix_hir::{
    DefId, HirConst, HirExpression, HirExpressionKind, HirFunction, HirItem, HirItemKind,
    HirModule, HirParamMode, HirStatementKind,
};
use std::fmt::Write as _;

/// A function the program defines; a genus method is one with a receiver.
pub(crate) struct Function {
    pub(crate) name: String,
    pub(crate) params: Vec<Ty>,
    /// Per parameter, whether its C storage is a pointer to the caller's cell:
    /// a `ref`/`mut` scalar (or ref optional scalar) parameter. Parallel to
    /// `params`; the call site passes an address, and the body dereferences.
    pub(crate) pointee: Vec<bool>,
    /// Per parameter, what a call that omits its argument passes. Parallel to
    /// `params`; the runner fills a default at the call site, or none for a
    /// plain `optional` slot (`lower_direct_call_args`).
    pub(crate) omissions: Vec<Omission>,
    /// `None` for a function with no result (`void`).
    pub(crate) ret: Option<Ty>,
    /// `⇥ E`'s C carrier. Error payloads must have a pointer representation
    /// because `faber_chan.value` is an unowned `const void *`.
    pub(crate) err: Option<Ty>,
    /// The genus a method belongs to; its receiver follows the arena.
    pub(crate) receiver: Option<DefId>,
}

/// What fills an argument a call leaves out, per parameter.
#[derive(Debug, Clone)]
pub(crate) enum Omission {
    /// The call must provide it.
    Required,
    /// `optional` with no default: the call passes none.
    Omitted,
    /// `optional coalesce <expr>`: the call evaluates the default.
    Default(HirExpression),
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
    pub(crate) genus_order: Vec<DefId>,
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
        let mut out = String::new();
        for function in &self.functions {
            writeln!(
                out,
                "{}{};",
                storage(called, function.def_id),
                function.prototype
            )
            .ok();
        }
        out
    }

    /// Every function definition, separated by blank lines.
    pub(crate) fn definitions(&self, called: &FxHashSet<DefId>) -> String {
        let mut out = String::new();
        for function in &self.functions {
            writeln!(
                out,
                "{}{}",
                storage(called, function.def_id),
                function.definition
            )
            .ok();
        }
        out
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
            match &item.kind {
                HirItemKind::Struct(genus) => {
                    if item
                        .annotations
                        .iter()
                        .any(|a| !self.ignorable_annotation(a))
                    {
                        return Err(unsupported("annotation"));
                    }
                    self.collect_genus(item.def_id, genus)?;
                    order.push(item.def_id);
                }
                HirItemKind::Enum(declaration) => {
                    if item
                        .annotations
                        .iter()
                        .any(|a| !self.ignorable_annotation(a))
                    {
                        return Err(unsupported("annotation"));
                    }
                    self.collect_enum(item.def_id, declaration)?;
                    order.push(item.def_id);
                }
                _ => {}
            }
        }
        let mut items = Items::default();
        let mut functions: Vec<(DefId, &HirFunction)> = Vec::new();
        for item in &hir.items {
            if item
                .annotations
                .iter()
                .any(|a| !self.ignorable_annotation(a))
            {
                return Err(unsupported("annotation"));
            }
            match &item.kind {
                // A test function is not part of the program: `faber run`
                // runs the entry block only, and `faber test` is another route.
                HirItemKind::Function(function) if function.test.is_some() => {}
                // A generic function has no C body until it is instantiated at
                // a concrete type. One nothing calls contributes nothing to the
                // program (`faber run` runs the entry only), so it is skipped;
                // a call to one is refused where it is written.
                HirItemKind::Function(function) if !function.type_params.is_empty() => {
                    self.generics.insert(item.def_id);
                }
                HirItemKind::Function(function) => {
                    self.signature(item.def_id, function, None)?;
                    functions.push((item.def_id, &**function));
                }
                // Constants wait for every signature: a value built with a
                // genus's `creo` hook calls a method.
                HirItemKind::Constant(_)
                // Collected with the genera, whose types it shares.
                | HirItemKind::Enum(_)
                // An alias is resolved in the type table; a use of one that
                // names an unsupported type is rejected where it is used.
                | HirItemKind::TypeAlias(_)
                // A schema is an experimental application-owned heading with no
                // runtime effect; `faber run` declares it and runs the entry. A
                // use of the type is rejected where it is used.
                | HirItemKind::Schema(_) => {}
                HirItemKind::Struct(genus) => {
                    for method in &genus.methods {
                        self.signature(method.def_id, &method.func, Some(item.def_id))?;
                        functions.push((method.def_id, &method.func));
                    }
                }
                // A contract is a name for what genera provide; a value typed
                // as one is rejected where its type is classified.
                HirItemKind::Interface(interface) => {
                    if !interface.type_params.is_empty() {
                        return Err(unsupported("generic interface declaration"));
                    }
                }
                HirItemKind::Import(_) => return Err(unsupported("import (library modules)")),
            }
        }
        for item in &hir.items {
            if let HirItemKind::Constant(constant) = &item.kind {
                let (declaration, initializer) = self.constant(item, constant)?;
                items.constants.push_str(&declaration);
                items.initializers.push_str(&initializer);
            }
        }
        for item in &hir.items {
            if let HirItemKind::Struct(_) = &item.kind {
                items
                    .constants
                    .push_str(&self.class_constants(item.def_id)?);
            }
        }
        items.genus_order = order;
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
        let err = if let Some(err_ty) = function.err_ty {
            if is_void_or_none(self.types, err_ty) {
                return Err(unsupported("void or none error channel type"));
            }
            let class = classify(self.types, err_ty)?;
            if !self.c_type(class)?.ends_with('*') {
                return Err(unsupported("scalar error channel type"));
            }
            self.need_class(class);
            // The channel's carrier is declared in the runtime header.
            self.support.need(crate::helpers::Helper::Trap);
            Some(class)
        } else {
            None
        };
        if function.cli_args.is_some() || function.test.is_some() {
            return Err(unsupported("command or test function"));
        }
        if function.body.is_none() {
            return Err(unsupported("function without a body"));
        }
        let mut params = Vec::with_capacity(function.params.len());
        let mut pointees = Vec::with_capacity(function.params.len());
        let mut omissions = Vec::with_capacity(function.params.len());
        for param in &function.params {
            let (class, pointee, omission) = self.signature_parameter(param)?;
            omissions.push(omission);
            pointees.push(pointee);
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
                pointee: pointees,
                omissions,
                ret,
                err,
                receiver,
            },
        );
        Ok(())
    }

    /// Classify one signature parameter and its omission and access mode.
    fn signature_parameter(
        &mut self,
        param: &radix_hir::HirParam,
    ) -> Result<(Ty, bool, Omission), CodegenError> {
        // A `ref`/`mut` scalar parameter is a pointer to the caller's cell.
        let pointee = crate::ty::classify_ref(self.types, param.ty)?;
        let class = match pointee {
            Some(class) => class,
            None => classify(self.types, param.ty)?,
        };
        // A bare `optional` slot is the optional of its declared type: a
        // call that omits it passes none. `optional coalesce <expr>` is
        // the declared type alone, because the call site fills the
        // default (the runner does this in `lower_direct_call_args`).
        let class = match (param.optional, param.default.is_some()) {
            (true, false) => {
                // The C storage is the optional of the declared type; the
                // HIR type stays the payload, so a body read narrows only
                // where the typechecker proved it present.
                match Inner::from_ty(class) {
                    Some(inner) => Ty::Opt(inner),
                    None => {
                        return Err(unsupported(
                            "optional parameter of a sized integer or optional",
                        ));
                    }
                }
            }
            _ => class,
        };
        let omission = match (param.optional, &param.default) {
            (true, Some(default)) => Omission::Default(default.clone()),
            (true, None) => Omission::Omitted,
            (false, _) => Omission::Required,
        };
        self.need_class(class);
        // `ref` / `mut` on a list or genus passes the caller's pointer,
        // which is what an unmarked one passes too. A scalar pointer is
        // the same idea for a cell. `ref string` is the identity pointer a
        // plain string already is; `mut string` would need a pointer to the
        // caller's pointer, which this subset does not emit.
        let shared = matches!(class, Ty::List(_) | Ty::Genus(_));
        match param.mode {
            HirParamMode::Owned | HirParamMode::Ex => {}
            HirParamMode::De if shared || pointee.is_some() || class == Ty::Text => {}
            HirParamMode::In if shared || pointee.is_some() => {}
            HirParamMode::De | HirParamMode::In | HirParamMode::Axis => {
                return Err(unsupported(
                    "parameter access marker on a scalar, own, or axis",
                ));
            }
        }
        Ok((class, pointee.is_some(), omission))
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
        for (ty, pointer) in function.params.iter().zip(&function.pointee) {
            let c = self.c_type(*ty)?;
            slots.push(if *pointer { format!("{c} *") } else { c });
        }
        if function.err.is_some() {
            slots.push("faber_chan *".to_owned());
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
        let (name, param_types, pointees, ret, err, receiver) = (
            signature.name.clone(),
            signature.params.clone(),
            signature.pointee.clone(),
            signature.ret,
            signature.err,
            signature.receiver,
        );
        self.scope = Namer::program_scope();
        self.loops = 0;
        self.current = Some(def_id);
        self.returns = ret.map_or(Returns::Void, Returns::Value);
        let (mut params, param_names) =
            self.definition_parameters(function, &param_types, &pointees, receiver)?;
        if err.is_some() {
            params.push("faber_chan *faber_e".to_owned());
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
        // A parameter the body never reads is still declared (an `optional`
        // slot a callee ignores is one); mark it used so
        // `-Werror=unused-parameter` stays quiet.
        for name in param_names {
            if !crate::mentions_name(&body, &name) {
                body.insert_str(0, &format!("    (void){name};\n"));
            }
        }
        if err.is_some() && !crate::mentions_name(&body, "faber_e") {
            body.insert_str(0, "    (void)faber_e;\n");
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
        if err.is_some() && !crate::mentions_name(&body, "faber_fail") {
            // Keep the mandated failure label warning-free when the declared
            // channel has no throwing site in this body.
            body.push_str("    if (0) goto faber_fail;\n");
        }
        if err.is_some() {
            let failure_return = match ret {
                Some(class) => format!("return {};", zero_of(self, class)?),
                None => "return;".to_owned(),
            };
            write!(body, "faber_fail:\n    {failure_return}\n").ok();
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

    /// Declare the arena, receiver, and named parameters for a definition.
    fn definition_parameters(
        &mut self,
        function: &HirFunction,
        param_types: &[Ty],
        pointees: &[bool],
        receiver: Option<DefId>,
    ) -> Result<(Vec<String>, Vec<String>), CodegenError> {
        let mut params = Vec::with_capacity(function.params.len() + 2);
        let mut param_names = Vec::with_capacity(function.params.len());
        if self.arena {
            params.push("faber_arena *faber_a".to_owned());
        }
        // `self` lowers to a path to the genus's own definition.
        if let Some(genus) = receiver {
            params.push(self.declare(Ty::Genus(genus), "faber_self", true)?);
            // A method body that never names `self` still declares the
            // receiver, so mark it used below like any other parameter.
            param_names.push("faber_self".to_owned());
            self.locals.insert(
                genus,
                Local {
                    name: "faber_self".to_owned(),
                    ty: Ty::Genus(genus),
                    constant: None,
                },
            );
        }
        for (index, (param, ty)) in function
            .params
            .iter()
            .zip(param_types.iter().copied())
            .enumerate()
        {
            let Some(kind) = self.names.binding_kind(param.def_id) else {
                return Err(unsupported("parameter the name catalog does not classify"));
            };
            let raw = self.names.resolve_symbol(param.name);
            let c_name = self.scope.claim(&prefixed_binding_name(kind, raw));
            self.need_class(ty);
            // A `ref`/`mut` scalar parameter is a pointer; the body read and
            // store go through it (`Emitter::path`/`Emitter::assign`).
            if pointees.get(index).copied().unwrap_or(false) {
                self.ref_params.insert(param.def_id);
                params.push(format!("{} *{c_name}", self.c_type(ty)?));
            } else {
                params.push(self.declare(ty, &c_name, true)?);
            }
            param_names.push(c_name.clone());
            self.locals.insert(
                param.def_id,
                Local {
                    name: c_name,
                    ty,
                    constant: None,
                },
            );
        }
        Ok((params, param_names))
    }
}

fn is_void_or_none(types: &radix_types::TypeTable, ty: radix_types::TypeId) -> bool {
    match types.get(ty) {
        radix_types::Type::Alias(_, inner) => is_void_or_none(types, *inner),
        radix_types::Type::Primitive(
            radix_types::Primitive::Vacuum
            | radix_types::Primitive::Nihil
            | radix_types::Primitive::Numquam,
        ) => true,
        _ => false,
    }
}

fn zero_of(emitter: &Emitter<'_>, class: Ty) -> Result<String, CodegenError> {
    let c_type = emitter.c_type(class)?;
    if c_type.ends_with('*') {
        Ok("NULL".to_owned())
    } else if class == Ty::Decimal {
        Ok("(faber_dec){0}".to_owned())
    } else if matches!(
        class,
        Ty::Int
            | Ty::Bool
            | Ty::Sized(_)
            | Ty::Modular(_)
            | Ty::Saturating(_)
            | Ty::Bits(_)
            | Ty::None
    ) {
        Ok("0".to_owned())
    } else {
        Ok(format!("({c_type}){{0}}"))
    }
}
