//! Statement emission: locals, assignment, control flow, and print.

use crate::error::unsupported;
use crate::expr::{Value, c_string, construct_name};
use crate::helpers::Helper;
use crate::items::Returns;
use crate::ty::{Ty, classify};
use crate::{Emitter, Local};
use radix_codegen_shared::CodegenError;
use radix_codegen_shared::names::prefixed_binding_name;
use radix_hir::{
    HirBlock, HirExpression, HirExpressionKind, HirIteraMode, HirLocal, HirScribeKind,
    HirStatement, HirStatementKind,
};
use radix_types::Type;

impl Emitter<'_> {
    /// Emit a block's statements one level deeper than the current line.
    pub(crate) fn block(&mut self, out: &mut String, block: &HirBlock) -> Result<(), CodegenError> {
        if block.expr.is_some() {
            return Err(unsupported("block value expression"));
        }
        if block.breakable.is_some() {
            return Err(unsupported("breakable fac block"));
        }
        self.depth += 1;
        for statement in &block.statements {
            self.statement(out, statement)?;
        }
        self.depth -= 1;
        Ok(())
    }

    fn indent(&self) -> String {
        "    ".repeat(self.depth)
    }

    pub(crate) fn line(&self, out: &mut String, text: &str) {
        out.push_str(&self.indent());
        out.push_str(text);
        out.push('\n');
    }

    fn statement(
        &mut self,
        out: &mut String,
        statement: &HirStatement,
    ) -> Result<(), CodegenError> {
        match &statement.kind {
            HirStatementKind::Local(local) => self.local(out, local),
            HirStatementKind::Expr(expr) => self.expr_statement(out, expr),
            HirStatementKind::Redde(value) => self.redde(out, value.as_ref()),
            HirStatementKind::Rumpe => self.loop_jump(out, "break;"),
            HirStatementKind::Perge => self.loop_jump(out, "continue;"),
            HirStatementKind::Tacet => Ok(()),
            HirStatementKind::IncDec(_) => Err(unsupported("increment or decrement")),
            HirStatementKind::Custodi(_) => Err(unsupported("custodi guard section")),
        }
    }

    /// `break` / `continue` act on the innermost loop in both languages; the
    /// emitter writes no `switch`, so a C `break` cannot bind elsewhere.
    fn loop_jump(&self, out: &mut String, text: &str) -> Result<(), CodegenError> {
        if self.loops == 0 {
            return Err(unsupported("loop jump outside a loop"));
        }
        self.line(out, text);
        Ok(())
    }

    /// `return` by context: the entry block ends the program with status 0, a
    /// function with a result returns its value (already wrapped in the
    /// `return` store by typecheck).
    fn redde(
        &mut self,
        out: &mut String,
        value: Option<&HirExpression>,
    ) -> Result<(), CodegenError> {
        match (self.returns, value) {
            (Returns::Main, None) => self.line(out, "return 0;"),
            (Returns::Void, None) => self.line(out, "return;"),
            (Returns::Value(class), Some(value)) => {
                let operand = self.expr(value)?;
                let operand = if class == Ty::Int {
                    self.fit(operand)?
                } else {
                    operand
                };
                if operand.value != crate::expr::Value::from(class) {
                    return Err(unsupported("value stored without a store check"));
                }
                self.line(out, &format!("return {};", operand.text));
            }
            (Returns::Main | Returns::Void, Some(_)) => {
                return Err(unsupported(
                    "return with a value from a function without one",
                ));
            }
            (Returns::Value(_), None) => {
                return Err(unsupported(
                    "return without a value from a function with one",
                ));
            }
        }
        Ok(())
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
        let value = if class == Ty::Int {
            self.fit(value)?
        } else {
            value
        };
        if value.value != Value::from(class) {
            return Err(unsupported("value stored without a store check"));
        }
        self.need_class(class);
        let Some(kind) = self.names.binding_kind(local.def_id) else {
            return Err(unsupported("local the name catalog does not classify"));
        };
        let raw = self.names.resolve_symbol(local.name);
        let name = self.scope.claim(&prefixed_binding_name(kind, raw));
        let line = format!(
            "{} = {};",
            self.declare(class, &name, local.mutable)?,
            value.text
        );
        self.line(out, &line);
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
            HirExpressionKind::Call(..) | HirExpressionKind::MethodCall(..) => {
                let call = self.expr(expr)?;
                let line = if call.value == Value::Void {
                    format!("{};", call.text)
                } else {
                    format!("(void){};", call.text)
                };
                self.line(out, &line);
                Ok(())
            }
            HirExpressionKind::Assign(place, value) => self.assign(out, place, value),
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch: None,
                else_block,
            } => self.si(out, cond, then_block, else_block.as_ref()),
            HirExpressionKind::Dum(cond, body) => self.dum(out, cond, body),
            HirExpressionKind::Itera {
                mode: HirIteraMode::Ex,
                coordinates,
                filum: None,
                binding,
                binding_name,
                iter,
                body,
                mutable,
            } if coordinates.len() <= 1 => self.itera(
                out,
                coordinates.first().copied(),
                (*binding, *binding_name),
                iter,
                body,
                *mutable,
            ),
            HirExpressionKind::Itera {
                mode: HirIteraMode::Ab,
                coordinates,
                filum: None,
                binding,
                binding_name,
                iter,
                body,
                mutable,
            } if coordinates.is_empty() => {
                self.range_loop(out, (*binding, *binding_name), iter, body, *mutable)
            }
            HirExpressionKind::IteraRange {
                ranges,
                bindings,
                body,
                mutable,
            } => {
                let ranges: Vec<&HirExpression> = ranges.iter().collect();
                self.range_product(out, &ranges, bindings, body, *mutable)
            }
            HirExpressionKind::Block(block) => {
                self.line(out, "{");
                self.block(out, block)?;
                self.line(out, "}");
                Ok(())
            }
            other => Err(unsupported(construct_name(other))),
        }
    }

    /// `for from xs const x { … }`: an index walk over the list, read once.
    /// Typecheck forbids changing the list's shape inside the loop
    /// (`loop_collection_shape_change`), so the length may be read each turn.
    fn itera(
        &mut self,
        out: &mut String,
        coordinate: Option<(radix_hir::DefId, radix_lexer::Symbol)>,
        (binding, binding_name): (radix_hir::DefId, radix_lexer::Symbol),
        iter: &HirExpression,
        body: &HirBlock,
        mutable: bool,
    ) -> Result<(), CodegenError> {
        let list = self.expr(iter)?;
        let Value::List(elem) = list.value else {
            return Err(unsupported("iteration over a value that is not a list"));
        };
        let Some(kind) = self.names.binding_kind(binding) else {
            return Err(unsupported(
                "loop binding the name catalog does not classify",
            ));
        };
        let raw = self.names.resolve_symbol(binding_name);
        let name = self.scope.claim(&prefixed_binding_name(kind, raw));
        let (source, index) = (
            format!("faber_t{}", self.temps),
            format!("faber_i{}", self.temps),
        );
        self.temps += 1;
        let class = elem.ty();
        self.need_class(class);
        self.line(out, "{");
        self.depth += 1;
        self.line(
            out,
            &format!(
                "faber_list_{} *const {source} = {};",
                elem.suffix(),
                list.text
            ),
        );
        self.line(out, &format!("int64_t {index};"));
        self.line(
            out,
            &format!("for ({index} = 0; {index} < {source}->len; {index}++) {{"),
        );
        self.depth += 1;
        self.line(
            out,
            &format!(
                "{} = {source}->items[{index}];",
                self.declare(class, &name, mutable)?
            ),
        );
        // `apud i`: the position, read before the element.
        if let Some((position, position_name)) = coordinate {
            let Some(position_kind) = self.names.binding_kind(position) else {
                return Err(unsupported(
                    "loop coordinate the name catalog does not classify",
                ));
            };
            let raw = self.names.resolve_symbol(position_name);
            let c_name = self.scope.claim(&prefixed_binding_name(position_kind, raw));
            let declaration = self.declare(Ty::Int, &c_name, false)?;
            self.line(out, &format!("{declaration} = {index};"));
            self.locals.insert(
                position,
                Local {
                    name: c_name,
                    ty: Ty::Int,
                    constant: None,
                },
            );
        }
        self.depth -= 1;
        self.locals.insert(
            binding,
            Local {
                name,
                ty: class,
                constant: None,
            },
        );
        self.loops += 1;
        self.block(out, body)?;
        self.loops -= 1;
        self.line(out, "}");
        self.depth -= 1;
        self.line(out, "}");
        Ok(())
    }

    fn condition(&mut self, cond: &HirExpression) -> Result<String, CodegenError> {
        let value = self.expr(cond)?;
        if value.value != Value::Truth {
            return Err(unsupported("condition that is not a boolean"));
        }
        Ok(value.text)
    }

    fn si(
        &mut self,
        out: &mut String,
        cond: &HirExpression,
        then_block: &HirBlock,
        else_block: Option<&HirBlock>,
    ) -> Result<(), CodegenError> {
        let test = self.condition(cond)?;
        self.line(out, &format!("if {test} {{"));
        self.block(out, then_block)?;
        if let Some(otherwise) = else_block {
            self.line(out, "} else {");
            self.block(out, otherwise)?;
        }
        self.line(out, "}");
        Ok(())
    }

    fn dum(
        &mut self,
        out: &mut String,
        cond: &HirExpression,
        body: &HirBlock,
    ) -> Result<(), CodegenError> {
        let test = self.condition(cond)?;
        self.line(out, &format!("while {test} {{"));
        self.loops += 1;
        self.block(out, body)?;
        self.loops -= 1;
        self.line(out, "}");
        Ok(())
    }

    /// `place ← value` into a mutable local. Typecheck wraps the value in the
    /// store; an exact value that arrives unwrapped is a missing fact.
    fn assign(
        &mut self,
        out: &mut String,
        place: &HirExpression,
        value: &HirExpression,
    ) -> Result<(), CodegenError> {
        if let HirExpressionKind::Field(base, name) = &place.kind {
            let line = self.field_write(base, *name, value)?;
            self.line(out, &line);
            return Ok(());
        }
        if let HirExpressionKind::Index(base, index) = &place.kind {
            let line = self.list_write(base, index, value)?;
            self.line(out, &line);
            return Ok(());
        }
        let HirExpressionKind::Path(def_id) = &place.kind else {
            return Err(unsupported("assignment to a place that is not a local"));
        };
        let Some(local) = self.locals.get(def_id) else {
            return Err(unsupported("assignment to a name that is not a local"));
        };
        let (name, class) = (local.name.clone(), local.ty);
        let value = self.expr(value)?;
        let value = if class == Ty::Int {
            self.fit(value)?
        } else {
            value
        };
        if value.value != Value::from(class) {
            return Err(unsupported("value stored without a store check"));
        }
        self.line(out, &format!("{name} = {};", value.text));
        Ok(())
    }

    /// `print a, b` writes the values space separated and ends the line: one
    /// `printf` whose format holds only conversion specifiers, spaces and the
    /// newline. Text is always an `%s` argument, never part of the format, so a
    /// `%` in Faber text prints as itself. An exact value prints through
    /// `faber_i128_text` into a block-scoped compound-literal buffer.
    fn print(
        &mut self,
        out: &mut String,
        kind: HirScribeKind,
        args: &[HirExpression],
    ) -> Result<(), CodegenError> {
        let mut operands = Vec::with_capacity(args.len());
        for arg in args {
            // The absent-option token waits on the option type.
            if let Some(ty) = arg.ty
                && matches!(self.types.get(ty), Type::Option(_))
            {
                return Err(unsupported("printing an absent option"));
            }
            operands.push(self.expr(arg)?);
        }
        let prelude = self.sequence(&mut operands);
        let tokens = self.names.display_tokens();
        let (yes, no) = (c_string(&tokens.true_)?, c_string(&tokens.false_)?);
        let mut specs = Vec::with_capacity(operands.len());
        let mut values = Vec::with_capacity(operands.len());
        for value in operands {
            let (spec, text) = match value.value {
                Value::Int => {
                    self.support.need_ints();
                    ("%\" PRId64 \"", value.text)
                }
                Value::Exact => {
                    self.support.need(Helper::Text);
                    (
                        "%s",
                        format!("faber_i128_text({}, (char[41]){{0}})", value.text),
                    )
                }
                Value::Text => ("%s", value.text),
                // The module's locale words: `true` / `false` for an `en`
                // program, `verum` / `falsum` for a `la` one.
                Value::Truth => ("%s", format!("({} ? {yes} : {no})", value.text)),
                Value::List(elem) => ("%s", self.show_list(&value.text, elem)?),
                Value::Genus(def_id) => ("%s", self.show_genus(&value.text, def_id)?),
                Value::Float => (
                    "%s",
                    format!(
                        "faber_f64_text({}, (char[FABER_F64_TEXT]){{0}})",
                        value.text
                    ),
                ),
                Value::Void => return Err(unsupported("printing a call with no result")),
            };
            specs.push(spec);
            values.push(text);
        }
        self.support.need_stdio();
        // `debug` and `warn` write their line to standard error; `print` and
        // `write` to standard output.
        let call = match kind {
            HirScribeKind::Vide | HirScribeKind::Mone => "fprintf(stderr, ",
            HirScribeKind::Nota | HirScribeKind::Scribe => "printf(",
        };
        let mut line = format!("{call}\"{}\\n\"", specs.join(" "));
        for value in values {
            line.push_str(", ");
            line.push_str(&value);
        }
        line.push_str(");");
        if prelude.is_empty() {
            self.line(out, &line);
        } else {
            // Arguments with calls are bound in source order first; the block
            // scopes the temporaries.
            self.line(out, "{");
            self.depth += 1;
            self.line(out, prelude.trim_end());
            self.line(out, &line);
            self.depth -= 1;
            self.line(out, "}");
        }
        Ok(())
    }
}
