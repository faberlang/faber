//! Methods and indexing on text.
//!
//! Text is a counted UTF-8 value in this target: a length, an index and a
//! slice count code points, as the runner counts them. The work is in
//! `runtime/c/faber.h` (`faber_text_*`); this only chooses the call.

use crate::Emitter;
use crate::error::unsupported;
use crate::expr::{Operand, Value, wrap};
use crate::helpers::Helper;
use crate::ty::Elem;
use radix_codegen_shared::CodegenError;
use radix_hir::{HirCallArg, HirExpression};
use radix_intrinsics::{IntrinsicMirKind, IntrinsicReturnKind};
use radix_lexer::Symbol;
use radix_types::{MirCollectionOp, TypeId};

/// Which runtime call a text method is, and the shape of its operands.
#[derive(Clone, Copy)]
enum Call {
    Length,
    IsEmpty,
    /// `faber_text_<name>(text, text)`: contains, `starts_with`, `ends_with`.
    Test(&'static str),
    Case(bool),
    Trim,
    Split,
    Replace,
    Slice,
    At,
}

impl Call {
    /// The result's value form, and whether the call allocates.
    fn result(self) -> (Value, Helper) {
        match self {
            Call::Length => (Value::Int, Helper::TextRead),
            Call::IsEmpty | Call::Test(_) => (Value::Truth, Helper::TextRead),
            Call::Case(_) | Call::Trim | Call::Replace | Call::Slice | Call::At => {
                (Value::Text, Helper::TextAlloc)
            }
            Call::Split => (Value::List(Elem::Text), Helper::TextAlloc),
        }
    }

    /// The call over operand texts already sequenced.
    fn text(self, o: &[Operand]) -> String {
        match self {
            Call::Length => format!("faber_text_length({})", o[0].text),
            Call::IsEmpty => format!("(({})[0] == '\\0')", o[0].text),
            Call::Test(name) => format!("(faber_text_{name}({}, {}))", o[0].text, o[1].text),
            Call::Case(upper) => format!(
                "faber_text_case(faber_a, {}, {})",
                o[0].text,
                u8::from(upper)
            ),
            Call::Trim => format!("faber_text_trim(faber_a, {})", o[0].text),
            Call::Split => format!("faber_text_split(faber_a, {}, {})", o[0].text, o[1].text),
            Call::Replace => format!(
                "faber_text_replace(faber_a, {}, {}, {})",
                o[0].text, o[1].text, o[2].text
            ),
            Call::Slice => format!(
                "faber_text_slice(faber_a, {}, {}, {})",
                o[0].text, o[1].text, o[2].text
            ),
            Call::At => format!("faber_text_at(faber_a, {}, {})", o[0].text, o[1].text),
        }
    }
}

impl Emitter<'_> {
    /// `s.length()`, `s.trim()`, `s.slice(a, b)` and the rest of the text
    /// methods this target has.
    pub(crate) fn text_method(
        &mut self,
        receiver: Operand,
        receiver_ty: TypeId,
        method: Symbol,
        args: &[HirCallArg],
    ) -> Result<Operand, CodegenError> {
        let name = self.names.resolve_symbol(method).to_owned();
        let Some(entry) =
            radix_intrinsics::lookup_method_for_type_id(receiver_ty, self.types, &name, args.len())
        else {
            return Err(unsupported("text method"));
        };
        if args
            .iter()
            .any(|arg| arg.name.is_some() || arg.spread || arg.marker.is_some())
        {
            return Err(unsupported("named, spread, or marked argument"));
        }
        let IntrinsicMirKind::Collection(op) = entry.mir_kind else {
            return Err(unsupported("text method outside the subset"));
        };
        let mut operands = vec![receiver];
        for arg in args {
            operands.push(self.expr(&arg.expr)?);
        }
        let texts = |from: usize, operands: &[Operand]| {
            operands[from..]
                .iter()
                .all(|operand| operand.value == Value::Text)
        };
        let index = |operand: &Operand| matches!(operand.value, Value::Int | Value::Exact);
        let count = operands.len();
        let call = match op {
            MirCollectionOp::Length if count == 1 => Call::Length,
            MirCollectionOp::IsEmpty if count == 1 => Call::IsEmpty,
            MirCollectionOp::Contains if count == 2 && texts(0, &operands) => {
                Call::Test("contains")
            }
            MirCollectionOp::StartsWith if count == 2 && texts(0, &operands) => {
                Call::Test("starts_with")
            }
            MirCollectionOp::EndsWith if count == 2 && texts(0, &operands) => {
                Call::Test("ends_with")
            }
            MirCollectionOp::Uppercase if count == 1 => Call::Case(true),
            MirCollectionOp::Lowercase if count == 1 => Call::Case(false),
            MirCollectionOp::Trim if count == 1 => Call::Trim,
            MirCollectionOp::Split if count == 2 && texts(0, &operands) => Call::Split,
            MirCollectionOp::Replace if count == 3 && texts(0, &operands) => Call::Replace,
            MirCollectionOp::Slice
                if count == 3
                    && texts(0, &operands[..1])
                    && index(&operands[1])
                    && index(&operands[2]) =>
            {
                Call::Slice
            }
            // `apud` is the total read; `accipe` (an optional) is not here.
            MirCollectionOp::Index
                if count == 2
                    && texts(0, &operands[..1])
                    && index(&operands[1])
                    && entry.return_kind != IntrinsicReturnKind::OptionTextus =>
            {
                Call::At
            }
            _ => return Err(unsupported("text method outside the subset")),
        };
        let (value, helper) = call.result();
        self.support.need(helper);
        let effect = operands.iter().any(|operand| operand.effect);
        let prelude = self.sequence(&mut operands)?;
        let text = wrap(&prelude, &call.text(&operands));
        // A truth operand carries its own parentheses.
        let text = if value == Value::Truth && !prelude.is_empty() {
            format!("({text})")
        } else {
            text
        };
        Ok(Operand::new(text, value).with_effect(effect))
    }

    /// `s[i]` on text.
    pub(crate) fn text_index(
        &mut self,
        base: Operand,
        index: &HirExpression,
    ) -> Result<Operand, CodegenError> {
        let position = self.expr(index)?;
        if !matches!(position.value, Value::Int | Value::Exact) {
            return Err(unsupported("text index that is not an integer"));
        }
        self.support.need(Helper::TextAlloc);
        let effect = base.effect || position.effect;
        let mut operands = [base, position];
        let prelude = self.sequence(&mut operands)?;
        let call = Call::At.text(&operands);
        Ok(Operand::new(wrap(&prelude, &call), Value::Text).with_effect(effect))
    }
}
