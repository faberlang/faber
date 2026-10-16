//! Anonymous closures.
//!
//! A `HirExpressionKind::Clausura` has no `DefId`, so it cannot be emitted
//! inline: C has no nested functions. A closure that reads no enclosing binding
//! is exactly a top-level function with the closure's parameters and body, and
//! its value is a C function pointer — the class the function-value machinery
//! already spells and calls indirectly ([`crate::expr`] `Value::Func`). Each
//! such closure is synthesized once and flushed with the function texts.
//!
//! A closure that captures an enclosing binding would need a closure struct
//! carrying the captured cells, which this subset does not emit, so it is
//! rejected by name. The check is deliberately conservative: anything the body
//! references that is not its own parameter, an inner local, or a named
//! function counts as a capture.

use crate::Emitter;
use crate::error::unsupported;
use crate::expr::{Operand, Value};
use crate::items::Returns;
use crate::names::Namer;
use crate::ty::{Ty, classify, returns};
use radix_codegen_shared::CodegenError;
use radix_codegen_shared::names::prefixed_binding_name;
use radix_hir::visit::{HirVisitor, walk_expr, walk_local, walk_stmt};
use radix_hir::{
    DefId, HirExpression, HirExpressionKind, HirLocal, HirParam, HirParamMode, HirStatement,
    HirStatementKind,
};
use rustc_hash::FxHashSet;

/// A closure waiting for its synthesized top-level function to be written.
#[derive(Clone)]
pub(crate) struct PendingClosure {
    /// The claimed file-scope C name (`f_closure_<n>`).
    pub(crate) name: String,
    pub(crate) params: Vec<HirParam>,
    /// Per parameter, the classified C storage class.
    pub(crate) param_types: Vec<Ty>,
    /// Per parameter, whether the C storage is a pointer to the caller's cell
    /// (a `ref`/`mut` scalar parameter, as for a named function).
    pub(crate) pointee: Vec<bool>,
    /// `None` for a closure with no result.
    pub(crate) ret: Option<Ty>,
    pub(crate) body: HirExpression,
}

