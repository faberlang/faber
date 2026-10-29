//! Higher-order collection method lowering (`omnia`, `quilibet`, `reperta`, `filtrata`,
//! `mappata`, `reducta`, `cumulata`, keyed `ordina`/`ordinata`).
//!
//! Collection transforms lower into explicit MIR loops over array receivers.
//! Each callback is emitted as a synthetic closure function, then invoked once
//! per element through the shared callable lowering path. Predicate folds
//! settle on the first decisive result; filtered results append only when the
//! predicate returns true; mapped results append the callback return value
//! directly; reduced results update an accumulator; cumulated results append
//! the accumulator after every fold step (serial prefix scan).
//!
//! INVARIANTS
//! ==========
//! - Only lista receivers are supported in this phase (exact-bound
//!   `BoundedArray` literals included — the loop reads length at runtime).
//! - Filter/map callbacks are single-parameter closures with non-failable
//!   signatures; reduce callbacks are two-parameter closures.
//! - Loop blocks are created before any terminator seals the current block.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;

impl FunctionBuilder<'_> {
    pub(super) fn lower_filtrata(
        &mut self,
        receiver: &HirExpression,
        closure: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let predicate = self.lower_closure_function(closure, bivalens)?;
        self.lower_collection_transform(receiver, predicate, TransformKind::Filter, expr)
    }

    /// `omnia` — left-to-right universal predicate fold over a `lista`.
    ///
    /// The initial accumulator is true, so an empty source satisfies the
    /// vacuous-truth law. A false predicate result settles the fold early.
    pub(super) fn lower_omnia(
        &mut self,
        receiver: &HirExpression,
        closure: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let predicate = self.lower_closure_function(closure, bivalens)?;
        self.lower_collection_predicate(receiver, predicate, TransformKind::All, expr)
    }

    /// `quilibet` — left-to-right existential predicate fold over a `lista`.
    ///
    /// The initial accumulator is false, so an empty source fails the fold.
    /// A true predicate result settles the fold early.
    pub(super) fn lower_quilibet(
        &mut self,
        receiver: &HirExpression,
        closure: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let predicate = self.lower_closure_function(closure, bivalens)?;
        self.lower_collection_predicate(receiver, predicate, TransformKind::Any, expr)
    }

    /// `reperta` (en `find`) — left-to-right search over a `lista`.
    ///
    /// The result starts `nihil`; the first element whose predicate holds
    /// settles the fold early as `Some(element)`. Evaluation order and the
    /// early stop are exactly `quilibet`'s.
    pub(super) fn lower_reperta(
        &mut self,
        receiver: &HirExpression,
        closure: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let predicate = self.lower_closure_function(closure, bivalens)?;
        self.lower_collection_predicate(receiver, predicate, TransformKind::Find, expr)
    }

    pub fn lower_mappata(
        &mut self,
        receiver: &HirExpression,
        closure: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(receiver_ty) = receiver.ty else {
            self.errors
                .push(MirError::missing_type(receiver.span, "mappata receiver"));
            return None;
        };
        if self.collection_element_ty(receiver_ty).is_none() {
            self.errors.push(MirError::unsupported(
                receiver.span,
                "mappata receiver is not a supported collection type",
            ));
            return None;
        }
        let Some(result_ty) = expr.ty else {
            self.errors
                .push(MirError::missing_type(expr.span, "mappata result"));
            return None;
        };
        let Some(result_element_ty) = self.collection_element_ty(result_ty) else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "mappata result is not a supported collection type",
            ));
            return None;
        };
        let mapper = self.lower_closure_function(closure, result_element_ty)?;
        self.lower_collection_transform(receiver, mapper, TransformKind::Map, expr)
    }

    pub(super) fn lower_reducta(
        &mut self,
        receiver: &HirExpression,
        reducer: &HirExpression,
        initial: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(receiver_ty) = receiver.ty else {
            self.errors
                .push(MirError::missing_type(receiver.span, "collection reduce receiver"));
            return None;
        };
        if !matches!(
            self.normalized_type(receiver_ty),
            Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. }
        ) {
            self.errors.push(MirError::unsupported(
                receiver.span,
                "collection reduce before array MIR lowering",
            ));
            return None;
        }
        let result_ty = self.expr_ty(expr)?;
        let reducer = self.lower_reducer_function(reducer, result_ty)?;
        self.lower_collection_reduce(receiver, receiver_ty, reducer, initial, result_ty, expr)
    }

    /// `cumulata` — serial prefix scan over an array receiver.
    ///
    /// Same seam as `reducta` (two-parameter reducer closure, seeded
    /// accumulator), but one prefix result is appended to the result array
    /// after every fold step, mirroring the Rust codegen contract pinned by
    /// `lista_cumulata_emits_serial_prefix_scan`.
    pub(super) fn lower_cumulata(
        &mut self,
        receiver: &HirExpression,
        reducer: &HirExpression,
        initial: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(receiver_ty) = receiver.ty else {
            self.errors
                .push(MirError::missing_type(receiver.span, "collection cumulata receiver"));
            return None;
        };
        if !matches!(
            self.normalized_type(receiver_ty),
            Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. }
        ) {
            self.errors.push(MirError::unsupported(
                receiver.span,
                "collection cumulata before array MIR lowering",
            ));
            return None;
        }
        let result_ty = self.expr_ty(expr)?;
        let Some(accumulator_ty) = self.collection_element_ty(result_ty.semantic_id()) else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "collection cumulata result before array MIR lowering",
            ));
            return None;
        };
        let reducer = self.lower_reducer_function(reducer, accumulator_ty)?;
        self.lower_collection_cumulata(receiver, receiver_ty, reducer, initial, result_ty, accumulator_ty, expr)
    }

    /// `extende` (en `extend`) — append every element of `source` to
    /// `receiver`, in order, in place.
    ///
    /// The source length is read once before the loop, so `xs.extend(xs)`
    /// appends the original elements exactly once. Each step is the same
    /// `Append` a single `xs.append(v)` runs.
    pub(super) fn lower_extende(
        &mut self,
        receiver: &HirExpression,
        source: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let (Some(receiver_ty), Some(source_ty)) = (receiver.ty, source.ty) else {
            self.errors
                .push(MirError::missing_type(expr.span, "collection extend operands"));
            return None;
        };
        let is_array = |lowerer: &Self, ty| {
            matches!(
                lowerer.normalized_type(lowerer.types.peel_access_marker(ty).1),
                Type::Array(_)
                    | Type::BoundedArray { .. }
                    | Type::RangedArray { .. }
                    | Type::BoundedOcteti { .. }
                    | Type::Primitive(Primitive::Octeti)
            )
        };
        if !is_array(self, receiver_ty) || !is_array(self, source_ty) {
            self.errors
                .push(MirError::unsupported(expr.span, "collection extend before array MIR lowering"));
            return None;
        }
        let result_ty = self.expr_ty(expr)?;
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));

        let target = self.lower_expr_value(receiver)?;
        let mut source_value = self.lower_expr_value(source)?;
        let mut source_ty = self.types.peel_access_marker(source_ty).1;
        if matches!(self.normalized_type(source_ty), Type::BoundedOcteti { .. }) {
            // A bounded byte buffer carries no per-element MIR place, so it
            // widens to the receiver's `list<b8>` first, the copy `↦ list<b8>`
            // performs. The checker admits this source only beside a `b8`
            // receiver, so the receiver's own type is that target.
            let list_ty = self.types.peel_access_marker(receiver_ty).1;
            if !matches!(self.normalized_type(list_ty), Type::Array(_)) {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "collection extend of a bounded byte buffer into a non-list receiver",
                ));
                return None;
            }
            let list_mir_ty = MirType::semantic(list_ty);
            source_value = self.runtime_call_value(
                MirIntrinsic::Convert(MirConversion {
                    store: None,
                    flavor: MirConversionFlavor::Runtime,
                    source_ty: MirType::semantic(source_ty),
                    target_ty: list_mir_ty,
                    params: Vec::new(),
                    recovery: None,
                    struct_defaults: Vec::new(),
                }),
                vec![source_value],
                list_mir_ty,
                source.span,
            );
            source_ty = list_ty;
        }
        let source_place = self.materialize_operand_place(source_value, MirType::semantic(source_ty), source.span);

        let index = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: index, name: None, ty: numerus, mutable: true, span: expr.span });
        self.assign(
            MirPlace::local(index),
            MirOperand::Constant(MirConstant::Int(0)),
            numerus,
            expr.span,
        );
        let length = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Length),
            vec![MirOperand::Place(source_place.clone())],
            numerus,
            expr.span,
        );
        if !self.current_is_open() {
            return None;
        }

        let cond_id = self.fresh_block(expr.span);
        let body_id = self.fresh_block(expr.span);
        let after_id = self.fresh_block(expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(cond_id);
        let continues = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(MirPlace::local(index)),
            length,
            bivalens,
            expr.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: continues, then_block: body_id, else_block: after_id },
            expr.span,
        );

        self.switch_to(body_id);
        let mut item_place = source_place;
        item_place
            .projections
            .push(MirProjection::Index(MirOperand::Place(MirPlace::local(index))));
        self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Append),
            vec![target, MirOperand::Place(item_place)],
            result_ty,
            expr.span,
        );
        if !self.current_is_open() {
            return None;
        }
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Constant(MirConstant::Int(1)),
            numerus,
            expr.span,
        );
        self.assign(MirPlace::local(index), next, numerus, expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(after_id);
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    fn lower_collection_predicate(
        &mut self,
        receiver: &HirExpression,
        callback: MirCallee,
        kind: TransformKind,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(receiver_ty) = receiver.ty else {
            self.errors
                .push(MirError::missing_type(receiver.span, "collection predicate receiver"));
            return None;
        };
        if !matches!(
            self.normalized_type(receiver_ty),
            Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. }
        ) {
            self.errors.push(MirError::unsupported(
                receiver.span,
                "collection predicate before array MIR lowering",
            ));
            return None;
        }
        let item_is_optional = match self.normalized_type(receiver_ty) {
            Type::Array(element) | Type::BoundedArray { element, .. } | Type::RangedArray { element, .. } => {
                matches!(self.normalized_type(*element), Type::Option(_))
            }
            _ => false,
        };
        let result_ty = self.expr_ty(expr)?;
        let collection_ty = MirType::semantic(receiver_ty);
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));

        let collection = self.lower_expr_value(receiver)?;
        let collection = self.materialize_operand_place(collection, collection_ty, receiver.span);

        let result = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: result, name: None, ty: result_ty, mutable: true, span: expr.span });
        let initial = if matches!(kind, TransformKind::Find) {
            self.assign_temp(MirValueKind::Option(MirOptionOp::None), result_ty, expr.span)
        } else {
            MirOperand::Constant(MirConstant::Bool(matches!(kind, TransformKind::All)))
        };
        self.assign(MirPlace::local(result), initial, result_ty, expr.span);

        let index = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: index, name: None, ty: numerus, mutable: true, span: expr.span });
        self.assign(
            MirPlace::local(index),
            MirOperand::Constant(MirConstant::Int(0)),
            numerus,
            expr.span,
        );

        let length = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Length),
            vec![MirOperand::Place(collection.clone())],
            numerus,
            expr.span,
        );
        if !self.current_is_open() {
            return None;
        }

        let cond_id = self.fresh_block(expr.span);
        let body_id = self.fresh_block(expr.span);
        let decisive_id = self.fresh_block(expr.span);
        let increment_id = self.fresh_block(expr.span);
        let after_id = self.fresh_block(expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(cond_id);
        let continues = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(MirPlace::local(index)),
            length,
            bivalens,
            expr.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: continues, then_block: body_id, else_block: after_id },
            expr.span,
        );

        self.switch_to(body_id);
        let mut item_place = collection;
        item_place
            .projections
            .push(MirProjection::Index(MirOperand::Place(MirPlace::local(index))));
        let item = MirOperand::Place(item_place);
        let predicate = self.call_callback_callee(callback, vec![item.clone()], bivalens, expr.span);
        if !self.current_is_open() {
            return None;
        }

        let (then_block, else_block) = match kind {
            TransformKind::All => (increment_id, decisive_id),
            TransformKind::Any | TransformKind::Find => (decisive_id, increment_id),
            TransformKind::Filter | TransformKind::Map => {
                return None;
            }
        };
        self.terminate_current(
            MirTerminatorKind::Branch { condition: predicate, then_block, else_block },
            expr.span,
        );

        self.switch_to(decisive_id);
        let decisive_value = if matches!(kind, TransformKind::Find) {
            if item_is_optional {
                item
            } else {
                self.assign_temp(MirValueKind::Option(MirOptionOp::Some(item)), result_ty, expr.span)
            }
        } else {
            MirOperand::Constant(MirConstant::Bool(matches!(kind, TransformKind::Any)))
        };
        self.assign(MirPlace::local(result), decisive_value, result_ty, expr.span);
        self.terminate_current(MirTerminatorKind::Goto(after_id), expr.span);

        self.switch_to(increment_id);
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Constant(MirConstant::Int(1)),
            numerus,
            expr.span,
        );
        self.assign(MirPlace::local(index), next, numerus, expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(after_id);
        Some(MirOperand::Place(MirPlace::local(result)))
    }

    fn lower_collection_transform(
        &mut self,
        receiver: &HirExpression,
        callback: MirCallee,
        kind: TransformKind,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(receiver_ty) = receiver.ty else {
            self.errors
                .push(MirError::missing_type(receiver.span, "collection transform receiver"));
            return None;
        };
        let set_element = match self.normalized_type(receiver_ty) {
            Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. } => None,
            Type::Set(item) => Some(*item),
            _ => {
                self.errors.push(MirError::unsupported(
                    receiver.span,
                    "collection transform before array MIR lowering",
                ));
                return None;
            }
        };
        let collection_ty = MirType::semantic(receiver_ty);
        let result_ty = self.expr_ty(expr)?;
        let collection = self.lower_expr_value(receiver)?;
        let collection = self.materialize_operand_place(collection, collection_ty, receiver.span);
        // A `copia` is unordered (D19.2): snapshot it into a `lista<T>`, the same
        // `copia ↦ lista<T>` conversion `for from` lowers to, so the loop reads
        // the array lane through one place. The rebuilt result is still a set,
        // so element order cannot leak into the result.
        let collection = match set_element {
            Some(element) => {
                let Some(array) = self.types.find_array(element) else {
                    self.errors
                        .push(MirError::missing_type(receiver.span, "copia transform snapshot list"));
                    return None;
                };
                let array_ty = MirType::semantic(array);
                let snapshot = self.runtime_call_value(
                    MirIntrinsic::Convert(MirConversion {
                        store: None,
                        flavor: MirConversionFlavor::Runtime,
                        source_ty: collection_ty,
                        target_ty: array_ty,
                        params: Vec::new(),
                        recovery: None,
                        struct_defaults: Vec::new(),
                    }),
                    vec![MirOperand::Place(collection)],
                    array_ty,
                    receiver.span,
                );
                if !self.current_is_open() {
                    return None;
                }
                self.materialize_operand_place(snapshot, array_ty, receiver.span)
            }
            None => collection,
        };
        self.lower_transform_over(collection, callback, kind, result_ty, expr.span)
    }

    /// Loop `callback` over an already-materialized array place, building a
    /// `result_ty` array (filter keeps items, map appends callback results).
    fn lower_transform_over(
        &mut self,
        collection: MirPlace,
        callback: MirCallee,
        kind: TransformKind,
        result_ty: MirType,
        span: Span,
    ) -> Option<MirOperand> {
        let (result_element_ty, result_aggregate) = match self.normalized_type(result_ty.semantic_id()) {
            Type::Array(item) => (MirType::semantic(*item), MirAggregateKind::Array),
            Type::Set(item) => (MirType::semantic(*item), MirAggregateKind::Set),
            _ => {
                self.errors.push(MirError::unsupported(
                    span,
                    "collection transform result before array MIR lowering",
                ));
                return None;
            }
        };
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let vacuum = MirType::semantic(self.types.primitive(Primitive::Vacuum));

        let result = self.construct_temp(result_aggregate, MirAggregateFields::Ordered(Vec::new()), result_ty, span);
        let result = self.materialize_operand_place(result, result_ty, span);

        let index = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: index, name: None, ty: numerus, mutable: true, span });
        self.assign(MirPlace::local(index), MirOperand::Constant(MirConstant::Int(0)), numerus, span);

        let length = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Length),
            vec![MirOperand::Place(collection.clone())],
            numerus,
            span,
        );
        if !self.current_is_open() {
            return None;
        }

        let cond_id = self.fresh_block(span);
        let body_id = self.fresh_block(span);
        let append_id = self.fresh_block(span);
        let increment_id = self.fresh_block(span);
        let after_id = self.fresh_block(span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), span);

        self.switch_to(cond_id);
        let continues =
            self.binary_temp(MirBinOp::Lt, MirOperand::Place(MirPlace::local(index)), length, bivalens, span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: continues, then_block: body_id, else_block: after_id },
            span,
        );

        self.switch_to(body_id);
        let mut item_place = collection.clone();
        item_place
            .projections
            .push(MirProjection::Index(MirOperand::Place(MirPlace::local(index))));
        let item = MirOperand::Place(item_place);
        let callback_return = match kind {
            TransformKind::Filter => bivalens,
            TransformKind::Map => result_element_ty,
            TransformKind::All | TransformKind::Any | TransformKind::Find => {
                self.errors
                    .push(MirError::unsupported(span, "predicate fold lowered as a collection transform"));
                return None;
            }
        };
        let mapped = self.call_callback_callee(callback, vec![item.clone()], callback_return, span);
        if !self.current_is_open() {
            return None;
        }

        match kind {
            TransformKind::Filter => {
                self.terminate_current(
                    MirTerminatorKind::Branch { condition: mapped, then_block: append_id, else_block: increment_id },
                    span,
                );
                self.switch_to(append_id);
                self.runtime_call_value(
                    MirIntrinsic::Collection(MirCollectionOp::Append),
                    vec![MirOperand::Place(result.clone()), item],
                    vacuum,
                    span,
                );
            }
            TransformKind::Map => {
                self.terminate_current(MirTerminatorKind::Goto(append_id), span);
                self.switch_to(append_id);
                self.runtime_call_value(
                    MirIntrinsic::Collection(MirCollectionOp::Append),
                    vec![MirOperand::Place(result.clone()), mapped],
                    vacuum,
                    span,
                );
            }
            TransformKind::All | TransformKind::Any | TransformKind::Find => {
                self.errors
                    .push(MirError::unsupported(span, "predicate fold lowered as a collection transform"));
                return None;
            }
        }
        if !self.current_is_open() {
            return None;
        }
        self.terminate_open_current(MirTerminatorKind::Goto(increment_id), span);

        self.switch_to(increment_id);
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Constant(MirConstant::Int(1)),
            numerus,
            span,
        );
        self.assign(MirPlace::local(index), next, numerus, span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), span);

        self.switch_to(after_id);
        Some(MirOperand::Place(result))
    }

    /// Closure `regex.muta(text, closure)` (RX-5): the closure runs once per
    /// match, left to right, over `RegexFindAll`'s matches with the ordinary
    /// map loop; `RegexReplaceEach` then joins the results into the text. The
    /// results are taken literally and never re-scanned. Same shape as the
    /// keyed sort: map, then one collection op over the mapped `lista`.
    pub(super) fn lower_regex_replace_closure(
        &mut self,
        receiver: &HirExpression,
        text: &HirExpression,
        closure: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let found = self.types.primitive(Primitive::Match);
        let textus = self.types.primitive(Primitive::Textus);
        // WHY: typecheck interns both carriers when it admits the closure form,
        // so they exist in the read-only type table here.
        let (Some(matches_ty), Some(texts_ty)) = (self.types.find_array(found), self.types.find_array(textus)) else {
            self.errors
                .push(MirError::missing_type(expr.span, "regex replace closure carriers"));
            return None;
        };
        let matches_ty = MirType::semantic(matches_ty);
        let texts_ty = MirType::semantic(texts_ty);
        let result_ty = self.expr_ty(expr)?;
        let regex = self.lower_expr_value(receiver)?;
        let text = self.lower_expr_value(text)?;
        let callback = self.lower_closure_function(closure, MirType::semantic(textus))?;
        let found_all = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::RegexFindAll),
            vec![regex.clone(), text.clone()],
            matches_ty,
            expr.span,
        );
        let found_all = self.materialize_operand_place(found_all, matches_ty, expr.span);
        let replacements = self.lower_transform_over(found_all, callback, TransformKind::Map, texts_ty, expr.span)?;
        Some(self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::RegexReplaceEach),
            vec![regex, text, replacements],
            result_ty,
            expr.span,
        ))
    }

    /// `ordina(key)` (D1.7): map the receiver to a key `lista` with the
    /// ordinary map loop, then `SortByKeys` reorders the receiver in place
    /// stably by those keys. The receiver is evaluated once. The keyed
    /// `ordinata(key)` copy form is retired; the sort
    /// expression builds new lists.
    pub(super) fn lower_sort_by_key(
        &mut self,
        receiver: &HirExpression,
        closure: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        // WHY: a local bound by `← copy` (or `own`/`ref`/`mut`) carries its
        // access marker on the type; the carrier is the payload under it.
        let receiver_ty = self.method_receiver_payload_id(receiver.ty?);
        if !matches!(
            self.normalized_type(receiver_ty),
            Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. }
        ) {
            self.errors.push(MirError::unsupported(
                receiver.span,
                "ordina key receiver before array MIR lowering",
            ));
            return None;
        }
        let key_ty = if let Some(Type::Func(sig)) = closure.ty.map(|ty| self.normalized_type(ty)) {
            sig.ret
        } else {
            self.errors
                .push(MirError::missing_type(closure.span, "ordina key selector"));
            return None;
        };
        // WHY: typecheck interns `lista<key>` when it admits the selector, so
        // the key carrier exists in the read-only type table here.
        let Some(key_lista_ty) = self.types.find_array(key_ty) else {
            self.errors
                .push(MirError::missing_type(closure.span, "ordina key lista"));
            return None;
        };
        let key_lista_ty = MirType::semantic(key_lista_ty);
        let key_fn = self.lower_closure_function(closure, MirType::semantic(key_ty))?;
        let collection_ty = MirType::semantic(receiver_ty);
        let (place, ty) = self.lower_assignment_place_with_fallback(receiver, collection_ty)?;
        let keys = self.lower_transform_over(place.clone(), key_fn, TransformKind::Map, key_lista_ty, expr.span)?;
        let sorted = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::SortByKeys),
            vec![MirOperand::Place(place.clone()), keys],
            ty,
            expr.span,
        );
        self.assign(place, sorted, ty, expr.span);
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    /// Lower the sort expression `sort via asc|desc from|ref source [body]`
    ///.
    ///
    /// Always builds a new list. The source terms are a list place (a set is
    /// converted, a map yields `Values` or `Keys`). A descending sort reverses
    /// the terms, sorts ascending (stable), and reverses the result, so equal
    /// keys keep source order in both directions. With a body the key list is
    /// the body mapped over the terms; `SortByKeys` then reorders the terms.
    pub(super) fn lower_sort_expr(
        &mut self,
        direction: radix_hir::HirSortDirection,
        source_mode: radix_hir::HirSortSource,
        source: &HirExpression,
        binding: Option<&(DefId, Symbol)>,
        body: Option<&HirBlock>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(source_ty) = source.ty else {
            self.errors
                .push(MirError::missing_type(source.span, "sort expression source"));
            return None;
        };
        let list_ty = self.expr_ty(expr)?;
        let span = expr.span;
        let mut payload = source_ty;
        while let Type::Ref(_, inner) = self.normalized_type(payload) {
            payload = *inner;
        }
        let (is_set, is_map) = match self.normalized_type(payload) {
            Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. } => (false, false),
            Type::Set(_) => (true, false),
            Type::Map(_, _) | Type::Ledger(_, _) => (false, true),
            _ => {
                self.errors.push(MirError::unsupported(
                    source.span,
                    "sort expression source before collection MIR lowering",
                ));
                return None;
            }
        };
        let source_value = self.lower_expr_value(source)?;
        let source_mir_ty = MirType::semantic(payload);
        let terms = if is_set {
            let converted = self.runtime_call_value(
                MirIntrinsic::Convert(MirConversion {
                    store: None,
                    flavor: MirConversionFlavor::Runtime,
                    source_ty: source_mir_ty,
                    target_ty: list_ty,
                    params: Vec::new(),
                    recovery: None,
                    struct_defaults: Vec::new(),
                }),
                vec![source_value],
                list_ty,
                span,
            );
            self.materialize_operand_place(converted, list_ty, span)
        } else if is_map {
            let map = self.materialize_operand_place(source_value, source_mir_ty, span);
            let op = match source_mode {
                radix_hir::HirSortSource::Values => MirCollectionOp::Values,
                radix_hir::HirSortSource::Keys => MirCollectionOp::Keys,
            };
            let snapshot =
                self.runtime_call_value(MirIntrinsic::Collection(op), vec![MirOperand::Place(map)], list_ty, span);
            self.materialize_operand_place(snapshot, list_ty, span)
        } else {
            self.materialize_operand_place(source_value, source_mir_ty, span)
        };
        if !self.current_is_open() {
            return None;
        }
        let descending = matches!(direction, radix_hir::HirSortDirection::Descending);
        let terms = if descending {
            let reversed = self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::Reverse),
                vec![MirOperand::Place(terms)],
                list_ty,
                span,
            );
            self.materialize_operand_place(reversed, list_ty, span)
        } else {
            terms
        };
        let sorted = match (binding, body) {
            (Some(binding), Some(body)) => {
                struct KeyType(Option<TypeId>);
                impl HirVisitor for KeyType {
                    fn visit_stmt(&mut self, stmt: &HirStatement) {
                        if let HirStatementKind::Redde(Some(value)) = &stmt.kind {
                            self.0 = value.ty;
                        }
                        radix_hir::visit::walk_stmt(self, stmt);
                    }
                }
                let mut key = KeyType(None);
                key.visit_block(body);
                let Some(key_ty) = key.0 else {
                    self.errors
                        .push(MirError::missing_type(body.span, "sort expression key"));
                    return None;
                };
                let Type::Array(element) = self.normalized_type(list_ty.semantic_id()) else {
                    self.errors
                        .push(MirError::unsupported(span, "sort expression result before list MIR lowering"));
                    return None;
                };
                let element = *element;
                let Some(key_lista_ty) = self.types.find_array(key_ty) else {
                    self.errors
                        .push(MirError::missing_type(span, "sort expression key lista"));
                    return None;
                };
                let key_lista_ty = MirType::semantic(key_lista_ty);
                let selector = radix_types::FuncSig {
                    type_params: Vec::new(),
                    params: vec![radix_types::ParamType {
                        ty: element,
                        mode: radix_types::SemanticParamMode::Owned,
                        optional: false,
                        sponte: false,
                    }],
                    ret: key_ty,
                    err: None,
                    is_async: false,
                    is_generator: false,
                };
                let closure = HirExpression {
                    id: expr.id,
                    kind: HirExpressionKind::Clausura(
                        vec![radix_hir::HirParam {
                            def_id: binding.0,
                            name: binding.1,
                            ty: element,
                            mode: HirParamMode::Owned,
                            optional: false,
                            sponte: false,
                            default: None,
                            span: body.span,
                        }],
                        Some(key_ty),
                        None,
                        Box::new(HirExpression {
                            id: expr.id,
                            kind: HirExpressionKind::Block(body.clone()),
                            ty: Some(key_ty),
                            span: body.span,
                        }),
                    ),
                    ty: self.types.find_equal(&Type::Func(selector)),
                    span: body.span,
                };
                let key_fn = self.lower_closure_function(&closure, MirType::semantic(key_ty))?;
                let keys = self.lower_transform_over(terms.clone(), key_fn, TransformKind::Map, key_lista_ty, span)?;
                self.runtime_call_value(
                    MirIntrinsic::Collection(MirCollectionOp::SortByKeys),
                    vec![MirOperand::Place(terms), keys],
                    list_ty,
                    span,
                )
            }
            _ => self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::Sort),
                vec![MirOperand::Place(terms)],
                list_ty,
                span,
            ),
        };
        if !self.current_is_open() {
            return None;
        }
        if !descending {
            return Some(sorted);
        }
        let sorted = self.materialize_operand_place(sorted, list_ty, span);
        Some(self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Reverse),
            vec![MirOperand::Place(sorted)],
            list_ty,
            span,
        ))
    }

    fn lower_collection_reduce(
        &mut self,
        receiver: &HirExpression,
        receiver_ty: TypeId,
        reducer: MirCallee,
        initial: &HirExpression,
        result_ty: MirType,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let collection_ty = MirType::semantic(receiver_ty);
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));

        let collection = self.lower_expr_value(receiver)?;
        let collection = self.materialize_operand_place(collection, collection_ty, receiver.span);
        // WHY: the fold rewrites the accumulator every step, so it must be a
        // fresh cell. `materialize_operand_place` would return the initial's
        // own place (a caller local or field) and the fold would overwrite it.
        let accumulator = self.lower_expr_value(initial)?;
        let accumulator_temp = self.push_temp(result_ty, initial.span);
        self.assign(MirPlace::temp(accumulator_temp), accumulator, result_ty, initial.span);
        let accumulator = MirPlace::temp(accumulator_temp);

        let index = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: index, name: None, ty: numerus, mutable: true, span: expr.span });
        self.assign(
            MirPlace::local(index),
            MirOperand::Constant(MirConstant::Int(0)),
            numerus,
            expr.span,
        );

        let length = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Length),
            vec![MirOperand::Place(collection.clone())],
            numerus,
            expr.span,
        );
        if !self.current_is_open() {
            return None;
        }

        let cond_id = self.fresh_block(expr.span);
        let body_id = self.fresh_block(expr.span);
        let increment_id = self.fresh_block(expr.span);
        let after_id = self.fresh_block(expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(cond_id);
        let continues = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(MirPlace::local(index)),
            length,
            bivalens,
            expr.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: continues, then_block: body_id, else_block: after_id },
            expr.span,
        );

        self.switch_to(body_id);
        let mut item_place = collection.clone();
        item_place
            .projections
            .push(MirProjection::Index(MirOperand::Place(MirPlace::local(index))));
        let next_accumulator = self.call_callback_callee(
            reducer,
            vec![MirOperand::Place(accumulator.clone()), MirOperand::Place(item_place)],
            result_ty,
            expr.span,
        );
        self.assign(accumulator.clone(), next_accumulator, result_ty, expr.span);
        if !self.current_is_open() {
            return None;
        }
        self.terminate_current(MirTerminatorKind::Goto(increment_id), expr.span);

        self.switch_to(increment_id);
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Constant(MirConstant::Int(1)),
            numerus,
            expr.span,
        );
        self.assign(MirPlace::local(index), next, numerus, expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(after_id);
        Some(MirOperand::Place(accumulator))
    }

    fn lower_collection_cumulata(
        &mut self,
        receiver: &HirExpression,
        receiver_ty: TypeId,
        reducer: MirCallee,
        initial: &HirExpression,
        result_ty: MirType,
        accumulator_ty: MirType,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let collection_ty = MirType::semantic(receiver_ty);
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let vacuum = MirType::semantic(self.types.primitive(Primitive::Vacuum));

        let collection = self.lower_expr_value(receiver)?;
        let collection = self.materialize_operand_place(collection, collection_ty, receiver.span);
        // WHY: each scan step rewrites the accumulator, so it must be a
        // fresh cell. Materializing the initial value could return a caller
        // local or field and let the scan overwrite it.
        let accumulator = self.lower_expr_value(initial)?;
        let accumulator_temp = self.push_temp(accumulator_ty, initial.span);
        self.assign(MirPlace::temp(accumulator_temp), accumulator, accumulator_ty, initial.span);
        let accumulator = MirPlace::temp(accumulator_temp);
        let result = self.construct_temp(
            MirAggregateKind::Array,
            MirAggregateFields::Ordered(Vec::new()),
            result_ty,
            expr.span,
        );
        let result = self.materialize_operand_place(result, result_ty, expr.span);

        let index = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: index, name: None, ty: numerus, mutable: true, span: expr.span });
        self.assign(
            MirPlace::local(index),
            MirOperand::Constant(MirConstant::Int(0)),
            numerus,
            expr.span,
        );

        let length = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Length),
            vec![MirOperand::Place(collection.clone())],
            numerus,
            expr.span,
        );
        if !self.current_is_open() {
            return None;
        }

        let cond_id = self.fresh_block(expr.span);
        let body_id = self.fresh_block(expr.span);
        let append_id = self.fresh_block(expr.span);
        let increment_id = self.fresh_block(expr.span);
        let after_id = self.fresh_block(expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(cond_id);
        let continues = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(MirPlace::local(index)),
            length,
            bivalens,
            expr.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: continues, then_block: body_id, else_block: after_id },
            expr.span,
        );

        self.switch_to(body_id);
        let mut item_place = collection.clone();
        item_place
            .projections
            .push(MirProjection::Index(MirOperand::Place(MirPlace::local(index))));
        let next_accumulator = self.call_callback_callee(
            reducer,
            vec![MirOperand::Place(accumulator.clone()), MirOperand::Place(item_place)],
            accumulator_ty,
            expr.span,
        );
        self.assign(accumulator.clone(), next_accumulator, accumulator_ty, expr.span);
        if !self.current_is_open() {
            return None;
        }
        self.terminate_current(MirTerminatorKind::Goto(append_id), expr.span);

        self.switch_to(append_id);
        self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Append),
            vec![MirOperand::Place(result.clone()), MirOperand::Place(accumulator)],
            vacuum,
            expr.span,
        );
        if !self.current_is_open() {
            return None;
        }
        self.terminate_current(MirTerminatorKind::Goto(increment_id), expr.span);

        self.switch_to(increment_id);
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Constant(MirConstant::Int(1)),
            numerus,
            expr.span,
        );
        self.assign(MirPlace::local(index), next, numerus, expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(after_id);
        Some(MirOperand::Place(result))
    }

    pub(super) fn collection_element_ty(&self, ty: TypeId) -> Option<MirType> {
        match self.normalized_type(ty) {
            Type::Array(item)
            | Type::BoundedArray { element: item, .. }
            | Type::RangedArray { element: item, .. }
            | Type::Set(item) => Some(MirType::semantic(*item)),
            _ => None,
        }
    }

    pub fn take_synthetic_functions(&mut self) -> Vec<MirFunction> {
        std::mem::take(&mut self.synthetic_functions)
    }

    pub(super) fn take_closure_environments(&mut self) -> Vec<MirClosureEnvironment> {
        std::mem::take(&mut self.closure_environments)
    }
}

#[derive(Debug, Clone, Copy)]
enum TransformKind {
    All,
    Any,
    Find,
    Filter,
    Map,
}
