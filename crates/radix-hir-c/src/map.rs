//! Maps (goal unit C5): `map<K, V>` (`tabula`) is an arena-allocated
//! `faber_map_<k>_<v>` handled by pointer, so every bind aliases (R1). Keys
//! and values are `int` or `text`, the classes the corpus maps use. Keys are
//! unique; the storage is parallel lists and a linear scan, and order is
//! unspecified, as the runner's (D19.2).

use crate::Emitter;
use crate::error::unsupported;
use crate::expr::{Operand, Value, wrap};
use crate::helpers::{Helper, ListOp};
use crate::ty::{Elem, Inner, Ty, classify};
use radix_codegen_shared::CodegenError;
use radix_hir::{HirCallArg, HirExpression, HirObjectField, HirObjectKey};
use radix_intrinsics::IntrinsicMirKind;
use radix_lexer::Symbol;
use radix_types::MirCollectionOp;
use std::fmt::Write as _;

impl Emitter<'_> {
    pub(crate) fn map_suffix(&self, k: Elem, v: Elem) -> Result<String, CodegenError> {
        Ok(format!("{}_{}", self.elem_suffix(k)?, self.elem_suffix(v)?))
    }

    /// The C struct type of a map value (`faber_map_i64_text *`).
    pub(crate) fn map_c_type(&self, k: Elem, v: Elem) -> Result<String, CodegenError> {
        Ok(format!("faber_map_{} *", self.map_suffix(k, v)?))
    }

    /// `empty`: a new map with no storage yet.
    pub(crate) fn empty_map(&mut self, k: Elem, v: Elem) -> Result<Operand, CodegenError> {
        self.need_class(Ty::Map(k, v));
        self.support.need(Helper::List(k, ListOp::New));
        self.support.need(Helper::List(v, ListOp::New));
        Ok(Operand::new(
            format!("faber_map_{}_new(faber_a)", self.map_suffix(k, v)?),
            Value::Map(k, v),
        ))
    }

    /// `{ "k": v, … }`: one map, the entries put in source order.
    pub(crate) fn map_entries(
        &mut self,
        k: Elem,
        v: Elem,
        entries: &[HirObjectField],
    ) -> Result<Operand, CodegenError> {
        let map = self.empty_map(k, v)?;
        let target = format!("faber_t{}", self.temps);
        self.temps += 1;
        let mut body = format!("{} {target} = {}; ", self.map_c_type(k, v)?, map.text);
        for entry in entries {
            let key = self.map_key(&entry.key, k)?;
            let Some(value) = &entry.value else {
                return Err(unsupported("shorthand value in a map literal"));
            };
            let value = self.element(value, v)?;
            write!(
                body,
                "faber_map_{}_put(faber_a, {target}, {}, {}); ",
                self.map_suffix(k, v)?,
                key.text,
                value.text
            )
            .ok();
        }
        let text = format!("(__extension__ ({{ {body}{target}; }}))");
        Ok(Operand::new(text, Value::Map(k, v)).with_effect(true))
    }

    /// `{ "k": v, … }`: a JSON object ascribing to a map (the form the
    /// frontend lowers a map literal to). JSON keys are text, so the key
    /// class is `text`; the value class picks each member's literal form.
    pub(crate) fn map_json_literal(
        &mut self,
        k: Elem,
        v: Elem,
        value: &radix_syntax::JsonValue,
    ) -> Result<Operand, CodegenError> {
        if k != Elem::Text {
            return Err(unsupported("non-text key in a map literal"));
        }
        let radix_syntax::JsonValue::Object(members) = value else {
            return Err(unsupported("json literal that is not an object"));
        };
        let map = self.empty_map(k, v)?;
        let target = format!("faber_t{}", self.temps);
        self.temps += 1;
        let mut body = format!("{} {target} = {}; ", self.map_c_type(k, v)?, map.text);
        for member in members {
            let raw = self.names.resolve_symbol(member.key);
            let decoded = radix_codegen_shared::unescape_faber_text(raw);
            let key = self.text_literal(&decoded)?;
            let value = self.json_value(&member.value, v)?;
            write!(
                body,
                "faber_map_{}_put(faber_a, {target}, {key}, {}); ",
                self.map_suffix(k, v)?,
                value.text
            )
            .ok();
        }
        let text = format!("(__extension__ ({{ {body}{target}; }}))");
        Ok(Operand::new(text, Value::Map(k, v)).with_effect(true))
    }

    /// A JSON scalar in the map value class: an integer or a text.
    fn json_value(
        &mut self,
        value: &radix_syntax::JsonValue,
        v: Elem,
    ) -> Result<Operand, CodegenError> {
        use radix_syntax::JsonValue;
        match (value, v) {
            (JsonValue::Numerus(n), Elem::Int) => self.int(i128::from(*n)),
            (JsonValue::String(symbol), Elem::Text) => {
                let raw = self.names.resolve_symbol(*symbol);
                let decoded = radix_codegen_shared::unescape_faber_text(raw);
                Ok(Operand::new(self.text_literal(&decoded)?, Value::Text))
            }
            _ => Err(unsupported("json value outside the map value class")),
        }
    }

    /// A literal or computed key of a map.
    fn map_key(&mut self, key: &HirObjectKey, k: Elem) -> Result<Operand, CodegenError> {
        match key {
            HirObjectKey::String(symbol) | HirObjectKey::Ident(symbol) => {
                if k != Elem::Text {
                    return Err(unsupported("non-text key in a map literal"));
                }
                let raw = self.names.resolve_symbol(*symbol);
                let decoded = radix_codegen_shared::unescape_faber_text(raw);
                Ok(Operand::new(self.text_literal(&decoded)?, Value::Text))
            }
            HirObjectKey::Computed(expr) => self.element(expr, k),
            HirObjectKey::Spread(_) => Err(unsupported("spread in a map literal")),
        }
    }

    /// `m[k]`: a checked read that traps `tabula missing key`, as the runner's
    /// bracket access does.
    pub(crate) fn map_read(
        &mut self,
        base: &HirExpression,
        index: &HirExpression,
        k: Elem,
        v: Elem,
    ) -> Result<Operand, CodegenError> {
        let map = self.expr(base)?;
        let key = self.element(index, k)?;
        let mut operands = [map, key];
        let prelude = self.sequence(&mut operands)?;
        let [map, key] = operands;
        self.need_class(Ty::Map(k, v));
        self.support.need(Helper::List(v, ListOp::New));
        let suffix = self.map_suffix(k, v)?;
        let slot = format!("faber_v{}", self.temps);
        self.temps += 1;
        let c_type = self.elem_c_type(v)?;
        let call = format!(
            "faber_map_{suffix}_get({}, {}, &{slot})",
            map.text, key.text
        );
        let text = format!(
            "(__extension__ ({{ {c_type} {slot} = 0; if (!{call}) {{ faber_trap(\"tabula missing key\"); }} {slot}; }}))"
        );
        Ok(Operand::new(wrap(&prelude, &text), Value::from(v.ty())).with_effect(true))
    }

    /// `m[k] ← v` as a statement line.
    pub(crate) fn map_write(
        &mut self,
        base: &HirExpression,
        index: &HirExpression,
        value: &HirExpression,
        k: Elem,
        v: Elem,
    ) -> Result<String, CodegenError> {
        let map = self.expr(base)?;
        let key = self.element(index, k)?;
        let value = self.element(value, v)?;
        self.support.need(Helper::List(k, ListOp::New));
        self.support.need(Helper::List(v, ListOp::New));
        Ok(format!(
            "faber_map_{}_put(faber_a, {}, {}, {});",
            self.map_suffix(k, v)?,
            map.text,
            key.text,
            value.text
        ))
    }

    /// The methods this target has for a map: `put` / `get` / `has` / `delete`
    /// and the count.
    pub(crate) fn map_method(
        &mut self,
        receiver: &HirExpression,
        method: Symbol,
        args: &[HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let name = self.names.resolve_symbol(method).to_owned();
        let Some(receiver_ty) = receiver.ty else {
            return Err(unsupported(
                "method call on a value without a resolved type",
            ));
        };
        let Some(entry) =
            radix_intrinsics::lookup_method_for_type_id(receiver_ty, self.types, &name, args.len())
        else {
            return Err(unsupported("method call"));
        };
        let map = self.expr(receiver)?;
        let Value::Map(k, v) = map.value else {
            return Err(unsupported("method call on a value that is not a map"));
        };
        if args
            .iter()
            .any(|arg| arg.name.is_some() || arg.spread || arg.marker.is_some())
        {
            return Err(unsupported("named, spread, or marked argument"));
        }
        let suffix = self.map_suffix(k, v)?;
        match entry.mir_kind {
            IntrinsicMirKind::Collection(MirCollectionOp::Length) => {
                let text = format!("({})->keys->len", map.text);
                let narrow = (!map.effect).then(|| crate::expr::Narrow::leaf(&text));
                let mut operand = Operand::new(text, Value::Int).with_effect(map.effect);
                operand.narrow = narrow;
                Ok(operand)
            }
            IntrinsicMirKind::Collection(MirCollectionOp::IsEmpty) => Ok(Operand::new(
                format!("(({})->keys->len == 0)", map.text),
                Value::Truth,
            )
            .with_effect(map.effect)),
            IntrinsicMirKind::Collection(MirCollectionOp::Put) => {
                let [key, value] = args else {
                    return Err(unsupported("map put without a key and a value"));
                };
                let key = self.element(&key.expr, k)?;
                let value = self.element(&value.expr, v)?;
                let mut operands = [map, key, value];
                let prelude = self.sequence(&mut operands)?;
                let [map, key, value] = operands;
                self.need_class(Ty::Map(k, v));
                let call = format!(
                    "faber_map_{suffix}_put(faber_a, {}, {}, {})",
                    map.text, key.text, value.text
                );
                Ok(Operand::new(wrap(&prelude, &call), Value::Void).with_effect(true))
            }
            IntrinsicMirKind::Collection(MirCollectionOp::Index) => self.map_get(map, k, v, args),
            IntrinsicMirKind::Collection(MirCollectionOp::Contains) => {
                let [key] = args else {
                    return Err(unsupported("map has without one key"));
                };
                let key = self.element(&key.expr, k)?;
                let mut operands = [map, key];
                let prelude = self.sequence(&mut operands)?;
                let [map, key] = operands;
                let call = format!("(faber_map_{suffix}_has({}, {}))", map.text, key.text);
                Ok(Operand::new(wrap(&prelude, &call), Value::Truth).with_effect(true))
            }
            IntrinsicMirKind::Collection(MirCollectionOp::Delete) => {
                let [key] = args else {
                    return Err(unsupported("map delete without one key"));
                };
                let key = self.element(&key.expr, k)?;
                let mut operands = [map, key];
                let prelude = self.sequence(&mut operands)?;
                let [map, key] = operands;
                let call = format!("(faber_map_{suffix}_delete({}, {}))", map.text, key.text);
                Ok(Operand::new(wrap(&prelude, &call), Value::Truth).with_effect(true))
            }
            _ => Err(unsupported("map method outside the subset")),
        }
    }

    /// `m.get(k)`: the value or none, through the map's out-parameter helper.
    fn map_get(
        &mut self,
        map: Operand,
        k: Elem,
        v: Elem,
        args: &[HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let [arg] = args else {
            return Err(unsupported("map get without one key"));
        };
        let key = self.element(&arg.expr, k)?;
        let mut operands = [map, key];
        let prelude = self.sequence(&mut operands)?;
        let [map, key] = operands;
        let inner = Inner::from_ty(v.ty())
            .ok_or_else(|| unsupported("map value without an optional form"))?;
        self.need_class(Ty::Map(k, v));
        self.support.need(Helper::List(v, ListOp::New));
        let slot = format!("faber_o{}", self.temps);
        let call = format!(
            "faber_map_{}_get({}, {}, &{slot})",
            self.map_suffix(k, v)?,
            map.text,
            key.text
        );
        let operand = self.optional_call(v, inner, &slot, &call)?;
        Ok(Operand::new(wrap(&prelude, &operand.text), operand.value).with_effect(true))
    }

    /// `k ∈ m` / `k ∉ m`: key membership.
    pub(crate) fn map_membership(
        &mut self,
        negated: bool,
        k: Elem,
        v: Elem,
        map_is_left: bool,
        l: Operand,
        r: Operand,
    ) -> Result<Operand, CodegenError> {
        let (map, key) = if map_is_left { (l, r) } else { (r, l) };
        if key.value != Value::from(k.ty()) {
            return Err(unsupported("map membership with a key of another class"));
        }
        let effect = map.effect || key.effect;
        let mut operands = [map, key];
        let prelude = self.sequence(&mut operands)?;
        let [map, key] = operands;
        self.need_class(Ty::Map(k, v));
        let call = format!(
            "(faber_map_{}_has({}, {}))",
            self.map_suffix(k, v)?,
            map.text,
            key.text
        );
        let text = if negated { format!("(!{call})") } else { call };
        Ok(Operand::new(wrap(&prelude, &text), Value::Truth).with_effect(effect))
    }
}

/// The map class of an expression's type, if it is one.
pub(crate) fn map_class(
    types: &radix_types::TypeTable,
    ty: Option<radix_types::TypeId>,
) -> Option<(Elem, Elem)> {
    match classify(types, ty?).ok()? {
        Ty::Map(k, v) => Some((k, v)),
        _ => None,
    }
}
