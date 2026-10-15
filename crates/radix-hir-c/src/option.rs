//! Optionals (`T ∪ none`).
//!
//! Text, lists and genera are pointers already, so their none is `NULL` and
//! wrapping a value is the identity. `int`, `bool` and `float` carry a
//! presence flag in the runtime's `faber_opt_<w>` struct. Nothing here reads
//! the value of a none: every read is behind the flag.

use crate::Emitter;
use crate::error::unsupported;
use crate::expr::{Operand, Value, c_string, wrap};
use crate::ty::{Inner, Ty, classify};
use radix_codegen_shared::CodegenError;
use radix_hir::{HirExpression, HirExpressionKind, HirLiteral};

impl Emitter<'_> {
    /// The `null` literal, given the optional class typecheck assigned it.
    pub(crate) fn none_value(&mut self, expr: &HirExpression) -> Result<Operand, CodegenError> {
        let Some(ty) = expr.ty else {
            return Err(unsupported("null without a resolved optional type"));
        };
        let class = classify(self.types, ty)?;
        self.none_of(class)
    }

    /// The none of an optional class.
    pub(crate) fn none_of(&mut self, class: Ty) -> Result<Operand, CodegenError> {
        let Ty::Opt(inner) = class else {
            return Err(unsupported("null where no optional is expected"));
        };
        self.need_class(Ty::Opt(inner));
        let text = match inner.scalar_struct() {
            Some(name) => format!("(({name}){{0}})"),
            None => "NULL".to_owned(),
        };
        Ok(Operand::new(text, Value::Opt(inner)))
    }

    /// A present value of the class `inner` carries, as the optional.
    pub(crate) fn some_value(&mut self, value: &Operand, inner: Inner) -> Operand {
        self.need_class(Ty::Opt(inner));
        let text = match inner.scalar_struct() {
            Some(name) => format!("(({name}){{1, {}}})", value.text),
            None => value.text.clone(),
        };
        let mut operand = Operand::new(text, Value::Opt(inner)).with_effect(value.effect);
        operand.stored = Some(Ty::Opt(inner));
        operand
    }

    /// `x is none` / `x not is none` (and `≡ null` / `≢ null`).
    pub(crate) fn none_test(
        &mut self,
        is_none: bool,
        subject: &HirExpression,
    ) -> Result<Operand, CodegenError> {
        let value = self.expr(subject)?;
        let Value::Opt(inner) = value.value else {
            return Err(unsupported("none test of a value that is not optional"));
        };
        let text = match (inner.is_pointer(), is_none) {
            (true, true) => format!("({} == NULL)", value.text),
            (true, false) => format!("({} != NULL)", value.text),
            (false, true) => format!("(!({}).has)", value.text),
            (false, false) => format!("(({}).has)", value.text),
        };
        Ok(Operand::new(text, Value::Truth).with_effect(value.effect))
    }

    /// `a coalesce b`: `a`'s value when present, else `b`, which runs only then.
    pub(crate) fn coalesce(
        &mut self,
        left: &HirExpression,
        right: &HirExpression,
    ) -> Result<Operand, CodegenError> {
        let l = self.expr(left)?;
        let Value::Opt(inner) = l.value else {
            // A left side that cannot be none is its own value: the fallback
            // never runs, so it is not evaluated.
            return Ok(l);
        };
        let r = self.expr(right)?;
        let class = inner.ty();
        let r = if class == Ty::Int { self.fit(r)? } else { r };
        let n = self.temps;
        self.temps += 1;
        let t = format!("faber_c{n}");
        let effect = l.effect || r.effect;
        let (prelude, present, result) = if r.value == Value::Opt(inner) {
            // `a coalesce b` with `b` optional too: still optional.
            match inner.scalar_struct() {
                Some(name) => (
                    format!("{name} {t} = {}; ", l.text),
                    format!("{t}.has"),
                    (format!("{t}"), r.text, Value::Opt(inner)),
                ),
                None => (
                    format!("{} {t} = {}; ", self.c_type(class)?, l.text),
                    format!("{t} != NULL"),
                    (t.clone(), r.text, Value::Opt(inner)),
                ),
            }
        } else if r.value == Value::from(class) {
            match inner.scalar_struct() {
                Some(name) => (
                    format!("{name} {t} = {}; ", l.text),
                    format!("{t}.has"),
                    (format!("{t}.value"), r.text, Value::from(class)),
                ),
                None => (
                    format!("{} {t} = {}; ", self.c_type(class)?, l.text),
                    format!("{t} != NULL"),
                    (t.clone(), r.text, Value::from(class)),
                ),
            }
        } else {
            return Err(unsupported("coalesce fallback of a different class"));
        };
        let (taken, fallback, value) = result;
        let body = format!("{present} ? {taken} : {fallback}");
        let text = wrap(&prelude, &body);
        let text = if value == Value::Truth {
            format!("({text})")
        } else {
            text
        };
        Ok(Operand::new(text, value).with_effect(effect))
    }

    /// `x!`: the value, or an abort when `x` is none.
    pub(crate) fn unwrap(&mut self, subject: &HirExpression) -> Result<Operand, CodegenError> {
        let value = self.expr(subject)?;
        let Value::Opt(inner) = value.value else {
            return Ok(value);
        };
        let n = self.temps;
        self.temps += 1;
        let t = format!("faber_c{n}");
        let class = inner.ty();
        self.support.need(crate::helpers::Helper::Trap);
        let abort = "faber_abort(\"unwrap of nihil\");";
        let (text, form) = match inner.scalar_struct() {
            Some(name) => (
                format!(
                    "{name} {t} = {}; if (!{t}.has) {{ {abort} }} {t}.value;",
                    value.text
                ),
                Value::from(class),
            ),
            None => (
                format!(
                    "{} {t} = {}; if ({t} == NULL) {{ {abort} }} {t};",
                    self.c_type(class)?,
                    value.text
                ),
                Value::from(class),
            ),
        };
        let text = format!("(__extension__ ({{ {text} }}))");
        let text = if form == Value::Truth {
            format!("({text})")
        } else {
            text
        };
        Ok(Operand::new(text, form).with_effect(value.effect))
    }

    /// A read of the optional `value` where typecheck proved it present
    /// (`expr.ty` is the narrowed class): its payload, with no check.
    pub(crate) fn narrow_read(value: &Operand, inner: Inner) -> Operand {
        let form = Value::from(inner.ty());
        let text = if inner.is_pointer() {
            value.text.clone()
        } else if inner == Inner::Bool {
            format!("(({}).value)", value.text)
        } else {
            format!("({}).value", value.text)
        };
        let mut operand = Operand::new(text, form).with_effect(value.effect);
        if inner == Inner::Int {
            operand.narrow = Some(crate::expr::Narrow::leaf(&operand.text));
        }
        operand
    }

    /// How an optional prints: its value as the runner shows the payload, or
    /// the locale's none word.
    pub(crate) fn show_option(&mut self, text: &str, inner: Inner) -> Result<String, CodegenError> {
        let none = c_string(&self.names.display_tokens().none.clone())?;
        let n = self.temps;
        self.temps += 1;
        let t = format!("faber_c{n}");
        let class = inner.ty();
        let (declaration, present, payload) = match inner.scalar_struct() {
            Some(name) => {
                let read = format!("{t}.value");
                let payload = if class == Ty::Bool {
                    Operand::new(format!("({read})"), Value::Truth)
                } else {
                    Operand::new(read, Value::from(class))
                };
                (format!("{name} {t} = {text};"), format!("{t}.has"), payload)
            }
            None => (
                format!("{} {t} = {text};", self.c_type(class)?),
                format!("{t} != NULL"),
                Operand::new(t.clone(), Value::from(class)),
            ),
        };
        let shown = self.show(&payload)?;
        Ok(format!(
            "(__extension__ ({{ {declaration} {present} ? {shown} : {none}; }}))"
        ))
    }

    /// Whether a literal is the null literal.
    pub(crate) fn is_none_literal(expr: &HirExpression) -> bool {
        matches!(expr.kind, HirExpressionKind::Literal(HirLiteral::Nil))
    }
}
