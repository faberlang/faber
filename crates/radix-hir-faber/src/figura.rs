//! Compile-time tensor/vector shape pretty-printing for Faber re-emission.
//!
//! Converts `IndexExpr` values to their Faber surface spelling
//! (literal, parameter, or tuple shape). Used by the Faber codegen
//! backend to re-emit type figura annotations in canonical form.
//! Does not perform index evaluation or type inference. The type facts
//! and interner ride the [`super::FaberCodegen`] context.
use radix_codegen_shared::CodegenError;
use radix_diagnostics::DiagnosticArg;
use radix_lexer::TokenKind;
use radix_types::{IndexExpr, IndexId, RuntimeExtentId};

use crate::KeywordSurface;

impl super::FaberCodegen<'_> {
    /// Pretty-print an interned index expression as Faber `Figura` syntax.
    ///
    /// An extent that depends on a runtime value is spelled as a size lift
    /// (`size n`, `size (n - 1)`; la `magnitudo`), because a bare value name
    /// in an extent position is refused (SEM008
    /// `annotation_extent_bare_value_name`). Literal and size-parameter
    /// extents keep their bare spelling. A tuple lifts each dimension on its
    /// own.
    pub(super) fn format_figura(&self, index_id: IndexId, surface: &impl KeywordSurface) -> String {
        match self.types.get_index(index_id) {
            IndexExpr::Tuple(dims) if dims.is_empty() => "[]".to_owned(),
            IndexExpr::Tuple(dims) => {
                let parts = dims
                    .iter()
                    .map(|dim| self.format_figura(*dim, surface))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{parts}]")
            }
            _ if !self.index_has_runtime(index_id) => self.format_figura_term(index_id, surface),
            IndexExpr::Runtime(_) => {
                let name = self.format_figura_term(index_id, surface);
                if name == "<error>" {
                    // Unrepresentable origin: the recorded emission error
                    // fails the output closed; do not dress it as a lift.
                    return name;
                }
                format!("{} {name}", surface.keyword(TokenKind::Magnitudo))
            }
            _ => {
                let term = self.format_figura_term(index_id, surface);
                format!("{} ({term})", surface.keyword(TokenKind::Magnitudo))
            }
        }
    }

    /// A vector lane count in the long form `vector<T, size N>`. The `_` hole
    /// stays bare, a run-time count already carries its own `size` lift, and
    /// any other term takes the marker (parenthesized when it is not a single
    /// literal or name).
    pub(super) fn vector_width_to_faber(
        &self,
        width: IndexId,
        surface: &impl KeywordSurface,
    ) -> String {
        match self.types.get_index(width) {
            IndexExpr::Infer(_) | IndexExpr::Unspecified => "_".to_owned(),
            IndexExpr::Tuple(_) => self.format_figura(width, surface),
            _ if self.index_has_runtime(width) => self.format_figura(width, surface),
            IndexExpr::Literal(_) | IndexExpr::Param(_) => format!(
                "{} {}",
                surface.keyword(TokenKind::Magnitudo),
                self.format_figura_term(width, surface)
            ),
            _ => format!(
                "{} ({})",
                surface.keyword(TokenKind::Magnitudo),
                self.format_figura_term(width, surface)
            ),
        }
    }

    /// A bounded head's count spelled after the `size` marker: `size 4` for the
    /// maximum length of a list, queue, stack or string. The `_` hole stays
    /// bare (its own need moves it).
    pub(super) fn list_count_to_faber(
        &self,
        capacity: IndexId,
        surface: &impl KeywordSurface,
    ) -> String {
        match self.types.get_index(capacity) {
            IndexExpr::Infer(_) | IndexExpr::Unspecified => "_".to_owned(),
            // A runtime count already carries its own `size` lift.
            _ if self.index_has_runtime(capacity) => self.format_figura(capacity, surface),
            _ => format!(
                "{} {}",
                surface.keyword(TokenKind::Magnitudo),
                self.format_figura_term(capacity, surface)
            ),
        }
    }

    /// Whether an extent term reads a runtime value anywhere in its
    /// affine/divide chain.
    fn index_has_runtime(&self, index_id: IndexId) -> bool {
        match self.types.get_index(index_id) {
            IndexExpr::Runtime(_) => true,
            IndexExpr::Affine { base, .. } => self.index_has_runtime(*base),
            IndexExpr::Divide { numerator, .. } => self.index_has_runtime(*numerator),
            IndexExpr::Tuple(dims) => dims.iter().any(|dim| self.index_has_runtime(*dim)),
            IndexExpr::Literal(_)
            | IndexExpr::Param(_)
            | IndexExpr::Infer(_)
            | IndexExpr::Unspecified => false,
        }
    }

    /// The unlifted spelling of one extent term; callers lift it when it
    /// reads a runtime value.
    fn format_figura_term(&self, index_id: IndexId, surface: &impl KeywordSurface) -> String {
        match self.types.get_index(index_id) {
            IndexExpr::Literal(value) => value.to_string(),
            IndexExpr::Param(symbol) => self.interner.resolve(*symbol).to_owned(),
            IndexExpr::Affine { base, offset } => {
                let base = self.format_figura_term(*base, surface);
                if *offset < 0 {
                    format!("{base} - {}", offset.unsigned_abs())
                } else {
                    format!("{base} + {offset}")
                }
            }
            IndexExpr::Divide { numerator, divisor } => {
                format!(
                    "({}) / {divisor}",
                    self.format_figura_term(*numerator, surface)
                )
            }
            // Callers hand tuples to `format_figura`; recursing keeps the
            // match total without a panic arm.
            IndexExpr::Tuple(_) => self.format_figura(index_id, surface),
            IndexExpr::Runtime(source) => self.format_runtime_extent(source),
            IndexExpr::Infer(_) | IndexExpr::Unspecified => "_".to_owned(),
        }
    }

    fn format_runtime_extent(&self, source: &RuntimeExtentId) -> String {
        let symbol = match source {
            RuntimeExtentId::ScalarBinding(def_id) => {
                self.runtime_value_names.borrow().get(def_id).copied()
            }
            RuntimeExtentId::ScalarParameter { parameter } => self
                .runtime_parameter_names
                .borrow()
                .get(parameter)
                .copied(),
            RuntimeExtentId::TensorBindingAxis { .. } | RuntimeExtentId::TensorFieldAxis { .. } => {
                None
            }
            RuntimeExtentId::ScalarExpression { .. }
            | RuntimeExtentId::ExpressionAxis { .. }
            | RuntimeExtentId::ExpressionFieldAxis { .. }
            | RuntimeExtentId::CallArgumentAxis { .. }
            | RuntimeExtentId::CallArgumentFieldAxis { .. }
            | RuntimeExtentId::FunctionParameterAxis { .. }
            | RuntimeExtentId::FunctionParameterFieldAxis { .. } => None,
        };
        if let Some(symbol) = symbol {
            return self.symbol_to_string(symbol);
        }

        self.record_type_emission_error(CodegenError {
            message: "runtime-origin dimension has no source Figura spelling".to_owned(),
            args: vec![DiagnosticArg::new(
                "issue",
                "runtime_extent_unrepresentable_in_source",
            )],
        })
    }
}

#[cfg(test)]
#[path = "figura_test.rs"]
mod tests;