impl Emitter<'_> {
    /// A closure in value position: synthesize a top-level C function and yield
    /// a function pointer to it ([`Value::Func`]).
    pub(crate) fn closure(
        &mut self,
        expr: &HirExpression,
        params: &[HirParam],
        ret: Option<radix_types::TypeId>,
        body: &HirExpression,
    ) -> Result<Operand, CodegenError> {
        let Some(ty) = expr.ty else {
            return Err(unsupported("closure without a resolved type"));
        };
        let Ty::Func(id) = classify(self.types, ty)? else {
            return Err(unsupported("closure of another class"));
        };
        if self.captures(params, body) {
            return Err(unsupported("capturing closure"));
        }
        let (param_types, pointee) = self.closure_params(params)?;
        // The result is the closure's own `→ T`, or the function type's result
        // when the compact form left it out (`int x ∴ x * 2`).
        let ret = match ret {
            Some(ret_ty) => returns(self.types, ret_ty)?,
            None => match self.types.get(radix_types::TypeId(id)) {
                radix_types::Type::Func(sig) => returns(self.types, sig.ret)?,
                _ => return Err(unsupported("closure without a signature")),
            },
        };
        let raw = format!("closure_{}", self.closure_seq);
        self.closure_seq += 1;
        let name = self.items_scope.claim(&format!("f_{raw}"));
        self.closures.push(PendingClosure {
            name: name.clone(),
            params: params.to_vec(),
            param_types,
            pointee,
            ret,
            body: body.clone(),
        });
        Ok(Operand::new(name, Value::Func(id)))
    }

    /// Classify a closure's parameters the way a named function's are: a
    /// `ref`/`mut` scalar is a pointer to the caller's cell, everything else is
    /// its declared class. An omitted-argument slot has no meaning for a call
    /// through a function value, so an optional parameter is refused.
    fn closure_params(
        &mut self,
        params: &[HirParam],
    ) -> Result<(Vec<Ty>, Vec<bool>), CodegenError> {
        let mut types = Vec::with_capacity(params.len());
        let mut pointees = Vec::with_capacity(params.len());
        for param in params {
            if param.optional {
                return Err(unsupported("optional closure parameter"));
            }
            let pointee = crate::ty::classify_ref(self.types, param.ty)?;
            let class = match pointee {
                Some(class) => class,
                None => classify(self.types, param.ty)?,
            };
            let shared = matches!(class, Ty::List(_) | Ty::Genus(_));
            match param.mode {
                HirParamMode::Owned | HirParamMode::Ex => {}
                HirParamMode::De if shared || pointee.is_some() || class == Ty::Text => {}
                HirParamMode::In if shared || pointee.is_some() => {}
                HirParamMode::De | HirParamMode::In | HirParamMode::Axis => {
                    return Err(unsupported(
                        "parameter access marker on a scalar, own, or axis closure parameter",
                    ));
                }
            }
            self.need_class(class);
            types.push(class);
            pointees.push(pointee.is_some());
        }
        Ok((types, pointees))
    }

    /// Whether the closure body reads an enclosing binding (a capture). Its own
    /// parameters, inner locals, and named functions are not captures.
    fn captures(&self, params: &[HirParam], body: &HirExpression) -> bool {
        let mut free = FreeVars::default();
        for param in params {
            free.declared.insert(param.def_id);
        }
        free.visit_expr(body);
        free.referenced
            .iter()
            .any(|def_id| !free.declared.contains(def_id) && !self.functions.contains_key(def_id))
    }

    /// Write every synthesized closure function discovered so far, as the
    /// prototype text and the definition text. A closure body may itself hold
    /// closures, so the work list grows as it is drained.
    pub(crate) fn flush_closures(&mut self) -> Result<(String, String), CodegenError> {
        let mut prototypes = String::new();
        let mut definitions = String::new();
        let mut done = 0;
        while done < self.closures.len() {
            let pending = self.closures[done].clone();
            done += 1;
            let (prototype, definition) = self.emit_closure(&pending)?;
            prototypes.push_str(&prototype);
            definitions.push_str(&definition);
        }
        Ok((prototypes, definitions))
    }

    /// One closure function: its signature is the closure's parameters and
    /// result, its body the closure's body. A `return` inside emits through
    /// `Returns::Value`, exactly as a named function's does.
    fn emit_closure(&mut self, pending: &PendingClosure) -> Result<(String, String), CodegenError> {
        self.scope = Namer::program_scope();
        self.loops = 0;
        self.current = None;
        self.returns = pending.ret.map_or(Returns::Void, Returns::Value);
        let mut slots = Vec::with_capacity(pending.params.len() + 1);
        let mut names = Vec::with_capacity(pending.params.len());
        if self.arena {
            slots.push("faber_arena *faber_a".to_owned());
        }
        for (index, (param, ty)) in pending.params.iter().zip(&pending.param_types).enumerate() {
            let Some(kind) = self.names.binding_kind(param.def_id) else {
                return Err(unsupported("closure parameter without a classified name"));
            };
            let raw = self.names.resolve_symbol(param.name);
            let name = self.scope.claim(&prefixed_binding_name(kind, raw));
            self.need_class(*ty);
            if pending.pointee.get(index).copied().unwrap_or(false) {
                self.ref_params.insert(param.def_id);
                slots.push(format!("{} *{name}", self.c_type(*ty)?));
            } else {
                slots.push(self.declare(*ty, &name, true)?);
            }
            names.push(name.clone());
            self.locals.insert(
                param.def_id,
                crate::Local {
                    name,
                    ty: *ty,
                    constant: None,
                },
            );
        }
        let mut body = String::new();
        self.closure_body(&mut body, &pending.body, pending.ret)?;
        if self.arena && !crate::mentions_arena(&body) {
            body.insert_str(0, "    (void)faber_a;\n");
        }
        for name in names {
            if !crate::mentions_name(&body, &name) {
                body.insert_str(0, &format!("    (void){name};\n"));
            }
        }
        let ret_text = match pending.ret {
            Some(ty) => self.c_type(ty)?,
            None => "void".to_owned(),
        };
        let space = if ret_text.ends_with('*') { "" } else { " " };
        let params = if slots.is_empty() {
            "void".to_owned()
        } else {
            slots.join(", ")
        };
        let prototype = format!("static {ret_text}{space}{}({params});\n", pending.name);
        let definition = format!(
            "static {ret_text}{space}{}({params}) {{\n{body}}}\n",
            pending.name
        );
        Ok((prototype, definition))
    }

    /// The closure's body: a block is its statements (a result-typed block that
    /// does not end in `return` gets the same unreachable trap a function
    /// gets); an expression body is `return <expr>;` when the closure has a
    /// result, or a discarded statement when it does not.
    fn closure_body(
        &mut self,
        out: &mut String,
        body: &HirExpression,
        ret: Option<Ty>,
    ) -> Result<(), CodegenError> {
        if let HirExpressionKind::Block(block) = &body.kind {
            self.block(out, block)?;
            let ends_in_return = block
                .statements
                .last()
                .is_some_and(|statement| matches!(statement.kind, HirStatementKind::Redde(_)));
            if ret.is_some() && !ends_in_return {
                self.support.need(crate::helpers::Helper::Trap);
                out.push_str("    faber_trap(\"function ended without a return\");\n");
            }
            return Ok(());
        }
        let operand = self.expr(body)?;
        match ret {
            Some(class) => {
                let operand = self.settle(operand, class)?;
                if operand.value != Value::from(class) {
                    return Err(unsupported("value stored without a store check"));
                }
                out.push_str("    return ");
                out.push_str(&operand.text);
                out.push_str(";\n");
            }
            None if operand.value == Value::Void => {
                out.push_str("    ");
                out.push_str(&operand.text);
                out.push_str(";\n");
            }
            None => {
                out.push_str("    (void)(");
                out.push_str(&operand.text);
                out.push_str(");\n");
            }
        }
        Ok(())
    }
}

/// Every `Path` a body reads, and every binding the body itself declares
/// (parameters, locals, and the parameters of any nested closure).
#[derive(Default)]
struct FreeVars {
    declared: FxHashSet<DefId>,
    referenced: FxHashSet<DefId>,
}

impl HirVisitor for FreeVars {
    fn visit_param(&mut self, param: &HirParam) {
        self.declared.insert(param.def_id);
        if let Some(default) = &param.default {
            self.visit_expr(default);
        }
    }

    fn visit_local(&mut self, local: &HirLocal) {
        self.declared.insert(local.def_id);
        walk_local(self, local);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Path(def_id) = &expr.kind {
            self.referenced.insert(*def_id);
        }
        walk_expr(self, expr);
    }

    fn visit_stmt(&mut self, stmt: &HirStatement) {
        walk_stmt(self, stmt);
    }
}
