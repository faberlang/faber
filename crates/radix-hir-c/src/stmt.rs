//! Statement emission: locals, assignment, control flow, and print.

use crate::error::unsupported;
use crate::expr::{Operand, Value, construct_name};
use crate::helpers::{Helper, ListOp};
use crate::items::Returns;
use crate::ty::{Ty, classify, int_width_of_bits};
use crate::{Emitter, Local};
use radix_codegen_shared::CodegenError;
use radix_codegen_shared::names::prefixed_binding_name;
use radix_hir::{
    HirBlock, HirCape, HirCustodi, HirExpression, HirExpressionKind, HirIteraMode, HirLocal,
    HirScribeKind, HirStatement, HirStatementKind,
};
use radix_lexer::Symbol;
use radix_types::BitsWidth;

impl Emitter<'_> {
    /// Emit a block's statements one level deeper than the current line.
    pub(crate) fn block(&mut self, out: &mut String, block: &HirBlock) -> Result<(), CodegenError> {
        self.depth += 1;
        let result = self.block_contents(out, block);
        self.depth -= 1;
        result
    }

    /// Emit a block at the current indentation without changing its scope.
    fn block_contents(&mut self, out: &mut String, block: &HirBlock) -> Result<(), CodegenError> {
        for statement in &block.statements {
            self.statement(out, statement)?;
        }
        // A tail expression of a block written for effect (an `else` that
        // holds an `elif`, a `fac` body) is one more statement: its value is
        // unused. A block that yields a value is a conditional's arm, which
        // [`Emitter::conditional`] reads itself.
        if let Some(tail) = &block.expr {
            self.expr_statement(out, tail)?;
        }
        Ok(())
    }

    /// The first `count` statements of `block`, one level deeper (a loop body
    /// whose last statement is written separately).
    fn statements(
        &mut self,
        out: &mut String,
        statements: &[HirStatement],
    ) -> Result<(), CodegenError> {
        self.depth += 1;
        for statement in statements {
            self.statement(out, statement)?;
        }
        self.depth -= 1;
        Ok(())
    }

    pub(crate) fn line(&self, out: &mut String, text: &str) {
        for _ in 0..self.depth {
            out.push_str("    ");
        }
        out.push_str(text);
        out.push('\n');
    }

    fn statement(
        &mut self,
        out: &mut String,
        statement: &HirStatement,
    ) -> Result<(), CodegenError> {
        self.statement_kind(out, statement)
            .map_err(|error| located(error, statement.span.start))
    }

    fn statement_kind(
        &mut self,
        out: &mut String,
        statement: &HirStatement,
    ) -> Result<(), CodegenError> {
        match &statement.kind {
            HirStatementKind::Local(local) => self.local(out, local),
            HirStatementKind::Expr(expr) => self.expr_statement(out, expr),
            HirStatementKind::Redde(value) => self.redde(out, value.as_ref()),
            HirStatementKind::ReddeFilum(_) => Err(unsupported("return_thread_lowering_pending")),
            HirStatementKind::Rumpe => self.loop_jump(out, "break;"),
            HirStatementKind::Perge => self.loop_jump(out, "continue;"),
            HirStatementKind::Tacet => Ok(()),
            HirStatementKind::IncDec(inc_dec) => self.inc_dec(out, inc_dec),
            HirStatementKind::Custodi(custodi) => self.custodi(out, custodi),
        }
    }

    /// `break` / `continue` act on the innermost loop in both languages; the
    /// emitter writes no `switch`, so a C `break` cannot bind elsewhere.
    fn loop_jump(&mut self, out: &mut String, text: &str) -> Result<(), CodegenError> {
        // `rumpe` leaves the innermost breakable scope: a plain `fac` block
        // when one is open inside the innermost loop, else the loop. A C
        // `break` binds to the loop only, so a block exit is a `goto`.
        if text == "break;"
            && let Some((depth, label, used)) = self.fac_exits.last_mut()
            && *depth == self.loops
        {
            *used = true;
            let jump = format!("goto {label};");
            self.line(out, &jump);
            return Ok(());
        }
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
                let operand = self.settle(operand, class)?;
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
        // The entry's arguments are the one runtime-provided binding.
        let entry_args = local.init.is_none() && self.entry_args == Some(local.name);
        if local.is_await || (local.runtime_provided && !entry_args) || local.await_store.is_some()
        {
            return Err(unsupported("await or runtime-provided binding"));
        }
        let Some(ty) = local
            .ty
            .or_else(|| local.init.as_ref().and_then(|init| init.ty))
        else {
            return Err(unsupported("local without a resolved type"));
        };
        let class = classify(self.types, ty)?;
        // A declaration initializer's `copy` lands on the declared type; the
        // bind deep-duplicates its source (R7).
        let copy = crate::ty::is_copy_marker(self.types, ty);
        let Some(init) = &local.init else {
            if entry_args && class == Ty::List(crate::ty::Elem::Text) {
                return self.entry_arguments(out, local);
            }
            // A deferred binding (`const int pending` then `pending ← 7`): the
            // frontend proves it is assigned before any read, so the C cell is
            // declared without an initializer and written once. It is declared
            // writable so that one store compiles.
            self.need_class(class);
            let Some(kind) = self.names.binding_kind(local.def_id) else {
                return Err(unsupported("local the name catalog does not classify"));
            };
            let raw = self.names.resolve_symbol(local.name);
            let name = self.scope.claim(&prefixed_binding_name(kind, raw));
            let line = format!("{};", self.declare(class, &name, true)?);
            self.line(out, &line);
            self.locals.insert(
                local.def_id,
                Local {
                    name,
                    ty: class,
                    constant: None,
                },
            );
            return Ok(());
        };
        let value = self.expr(init)?;
        let value = if copy { self.copy_value(value)? } else { value };
        let value = self.settle(value, class)?;
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

    /// `main args xs`: the arguments after the program name, as a text list.
    fn entry_arguments(&mut self, out: &mut String, local: &HirLocal) -> Result<(), CodegenError> {
        let Some(kind) = self.names.binding_kind(local.def_id) else {
            return Err(unsupported("local the name catalog does not classify"));
        };
        let raw = self.names.resolve_symbol(local.name);
        let name = self.scope.claim(&prefixed_binding_name(kind, raw));
        let class = Ty::List(crate::ty::Elem::Text);
        self.need_class(class);
        self.support.need(Helper::List(
            crate::ty::Elem::Text,
            crate::helpers::ListOp::Push,
        ));
        self.uses_args = true;
        let declaration = self.declare(class, &name, local.mutable)?;
        self.line(
            out,
            &format!("{declaration} = faber_argv_text(faber_a, argc, argv);"),
        );
        self.locals.insert(
            local.def_id,
            Local {
                name,
                ty: class,
                constant: None,
            },
        );
        Ok(())
    }

    pub(crate) fn expr_statement(
        &mut self,
        out: &mut String,
        expr: &HirExpression,
    ) -> Result<(), CodegenError> {
        match &expr.kind {
            HirExpressionKind::Scribe(kind, args) => self.print(out, *kind, args),
            HirExpressionKind::Throw(value) => self.throw(out, value),
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
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
                params,
            } => self.conversion_statement(out, target, source, params, recovery.as_deref()),
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
                // A lane binder (`thread f` / `filum f`) is declared metadata,
                // not a runtime binding: MIR lowering ignores it, so the C
                // loop is the plain single-source loop.
                filum: _,
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
                filum: _,
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
            HirExpressionKind::IteraLockstep {
                sources,
                coordinates,
                bindings,
                body,
                mutable,
            } => self.lockstep(out, sources, coordinates, bindings, body, *mutable),
            HirExpressionKind::Discerne {
                scrutinees, arms, ..
            } => self.switch(out, scrutinees, arms),
            HirExpressionKind::Block(block) => self.plain_block(out, block),
            HirExpressionKind::Handled { body, catch } => self.handled(out, body, catch),
            HirExpressionKind::Loop(block) => self.loop_block(out, block),
            HirExpressionKind::Panic(message) => {
                let message = self.expr(message)?;
                if message.value != Value::Text {
                    return Err(unsupported("panic with a message that is not text"));
                }
                self.support.need(Helper::Trap);
                self.line(out, &format!("faber_abort({});", message.text));
                Ok(())
            }
            HirExpressionKind::Adfirma(cond, message) => {
                self.assertion(out, cond, message.as_deref())
            }
            other => Err(unsupported(construct_name(other))),
        }
    }

    fn throw(&mut self, out: &mut String, value: &HirExpression) -> Result<(), CodegenError> {
        let Some(err) = self
            .current
            .and_then(|def_id| self.functions.get(&def_id))
            .and_then(|function| function.err)
        else {
            return Err(unsupported("throw without an error channel"));
        };
        let value = self.expr(value)?;
        let value = self.settle(value, err)?;
        if value.value != Value::from(err) {
            return Err(unsupported("error payload stored without a store check"));
        }
        if let Some(channel) = self.channels.last().cloned() {
            self.channel_handlers.insert(channel.clone());
            self.line(
                out,
                &format!(
                    "{channel} = (faber_chan){{true, (const void *)({})}};",
                    value.text
                ),
            );
            self.line(out, &format!("goto {channel}_handler;"));
        } else {
            self.line(
                out,
                &format!(
                    "*faber_e = (faber_chan){{true, (const void *)({})}};",
                    value.text
                ),
            );
            self.line(out, "goto faber_fail;");
        }
        Ok(())
    }

    /// `do { ... } catch name { ... }`: a local channel records one failure,
    /// and a handler label leaves the body before the catch arm runs.
    fn handled(
        &mut self,
        out: &mut String,
        body: &HirBlock,
        catch: &HirCape,
    ) -> Result<(), CodegenError> {
        let channel = format!("faber_c{}", self.temps);
        self.temps += 1;
        let handler = format!("{channel}_handler");
        self.line(out, "{");
        self.depth += 1;
        self.line(out, &format!("faber_chan {channel} = {{false, NULL}};"));
        self.channels.push(channel.clone());
        let body_result = self.block_contents(out, body);
        let popped = self.channels.pop();
        debug_assert_eq!(popped.as_deref(), Some(channel.as_str()));
        if let Err(error) = body_result {
            self.depth -= 1;
            self.line(out, "}");
            return Err(error);
        }
        if self.channel_handlers.remove(&channel) {
            self.line(out, &format!("{handler}:;"));
        }
        self.line(out, &format!("if ({channel}.set) {{"));
        self.depth += 1;
        let catch_result = self.catch_binding(out, catch, &channel);
        let catch_result = catch_result.and_then(|()| self.block_contents(out, &catch.body));
        self.depth -= 1;
        self.line(out, "}");
        self.depth -= 1;
        self.line(out, "}");
        catch_result
    }

    /// Bind the caught payload as the catch clause's ordinary local.
    fn catch_binding(
        &mut self,
        out: &mut String,
        catch: &HirCape,
        channel: &str,
    ) -> Result<(), CodegenError> {
        let Some(ty) = catch.binding_ty else {
            return Err(unsupported("catch binding without a resolved type"));
        };
        let class = classify(self.types, ty)?;
        self.need_class(class);
        let c_type = self.c_type(class)?;
        let Some(kind) = self.names.binding_kind(catch.binding_def_id) else {
            return Err(unsupported(
                "catch binding the name catalog does not classify",
            ));
        };
        let raw = self.names.resolve_symbol(catch.binding_name);
        let name = self.scope.claim(&prefixed_binding_name(kind, raw));
        let declaration = self.declare(class, &name, false)?;
        self.line(out, &format!("{declaration} = ({c_type}){channel}.value;"));
        self.locals.insert(
            catch.binding_def_id,
            Local {
                name,
                ty: class,
                constant: None,
            },
        );
        Ok(())
    }

    /// A block written as a statement. A `fac` block is a breakable scope:
    /// `rumpe` inside it leaves the block, so it ends in a label when one did.
    fn plain_block(&mut self, out: &mut String, block: &HirBlock) -> Result<(), CodegenError> {
        let scope = block
            .breakable
            .is_some_and(radix_hir::HirBreakableKind::is_scope);
        if scope {
            let label = format!("faber_x{}", self.temps);
            self.temps += 1;
            self.fac_exits.push((self.loops, label, false));
        }
        self.line(out, "{");
        let result = self.block(out, block);
        self.line(out, "}");
        if scope && let Some((_, label, used)) = self.fac_exits.pop() {
            result?;
            if used {
                self.line(out, &format!("{label}:;"));
            }
            return Ok(());
        }
        result
    }

    /// `fac { … } dum cond` lowers to a loop whose last statement is
    /// `if (!cond) rumpe`; C's `do … while` runs `continue` through that test,
    /// as `perge` does. Any other loop is `for (;;)`.
    fn loop_block(&mut self, out: &mut String, block: &HirBlock) -> Result<(), CodegenError> {
        if block.expr.is_some() {
            return Err(unsupported("loop with a value"));
        }
        let test = block.statements.split_last().and_then(|(last, rest)| {
            let HirStatementKind::Expr(HirExpression {
                kind:
                    HirExpressionKind::Si {
                        cond,
                        then_block,
                        then_catch: None,
                        else_block: None,
                    },
                ..
            }) = &last.kind
            else {
                return None;
            };
            let HirExpressionKind::Unary(radix_hir::HirUnOp::Not, inner) = &cond.kind else {
                return None;
            };
            let only_break = then_block.expr.is_none()
                && matches!(
                    then_block.statements.as_slice(),
                    [HirStatement {
                        kind: HirStatementKind::Rumpe,
                        ..
                    }]
                );
            only_break.then_some((rest, inner))
        });
        self.loops += 1;
        let result = if let Some((rest, cond)) = test {
            self.line(out, "do {");
            let body = self.statements(out, rest);
            self.loops -= 1;
            body?;
            let test = self.condition(cond)?;
            self.line(out, &format!("}} while {test};"));
            return Ok(());
        } else {
            self.line(out, "for (;;) {");
            self.block(out, block)
        };
        self.loops -= 1;
        result?;
        self.line(out, "}");
        Ok(())
    }

    /// `assert cond [panic "msg"]`: a failed assertion aborts with the
    /// message, or `assertion failed`.
    fn assertion(
        &mut self,
        out: &mut String,
        cond: &HirExpression,
        message: Option<&HirExpression>,
    ) -> Result<(), CodegenError> {
        let test = self.condition(cond)?;
        let failure_text = match message {
            Some(message) => {
                let message = self.expr(message)?;
                if message.value != Value::Text {
                    return Err(unsupported("assertion message that is not text"));
                }
                message.text
            }
            None => self.text_literal("assertion failed")?,
        };
        self.support.need(Helper::Trap);
        self.line(out, &format!("if (!{test}) {{"));
        self.depth += 1;
        self.line(out, &format!("faber_abort({failure_text});"));
        self.depth -= 1;
        self.line(out, "}");
        Ok(())
    }

    /// `place ↑` / `place ↓` on an `int` local: one exact step, stored back
    /// through the same check an assignment makes.
    fn inc_dec(
        &mut self,
        out: &mut String,
        inc_dec: &radix_hir::HirIncDec,
    ) -> Result<(), CodegenError> {
        let HirExpressionKind::Path(def_id) = &inc_dec.target.kind else {
            return Err(unsupported(
                "increment or decrement of a place that is not a local",
            ));
        };
        let Some(local) = self.locals.get(def_id) else {
            return Err(unsupported(
                "increment or decrement of a name that is not a local",
            ));
        };
        let name = local.name.clone();
        let Ok(written) = self.names.resolve_def(*def_id) else {
            return Err(unsupported("increment of a name the catalog does not hold"));
        };
        let inc = inc_dec.op == radix_hir::HirIncDecOp::Inc;
        match local.ty {
            Ty::Int => {
                let (helper, call) = if inc {
                    (Helper::Add, "faber_add")
                } else {
                    (Helper::Sub, "faber_sub")
                };
                self.support.need(helper);
                self.support.need(Helper::StoreI64);
                let at = self.text_literal(&format!("assignment to `{written}`"))?;
                self.line(
                    out,
                    &format!("{name} = faber_store_i64({call}({name}, 1), {at});"),
                );
            }
            Ty::Sized(width) => {
                let call = if inc { "faber_add" } else { "faber_sub" };
                self.support.need(Helper::SizedStore);
                let at = self.text_literal(&format!("assignment to `{written}`"))?;
                self.line(
                    out,
                    &format!(
                        "{name} = faber_store_{}({call}({name}, 1), {at});",
                        width.marker()
                    ),
                );
            }
            Ty::Modular(width) | Ty::Saturating(width) => {
                self.support.need(Helper::Wide);
                self.support.need_ints();
                let call = if inc {
                    "faber_add_wide"
                } else {
                    "faber_sub_wide"
                };
                let kind = if matches!(local.ty, Ty::Modular(_)) {
                    "wrapping"
                } else {
                    "saturating"
                };
                self.line(
                    out,
                    &format!(
                        "{name} = faber_store_{kind}_{}({call}({name}, 1));",
                        width.marker()
                    ),
                );
            }
            _ => return Err(unsupported("increment or decrement of a non-integer")),
        }
        Ok(())
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
        if let Value::Map(k, v) = list.value {
            return self.itera_map(
                out,
                coordinate,
                (binding, binding_name),
                &list,
                k,
                v,
                body,
                mutable,
            );
        }
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

    /// `for from m const v { … }` / `for from m at [k] const v { … }`: a walk
    /// over a map's values (`ex` binds the value; the optional coordinate binds
    /// the key). A key added while the walk is live traps `map changed size
    /// during iteration`, as the runner does.
    #[allow(clippy::too_many_arguments)]
    fn itera_map(
        &mut self,
        out: &mut String,
        coordinate: Option<(radix_hir::DefId, radix_lexer::Symbol)>,
        (binding, binding_name): (radix_hir::DefId, radix_lexer::Symbol),
        map: &Operand,
        k: crate::ty::Elem,
        v: crate::ty::Elem,
        body: &HirBlock,
        mutable: bool,
    ) -> Result<(), CodegenError> {
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
        let class = v.ty();
        self.need_class(class);
        self.support.need(Helper::List(k, ListOp::New));
        self.support.need(Helper::List(v, ListOp::New));
        let suffix = Self::map_suffix(k, v);
        self.line(out, "{");
        self.depth += 1;
        self.line(
            out,
            &format!("faber_map_{suffix} *const {source} = {};", map.text),
        );
        self.line(
            out,
            &format!("const int64_t {source}len = {source}->keys->len;"),
        );
        self.line(out, &format!("int64_t {index};"));
        self.line(
            out,
            &format!("for ({index} = 0; {index} < {source}->keys->len; {index}++) {{"),
        );
        self.depth += 1;
        self.line(
            out,
            &format!(
                "if ({source}->keys->len != {source}len) faber_trap(\"map changed size during iteration\");"
            ),
        );
        self.line(
            out,
            &format!(
                "{} = {source}->values->items[{index}];",
                self.declare(class, &name, mutable)?
            ),
        );
        if let Some((position, position_name)) = coordinate {
            let Some(position_kind) = self.names.binding_kind(position) else {
                return Err(unsupported(
                    "loop coordinate the name catalog does not classify",
                ));
            };
            let raw = self.names.resolve_symbol(position_name);
            let c_name = self.scope.claim(&prefixed_binding_name(position_kind, raw));
            let declaration = self.declare(k.ty(), &c_name, false)?;
            self.line(
                out,
                &format!("{declaration} = {source}->keys->items[{index}];"),
            );
            self.locals.insert(
                position,
                Local {
                    name: c_name,
                    ty: k.ty(),
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
        self.line(
            out,
            &format!(
                "if ({source}->keys->len != {source}len) faber_trap(\"map changed size during iteration\");"
            ),
        );
        self.depth -= 1;
        self.line(out, "}");
        Ok(())
    }

    /// `for from xs, ys const x, y { … }`: a lockstep walk. Every source is
    /// evaluated once, all must have the same length (a run-time abort as the
    /// runner's, `itera lockstep sources must have equal length`), and the
    /// binders read the aligned element of each. The optional `apud [i]`
    /// coordinate binds the shared index. Typecheck forbids a shape change.
    fn lockstep(
        &mut self,
        out: &mut String,
        sources: &[HirExpression],
        coordinates: &[(radix_hir::DefId, radix_lexer::Symbol)],
        bindings: &[(radix_hir::DefId, radix_lexer::Symbol)],
        body: &HirBlock,
        mutable: bool,
    ) -> Result<(), CodegenError> {
        if sources.is_empty() || sources.len() != bindings.len() {
            return Err(unsupported(
                "lockstep iteration without one source per binder",
            ));
        }
        if coordinates.len() > 1 {
            return Err(unsupported("lockstep iteration with several coordinates"));
        }
        let coordinate = coordinates.first();
        let mut lists = Vec::with_capacity(sources.len());
        for source in sources {
            let value = self.expr(source)?;
            let Value::List(elem) = value.value else {
                return Err(unsupported("lockstep source that is not a list"));
            };
            lists.push((value.text, elem));
        }
        self.support.need(Helper::Trap);
        self.line(out, "{");
        self.depth += 1;
        let mut names = Vec::with_capacity(lists.len());
        for (text, elem) in &lists {
            let name = format!("faber_t{}", self.temps);
            self.temps += 1;
            self.line(
                out,
                &format!("faber_list_{} *const {name} = {text};", elem.suffix()),
            );
            names.push(name);
        }
        for name in &names[1..] {
            self.line(
                out,
                &format!(
                    "if ({name}->len != {}->len) faber_abort(\"itera lockstep sources must have equal length\");",
                    names[0]
                ),
            );
        }
        let index = format!("faber_i{}", self.temps);
        self.temps += 1;
        self.line(out, &format!("int64_t {index};"));
        self.line(
            out,
            &format!(
                "for ({index} = 0; {index} < {}->len; {index}++) {{",
                names[0]
            ),
        );
        self.depth += 1;
        for ((name, (_, elem)), (binding, binding_name)) in names.iter().zip(lists).zip(bindings) {
            let Some(kind) = self.names.binding_kind(*binding) else {
                return Err(unsupported(
                    "loop binding the name catalog does not classify",
                ));
            };
            let raw = self.names.resolve_symbol(*binding_name);
            let c_name = self.scope.claim(&prefixed_binding_name(kind, raw));
            let class = elem.ty();
            self.need_class(class);
            let declaration = self.declare(class, &c_name, mutable)?;
            self.line(out, &format!("{declaration} = {name}->items[{index}];"));
            self.locals.insert(
                *binding,
                Local {
                    name: c_name,
                    ty: class,
                    constant: None,
                },
            );
        }
        if let Some((position, position_name)) = coordinate {
            let Some(kind) = self.names.binding_kind(*position) else {
                return Err(unsupported(
                    "loop coordinate the name catalog does not classify",
                ));
            };
            let raw = self.names.resolve_symbol(*position_name);
            let c_name = self.scope.claim(&prefixed_binding_name(kind, raw));
            let declaration = self.declare(Ty::Int, &c_name, false)?;
            self.line(out, &format!("{declaration} = {index};"));
            self.locals.insert(
                *position,
                Local {
                    name: c_name,
                    ty: Ty::Int,
                    constant: None,
                },
            );
        }
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

    /// `custodi { si <cond> { … } … }`: sequential early-exit guard clauses.
    /// Each is an `if` whose body may `return`, exactly as the runner runs
    /// them before the ordinary body.
    fn custodi(&mut self, out: &mut String, custodi: &HirCustodi) -> Result<(), CodegenError> {
        for clause in &custodi.clauses {
            let test = self.condition(&clause.cond)?;
            self.line(out, &format!("if {test} {{"));
            self.block(out, &clause.body)?;
            self.line(out, "}");
        }
        Ok(())
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
        let mut otherwise = else_block;
        // An `elif` is an `else` block holding one `if`: written flat.
        while let Some(block) = otherwise {
            let chained = match (block.statements.is_empty(), block.expr.as_deref()) {
                (
                    true,
                    Some(HirExpression {
                        kind:
                            HirExpressionKind::Si {
                                cond,
                                then_block,
                                then_catch: None,
                                else_block,
                            },
                        ..
                    }),
                ) => Some((cond, then_block, else_block.as_ref())),
                _ => None,
            };
            if let Some((cond, then_block, next)) = chained {
                let test = self.condition(cond)?;
                self.line(out, &format!("}} else if {test} {{"));
                self.block(out, then_block)?;
                otherwise = next;
            } else {
                self.line(out, "} else {");
                self.block(out, block)?;
                break;
            }
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
            let line = match crate::map::map_class(self.types, base.ty) {
                Some((k, v)) => self.map_write(base, index, value, k, v)?,
                None => self.list_write(base, index, value)?,
            };
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
        let value = self.settle(value, class)?;
        if value.value != Value::from(class) {
            return Err(unsupported("value stored without a store check"));
        }
        // A `ref`/`mut` scalar parameter stores through the caller's pointer.
        let target = if self.ref_params.contains(def_id) {
            format!("(*{name})")
        } else {
            name
        };
        self.line(out, &format!("{target} = {};", value.text));
        Ok(())
    }

    /// `place ↤ source [via HINT] [⊥ recovery]` written as a statement:
    /// convert the source to the place's own type, then store it once.
    fn conversion_statement(
        &mut self,
        out: &mut String,
        target: &HirExpression,
        source: &HirExpression,
        params: &[Symbol],
        recovery: Option<&HirExpression>,
    ) -> Result<(), CodegenError> {
        let Some(ty) = target.ty else {
            return Err(unsupported("conversion assignment without a target type"));
        };
        let value = self.conversion(source, ty, params, recovery)?;
        let HirExpressionKind::Path(def_id) = &target.kind else {
            return Err(unsupported(
                "conversion assignment to a place that is not a local",
            ));
        };
        let Some(local) = self.locals.get(def_id) else {
            return Err(unsupported("assignment to a name that is not a local"));
        };
        let (name, class) = (local.name.clone(), local.ty);
        let value = self.settle(value, class)?;
        if value.value != Value::from(class) {
            return Err(unsupported("value stored without a store check"));
        }
        let target = if self.ref_params.contains(def_id) {
            format!("(*{name})")
        } else {
            name
        };
        self.line(out, &format!("{target} = {};", value.text));
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
        if self.print_joins(args) {
            return self.print_line(out, kind, args);
        }
        // The runner prints each argument as its own line when one of them
        // has no shared rendering (a genus).
        for arg in args {
            self.print_line(out, kind, std::slice::from_ref(arg))?;
        }
        Ok(())
    }

    /// Whether the runner writes these arguments as one space-joined line:
    /// a single argument, a literal text first with up to two more (its
    /// format-string form), or every argument having a shared rendering. A
    /// genus value (and so an enumeration, and an optional one) has none, so
    /// a statement that carries one prints each argument on its own line. A
    /// sized integer renders like any integer.
    fn print_joins(&self, args: &[HirExpression]) -> bool {
        if args.len() <= 1 {
            return true;
        }
        let literal_first = matches!(
            args[0].kind,
            HirExpressionKind::Literal(radix_hir::HirLiteral::String(_))
        );
        if literal_first && args.len() <= 3 {
            return true;
        }
        args.iter()
            .all(|arg| arg.ty.is_some_and(|ty| self.argument_joins(ty)))
    }

    /// Whether one print argument has a shared rendering. A genus (and so an
    /// enumeration, and an optional one) has none, and neither does `ascii` or
    /// bounded text: the runner prints each on its own line. A sized integer
    /// renders like any integer.
    fn argument_joins(&self, ty: radix_types::TypeId) -> bool {
        match self.types.get(ty) {
            radix_types::Type::Alias(_, inner) => self.argument_joins(*inner),
            radix_types::Type::Primitive(radix_types::Primitive::Ascii)
            | radix_types::Type::BoundedAscii { .. }
            | radix_types::Type::BoundedTextus { .. } => false,
            _ => classify(self.types, ty).is_ok_and(|class| {
                !matches!(class, Ty::Genus(_) | Ty::Opt(crate::ty::Inner::Genus(_)))
            }),
        }
    }

    /// One `printf` for `args`, space separated, ending the line.
    fn print_line(
        &mut self,
        out: &mut String,
        kind: HirScribeKind,
        args: &[HirExpression],
    ) -> Result<(), CodegenError> {
        let mut operands = Vec::with_capacity(args.len());
        for arg in args {
            operands.push(self.expr(arg)?);
        }
        let prelude = self.sequence(&mut operands);
        let tokens = self.names.display_tokens();
        let (yes, no) = (
            self.text_literal(&tokens.true_)?,
            self.text_literal(&tokens.false_)?,
        );
        let mut specs = Vec::with_capacity(operands.len());
        let mut values = Vec::with_capacity(operands.len());
        for (arg, value) in args.iter().zip(operands) {
            // Raw-bits values share an integer carrier with ordinary values,
            // so their source type, not the carrier, selects hexadecimal.
            let bits_width = arg
                .ty
                .and_then(|ty| classify(self.types, ty).ok())
                .and_then(|class| match class {
                    Ty::Bits(width) => Some(width),
                    _ => None,
                });
            let (spec, text) = if let Some(width) = bits_width {
                self.support.need_ints();
                (
                    bits_display_spec(width),
                    format!("({}){}", int_width_of_bits(width).c_type(), value.text),
                )
            } else {
                match value.value {
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
                    Value::List(elem) | Value::Set(elem) => {
                        ("%s", self.show_list(&value.text, elem)?)
                    }
                    Value::Map(..) => return Err(unsupported("printing a map")),
                    Value::Genus(def_id) => ("%s", self.show_genus(&value.text, def_id)?),
                    Value::Opt(inner) => ("%s", self.show_option(&value.text, inner)?),
                    Value::Float => (
                        "%s",
                        format!(
                            "faber_f64_text({}, (char[FABER_F64_TEXT]){{0}})",
                            value.text
                        ),
                    ),
                    Value::Float32 => (
                        "%s",
                        format!(
                            "faber_f32_text({}, (char[FABER_F32_TEXT]){{0}})",
                            value.text
                        ),
                    ),
                    Value::Decimal => {
                        self.support.need(Helper::DecimalText);
                        ("%s", format!("faber_dec_fmt(faber_a, {})", value.text))
                    }
                    // A bare `none` prints the locale's none word.
                    Value::None => ("%s", self.text_literal(&tokens.none)?),
                    Value::Func(_) => return Err(unsupported("printing a function value")),
                    Value::Void => return Err(unsupported("printing a call with no result")),
                }
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

/// The full-width hexadecimal format fragment for a raw-bits value.
fn bits_display_spec(width: BitsWidth) -> &'static str {
    match width {
        BitsWidth::B8 => "0x%02\" PRIx8 \"",
        BitsWidth::B16 => "0x%04\" PRIx16 \"",
        BitsWidth::B32 => "0x%08\" PRIx32 \"",
        BitsWidth::B64 => "0x%016\" PRIx64 \"",
    }
}

/// Name the byte offset of the innermost statement a rejection came from, so
/// a corpus tally can say where a construct sits. The `construct` fact is
/// unchanged; only the first (innermost) statement adds the offset.
fn located(mut error: CodegenError, offset: u32) -> CodegenError {
    if error.args.iter().any(|arg| arg.name == "offset") {
        return error;
    }
    error.args.push(radix_diagnostics::DiagnosticArg::new(
        "offset",
        offset.to_string(),
    ));
    error.message = format!("{} (byte {offset})", error.message);
    error
}
