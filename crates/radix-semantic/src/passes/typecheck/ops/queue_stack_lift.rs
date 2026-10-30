//! Queue and stack product glyphs (the 2026-10-04 aggregate operator rulings,
//! C5 rider).
//!
//! Two queues or two stacks pair by position under `+ -` and under the glyph
//! forms `⊙ ⊘`. `*` and `/` between two of them are errors that name `⊙` and
//! `⊘`, exactly as for a list; a scalar still broadcasts on `* / %`. The glyph
//! forms lower to the same collection runtime ops as the list glyphs.

use super::{
    GLYPH_ELEMENT_TYPE_MISMATCH, GLYPH_OP_NUMERIC_ELEMENT_REQUIRED, GLYPH_OPERANDS_UNSUPPORTED,
    HirBinOp, QueueStackArithmeticKind, TypeChecker, TypeId,
};

/// `queue * queue` / `stack * stack` is not elementwise; the glyph says which
/// product is meant.
pub(super) const ISSUE_QUEUE_STACK_MULTIPLY_REQUIRES_GLYPH: &str =
    "queue_stack_multiply_requires_glyph";
/// `queue / queue` / `stack / stack` is not elementwise; the glyph names it.
pub(super) const ISSUE_QUEUE_STACK_DIVIDE_REQUIRES_GLYPH: &str =
    "queue_stack_divide_requires_glyph";

impl TypeChecker<'_> {
    /// `⊙` and `⊘` over two queues or two stacks of one kind: the result keeps
    /// the kind, the shared element type and the smaller bounded capacity.
    /// `None` means neither operand is a queue or stack, so the caller reports
    /// the glyph's own operand diagnostic.
    pub(super) fn check_queue_stack_hadamard(
        &mut self,
        op: HirBinOp,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        span: radix_lexer::Span,
    ) -> Option<TypeId> {
        let lhs = self.queue_stack_arithmetic_shape(lhs_ty);
        let rhs = self.queue_stack_arithmetic_shape(rhs_ty);
        if lhs.is_none() && rhs.is_none() {
            return None;
        }
        let (Some(left), Some(right)) = (lhs, rhs) else {
            self.glyph_invalid_operand(op, GLYPH_OPERANDS_UNSUPPORTED, span);
            return Some(self.error_type);
        };
        if left.kind != right.kind {
            self.glyph_invalid_operand(op, GLYPH_OPERANDS_UNSUPPORTED, span);
            return Some(self.error_type);
        }
        if !self.is_numeric_queue_stack_element(left.element)
            || !self.is_numeric_queue_stack_element(right.element)
        {
            self.glyph_invalid_operand(op, GLYPH_OP_NUMERIC_ELEMENT_REQUIRED, span);
            return Some(self.error_type);
        }
        if !self.numeric_shapes_match(left.element, right.element) {
            self.glyph_invalid_operand(op, GLYPH_ELEMENT_TYPE_MISMATCH, span);
            return Some(self.error_type);
        }
        let Some(capacity) = self.queue_stack_min_capacity(left.capacity, right.capacity) else {
            self.glyph_invalid_operand(op, GLYPH_OPERANDS_UNSUPPORTED, span);
            return Some(self.error_type);
        };
        Some(self.queue_stack_result(left.kind, left.element, capacity))
    }

    /// The queue or stack type an elementwise result takes.
    pub(super) fn queue_stack_result(
        &mut self,
        kind: QueueStackArithmeticKind,
        element: TypeId,
        capacity: Option<radix_types::IndexId>,
    ) -> TypeId {
        match (kind, capacity) {
            (QueueStackArithmeticKind::Queue, Some(capacity)) => {
                self.types.bounded_queue(element, capacity)
            }
            (QueueStackArithmeticKind::Queue, None) => self.types.queue(element),
            (QueueStackArithmeticKind::Stack, Some(capacity)) => {
                self.types.bounded_stack(element, capacity)
            }
            (QueueStackArithmeticKind::Stack, None) => self.types.stack(element),
        }
    }
}
