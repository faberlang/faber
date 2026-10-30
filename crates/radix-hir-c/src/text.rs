//! Text built at run time (goal unit C5): `a + b` on text and `"§"(x)`
//! templates. Both allocate in the arena the function received.

use crate::Emitter;
use crate::error::unsupported;
use crate::expr::{Operand, Value};
use crate::helpers::Helper;
use radix_codegen_shared::CodegenError;
use radix_hir::HirExpression;
use radix_lexer::Symbol;

/// One piece of a template: literal text, or the argument a hole shows.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Piece {
    Text(String),
    Hole(usize),
}

/// Split a template on its holes. `§` shows the next argument in order, `§N`
/// shows argument `N` (0-based); the two mix, and a bare `§` counts only bare
/// holes, as the runner does (`"§, §, §0"(a, b)` shows `a, b, a`).
///
/// The split runs on the raw source text and each literal piece is decoded
/// afterwards, so a `\u` escape that decodes to `§` stays text. Decoding
/// follows ordinary strings and the Rust target; the runner prints template
/// escapes undecoded (goal finding, 2026-10-14).
pub(crate) fn template_pieces(raw: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let mut next = 0;
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '§' {
            literal.push(ch);
            continue;
        }
        let mut digits = String::new();
        while let Some(digit) = chars.next_if(char::is_ascii_digit) {
            digits.push(digit);
        }
        let index = if let Ok(index) = digits.parse::<usize>() {
            index
        } else {
            next += 1;
            next - 1
        };
        if !literal.is_empty() {
            let decoded = radix_codegen_shared::unescape_faber_text(&literal);
            pieces.push(Piece::Text(decoded));
            literal.clear();
        }
        pieces.push(Piece::Hole(index));
    }
    if !literal.is_empty() {
        pieces.push(Piece::Text(radix_codegen_shared::unescape_faber_text(
            &literal,
        )));
    }
    pieces
}

/// True when `text` is a plain C name, or one in the parentheses a truth
/// operand carries: reading it twice, or not at all, cannot show.
fn is_name(text: &str) -> bool {
    let bare = text
        .strip_prefix('(')
        .and_then(|inner| inner.strip_suffix(')'))
        .unwrap_or(text);
    let mut chars = bare.chars();
    chars
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

impl Emitter<'_> {
    /// `a + b` on text: one arena allocation holding both.
    pub(crate) fn concat(&mut self, l: Operand, r: Operand) -> Result<Operand, CodegenError> {
        self.support.need(Helper::Concat);
        let effect = l.effect || r.effect;
        let mut operands = [l, r];
        let prelude = self.sequence(&mut operands)?;
        let [l, r] = operands;
        let call = format!("faber_concat(faber_a, {}, {})", l.text, r.text);
        Ok(Operand::new(crate::expr::wrap(&prelude, &call), Value::Text).with_effect(effect))
    }

    /// `"…§…"(args)`: every argument that is not a constant or a plain name
    /// is evaluated once, in order, into a temporary first, because a hole may
    /// show an argument twice or not at all and Faber evaluates each argument
    /// exactly once. Then one `faber_join` builds the text.
    pub(crate) fn template(
        &mut self,
        template: Symbol,
        args: &[HirExpression],
    ) -> Result<Operand, CodegenError> {
        let mut operands = Vec::with_capacity(args.len());
        for arg in args {
            operands.push(self.expr(arg)?);
        }
        let effect = operands.iter().any(|operand| operand.effect);
        let mut prelude = String::new();
        for operand in &mut operands {
            if operand.constant.is_none() && !is_name(&operand.text) {
                prelude.push_str(&self.bind(operand)?);
            }
        }
        let shown = operands
            .iter()
            .map(|operand| self.show(operand))
            .collect::<Result<Vec<_>, _>>()?;
        let raw = self.names.resolve_symbol(template).to_owned();
        let mut parts = Vec::new();
        for piece in template_pieces(&raw) {
            match piece {
                Piece::Text(text) => parts.push(self.text_literal(&text)?),
                Piece::Hole(index) => match shown.get(index) {
                    Some(text) => parts.push(text.clone()),
                    None => return Err(unsupported("template hole without an argument")),
                },
            }
        }
        let text = match parts.as_slice() {
            [] => "\"\"".to_owned(),
            // One piece is already the text: a join would allocate and copy
            // the same immutable bytes a second time.
            [only] => crate::expr::wrap(&prelude, only),
            many => {
                self.support.need(Helper::Join);
                let call = format!("faber_join(faber_a, {}, {})", many.len(), many.join(", "));
                crate::expr::wrap(&prelude, &call)
            }
        };
        Ok(Operand::new(text, Value::Text).with_effect(effect))
    }

    /// The text a hole shows for one value, as `print` shows it.
    pub(crate) fn show(&mut self, operand: &Operand) -> Result<String, CodegenError> {
        match operand.value {
            Value::Text => Ok(operand.text.clone()),
            Value::Int => {
                self.support.need(Helper::IntText);
                Ok(format!("faber_int_text(faber_a, {})", operand.text))
            }
            Value::Exact => {
                self.support.need(Helper::ExactText);
                Ok(format!("faber_exact_text(faber_a, {})", operand.text))
            }
            Value::Truth => {
                let tokens = self.names.display_tokens();
                let (yes, no) = (
                    self.text_literal(&tokens.true_)?,
                    self.text_literal(&tokens.false_)?,
                );
                Ok(format!("({} ? {yes} : {no})", operand.text))
            }
            Value::Decimal => {
                self.support.need(Helper::DecimalText);
                Ok(format!("faber_dec_fmt(faber_a, {})", operand.text))
            }
            Value::List(elem) | Value::Set(elem) => self.show_list(&operand.text, elem),
            Value::Map(..) => Err(unsupported("showing a map")),
            Value::Genus(def_id) => self.show_genus(&operand.text, def_id),
            Value::Float => {
                self.support.need(Helper::FloatText);
                Ok(format!("faber_float_text(faber_a, {})", operand.text))
            }
            Value::Float32 => {
                self.support.need(Helper::FloatText);
                Ok(format!("faber_float_text_f32(faber_a, {})", operand.text))
            }
            Value::Opt(inner) => self.show_option(&operand.text, inner),
            Value::None => Ok(self.text_literal(&self.names.display_tokens().none)?),
            Value::Void => Err(unsupported("showing a call with no result")),
            Value::Func(_) => Err(unsupported("showing a function value")),
        }
    }
}
