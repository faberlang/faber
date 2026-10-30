//! Optionals (`T ∪ none`).
//!
//! Text, lists and genera are pointers already, so their none is `NULL` and
//! wrapping a value is the identity. `int`, `bool` and `float` carry a
//! presence flag in the runtime's `faber_opt_<w>` struct. Nothing here reads
//! the value of a none: every read is behind the flag.

use crate::Emitter;
use crate::error::unsupported;
use crate::expr::{Operand, Value, wrap};
use crate::helpers::Helper;
use crate::ty::{Elem, Inner, IntWidth, Ty, classify};
use radix_codegen_shared::CodegenError;
use radix_hir::{HirExpression, HirExpressionKind, HirLiteral};

impl Emitter<'_> {
    /// `a?.b`: none when `a` is none, else the field `b` as an optional. A
    /// field that is itself an optional is the result as it stands, so a
    /// chain `a?.b?.c` is one level of optional throughout.
    pub(crate) fn optional_member(
        &mut self,
        expr: &HirExpression,
        subject: &HirExpression,
        kind: &radix_hir::HirOptionalChainKind,
    ) -> Result<Operand, CodegenError> {
        let name = match kind {
            radix_hir::HirOptionalChainKind::Member(name) => name,
            radix_hir::HirOptionalChainKind::Index(index) => {
                return self.optional_index(subject, index);
            }
            radix_hir::HirOptionalChainKind::Call(_) => {
                return Err(unsupported("optional call chaining"));
            }
        };
        let base = self.expr(subject)?;
        // Optional chaining over `none` is `none` (the runner reads no field).
        if base.value == Value::None {
            return Ok(base);
        }
        // A genus that is not optional is always present: the chain is then
        // the plain field read, wrapped.
        let (Value::Opt(Inner::Genus(def_id)) | Value::Genus(def_id)) = base.value else {
            return Err(unsupported(
                "optional chaining over a value that is not an optional genus",
            ));
        };
        let Some(ty) = expr.ty else {
            return Err(unsupported("optional chaining without a resolved type"));
        };
        let Ty::Opt(inner) = classify(self.types, ty)? else {
            return Err(unsupported("optional chaining that is not an optional"));
        };
        let n = self.temps;
        self.temps += 1;
        let held = format!("faber_c{n}");
        let object = Operand::new(held.clone(), Value::Genus(def_id));
        let field = self.field_from(&object, *name)?;
        let present = match field.value {
            Value::Opt(_) => field,
            _ => self.some_value(&field, inner),
        };
        let none = self.none_of(Ty::Opt(inner))?;
        let c_type = self.c_type(Ty::Genus(def_id))?;
        let text = format!(
            "(__extension__ ({{ {c_type}{held} = {}; {held} == NULL ? {} : {}; }}))",
            base.text, none.text, present.text
        );
        Ok(Operand::new(text, Value::Opt(inner)).with_effect(base.effect))
    }

    /// `list?[i]`: the element at `i` as an optional, none past either end
    /// (the list's `get`).
    fn optional_index(
        &mut self,
        subject: &HirExpression,
        index: &HirExpression,
    ) -> Result<Operand, CodegenError> {
        let list = self.expr(subject)?;
        match list.value {
            Value::Map(k, v) => return self.optional_map_index(list, index, k, v, false),
            Value::Opt(Inner::Map(k, v)) => {
                return self.optional_map_index(list, index, k, v, true);
            }
            _ => {}
        }
        let Value::List(elem) = list.value else {
            return Err(unsupported(
                "optional index over a value that is not a list or map",
            ));
        };
        let at = self.expr(index)?;
        if !matches!(at.value, Value::Int | Value::Exact) {
            return Err(unsupported("list position that is not an integer"));
        }
        if matches!(elem, Elem::Genus(_)) {
            return Err(unsupported("optional index on a list of genus"));
        }
        let Some(inner) = Inner::from_ty(elem.ty()) else {
            return Err(unsupported(
                "optional index over a list of this element class",
            ));
        };
        let effect = list.effect || at.effect;
        let mut operands = [list, at];
        let prelude = self.sequence(&mut operands)?;
        let [list, at] = operands;
        self.support
            .need(Helper::List(elem, crate::helpers::ListOp::At));
        self.need_class(Ty::Opt(inner));
        let slot = format!("faber_o{}", self.temps);
        let call = format!(
            "faber_list_{}_get({}, {}, &{slot})",
            self.elem_suffix(elem)?,
            list.text,
            at.text
        );
        let operand = self.optional_call(elem, inner, &slot, &call)?;
        Ok(Operand::new(wrap(&prelude, &operand.text), operand.value).with_effect(effect))
    }

    /// `m?["k"]`: the map's value as an optional, none when the key is absent
    /// (and, for an optional map, when the map itself is none).
    fn optional_map_index(
        &mut self,
        base: Operand,
        index: &HirExpression,
        k: Elem,
        v: Elem,
        optional_map: bool,
    ) -> Result<Operand, CodegenError> {
        let key = self.element(index, k)?;
        let effect = base.effect || key.effect;
        let mut operands = [base, key];
        let prelude = self.sequence(&mut operands)?;
        let [map, key] = operands;
        self.need_class(Ty::Map(k, v));
        let Some(inner) = Inner::from_ty(v.ty()) else {
            return Err(unsupported("optional map index of this value class"));
        };
        self.need_class(Ty::Opt(inner));
        let suffix = self.map_suffix(k, v)?;
        let slot = format!("faber_v{}", self.temps);
        let call = format!(
            "faber_map_{suffix}_get({}, {}, &{slot})",
            map.text, key.text
        );
        let operand = self.optional_call(v, inner, &slot, &call)?;
        if !optional_map {
            return Ok(
                Operand::new(wrap(&prelude, &operand.text), operand.value).with_effect(effect)
            );
        }
        let none = self.none_of(Ty::Opt(inner))?;
        let c_type = self.c_type(Ty::Map(k, v))?;
        let held = format!("faber_m{}", self.temps);
        self.temps += 1;
        let text = format!(
            "(__extension__ ({{ {c_type} {held} = {}; {held} == NULL ? {} : {}; }}))",
            map.text, none.text, operand.text
        );
        Ok(Operand::new(wrap(&prelude, &text), Value::Opt(inner)).with_effect(effect))
    }

    /// `x![i]`: the assertion has run in `unwrap`; what is left is the
    /// plain index read on what it left.
    pub(crate) fn asserted_index(
        &mut self,
        base: Operand,
        index: &HirExpression,
    ) -> Result<Operand, CodegenError> {
        if base.value == Value::Text {
            return self.text_index(base, index);
        }
        if let Value::List(elem) = base.value {
            let position = self.expr(index)?;
            if !matches!(position.value, Value::Int | Value::Exact) {
                return Err(unsupported("list index that is not an integer"));
            }
            return Ok(self.list_at_operands(base, position, elem)?);
        }
        Err(unsupported(
            "non-null assertion followed by an index on this class",
        ))
    }

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
        // A bare `none` value: the only value it can hold.
        if class == Ty::None {
            return Ok(Operand::new("(0)".to_owned(), Value::None));
        }
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

    /// A read that wants the optional value itself, not a guard-narrowed
    /// payload: a direct path read is taken unwidened, because only a path
    /// read narrows. A nested base read (`a.b`) still narrows normally.
    fn optional_subject(&mut self, subject: &HirExpression) -> Result<Operand, CodegenError> {
        match &subject.kind {
            HirExpressionKind::Path(def_id) => self.path(*def_id),
            _ => self.expr(subject),
        }
    }

    /// `x is none` / `x not is none` (and `≡ null` / `≢ null`).
    pub(crate) fn none_test(
        &mut self,
        is_none: bool,
        subject: &HirExpression,
    ) -> Result<Operand, CodegenError> {
        let value = self.optional_subject(subject)?;
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
        // The left side wants the optional value itself, not a guard-narrowed
        // payload: `x coalesce y` tests the optional.
        let l = self.optional_subject(left)?;
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
                    (t.clone(), r.text, Value::Opt(inner)),
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
        } else if form == Value::Exact {
            // A `u64` slot holds a `uint64_t`; the exact form is `faber_i128`.
            format!("((faber_i128)({}).value)", value.text)
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
        // The generic payload path would print a `bN` as a decimal, but bits
        // are shown as hex at their own width (unit 3e80d069-1.4).
        if matches!(inner, Inner::Bits(_)) {
            return Err(unsupported("display of an optional raw-bits value"));
        }
        let none = self.text_literal(&self.names.display_tokens().none.clone())?;
        let n = self.temps;
        self.temps += 1;
        let t = format!("faber_c{n}");
        let class = inner.ty();
        let (declaration, present, payload) = match inner.scalar_struct() {
            Some(name) => {
                let read = format!("{t}.value");
                let payload = if class == Ty::Bool {
                    Operand::new(format!("({read})"), Value::Truth)
                } else if class == Ty::Modular(IntWidth::U64) {
                    Operand::new(format!("((faber_i128){read})"), Value::Exact)
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
