//! Statement emission: locals and print.

use crate::error::unsupported;
use crate::expr::construct_name;
use crate::ty::{Ty, classify};
use crate::{Emitter, Local};
use radix_codegen_shared::CodegenError;
use radix_hir::{
    HirBlock, HirExpression, HirExpressionKind, HirLocal, HirScribeKind, HirStatement,
    HirStatementKind,
};
use radix_types::{Primitive, Type};

impl Emitter<'_> {
    /// Emit a block's statements, one level deep (the program has one function).
    pub(crate) fn block(&mut self, out: &mut String, block: &HirBlock) -> Result<(), CodegenError> {
        if block.expr.is_some() {
            return Err(unsupported("block value expression"));
        }
        if block.breakable.is_some() {
            return Err(unsupported("breakable fac block"));
        }
        for statement in &block.statements {
            self.statement(out, statement)?;
        }
        Ok(())
    }

    fn statement(
        &mut self,
        out: &mut String,
        statement: &HirStatement,
    ) -> Result<(), CodegenError> {
        match &statement.kind {
            HirStatementKind::Local(local) => self.local(out, local),
            HirStatementKind::Expr(expr) => self.expr_statement(out, expr),
            HirStatementKind::Redde(None) => {
                out.push_str("    return 0;\n");
                Ok(())
            }
            HirStatementKind::Redde(Some(_)) => Err(unsupported("return with a value")),
            HirStatementKind::Rumpe => Err(unsupported("loop break")),
            HirStatementKind::Perge => Err(unsupported("loop continue")),
            HirStatementKind::Tacet => Ok(()),
            HirStatementKind::IncDec(_) => Err(unsupported("increment or decrement")),
            HirStatementKind::Custodi(_) => Err(unsupported("custodi guard section")),
        }
    }

    fn local(&mut self, out: &mut String, local: &HirLocal) -> Result<(), CodegenError> {
        if local.is_await || local.runtime_provided || local.await_store.is_some() {
            return Err(unsupported("await or runtime-provided binding"));
        }
        let Some(ty) = local
            .ty
            .or_else(|| local.init.as_ref().and_then(|init| init.ty))
        else {
            return Err(unsupported("local without a resolved type"));
        };
        let class = classify(self.types, ty)?;
        let Some(init) = &local.init else {
            return Err(unsupported("local without an initializer"));
        };
        let value = self.expr(init)?;
        if value.ty != class {
            return Err(unsupported("conversion between value classes"));
        }
        if class == Ty::Int {
            self.includes.need_ints();
        }
        let name = self.scope.claim(self.names.resolve_symbol(local.name));
        let line = format!(
            "    {} = {};\n",
            class.declare(&name, local.mutable),
            value.text
        );
        out.push_str(&line);
        let constant = if local.mutable { None } else { value.constant };
        self.locals.insert(
            local.def_id,
            Local {
                name,
                ty: class,
                constant,
            },
        );
        Ok(())
    }

    fn expr_statement(
        &mut self,
        out: &mut String,
        expr: &HirExpression,
    ) -> Result<(), CodegenError> {
        match &expr.kind {
            HirExpressionKind::Scribe(kind, args) => self.print(out, *kind, args),
            other => Err(unsupported(construct_name(other))),
        }
    }

    /// `print a, b` writes the values space separated and ends the line: one
    /// `printf` whose format holds only conversion specifiers, spaces and the
    /// newline. Text is always an `%s` argument, never part of the format, so a
    /// `%` in Faber text prints as itself.
    fn print(
        &mut self,
        out: &mut String,
        kind: HirScribeKind,
        args: &[HirExpression],
    ) -> Result<(), CodegenError> {
        if kind != HirScribeKind::Nota {
            return Err(unsupported(
                "write, debug, or warn output (only print is supported)",
            ));
        }
        let mut specs = Vec::with_capacity(args.len());
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            // Display tokens for booleans and absent options wait on the
            // boolean and option types; no print path may render one yet.
            if let Some(ty) = arg.ty {
                match self.types.get(ty) {
                    Type::Primitive(Primitive::Bivalens) => {
                        return Err(unsupported("printing a boolean"));
                    }
                    Type::Option(_) => return Err(unsupported("printing an absent option")),
                    _ => {}
                }
            }
            let value = self.expr(arg)?;
            specs.push(match value.ty {
                Ty::Int => {
                    self.includes.need_ints();
                    "%\" PRId64 \""
                }
                Ty::Text => "%s",
            });
            values.push(value.text);
        }
        self.includes.need_stdio();
        let mut line = format!("    printf(\"{}\\n\"", specs.join(" "));
        for value in values {
            line.push_str(", ");
            line.push_str(&value);
        }
        line.push_str(");\n");
        out.push_str(&line);
        Ok(())
    }
}
