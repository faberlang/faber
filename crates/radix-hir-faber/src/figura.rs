//! Compile-time tensor/vector shape pretty-printing for Faber re-emission.
//!
//! Converts `IndexExpr` values to their Faber surface spelling
//! (literal, parameter, or tuple shape). Used by the Faber codegen
//! backend to re-emit type figura annotations in canonical form.
//! Does not perform index evaluation or type inference. The type facts
//! and interner ride the [`super::FaberCodegen`] context.
use radix_codegen_shared::CodegenError;
use radix_diagnostics::DiagnosticArg;
use radix_types::{IndexExpr, IndexId, RuntimeExtentId};

impl super::FaberCodegen<'_> {
    /// Pretty-print an interned index expression as Faber `Figura` syntax.
    pub(super) fn format_figura(&self, index_id: IndexId) -> String {
        match self.types.get_index(index_id) {
            IndexExpr::Literal(value) => value.to_string(),
            IndexExpr::Param(symbol) => self.interner.resolve(*symbol).to_owned(),
            IndexExpr::Affine { base, offset } => {
                let base = self.format_figura(*base);
                if *offset < 0 {
                    format!("{base} - {}", offset.unsigned_abs())
                } else {
                    format!("{base} + {offset}")
                }
            }
            IndexExpr::Divide { numerator, divisor } => {
                format!("({}) / {divisor}", self.format_figura(*numerator))
            }
            IndexExpr::Tuple(dims) if dims.is_empty() => "[]".to_owned(),
            IndexExpr::Tuple(dims) => {
                let parts = dims
                    .iter()
                    .map(|dim| self.format_figura(*dim))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{parts}]")
            }
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
