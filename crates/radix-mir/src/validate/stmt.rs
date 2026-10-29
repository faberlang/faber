//! Block, statement, terminator, and CFG structural checks.

use super::{
    DuplicateFunctionFinder, FunctionScope, FunctionShape, GenericTypeBindings, Validator,
};
use crate::generic::MirShapeBindings;
use crate::visit::terminator_successors;
use crate::{
    MirBlock, MirBlockId, MirCallee, MirFunction, MirFunctionId, MirIntrinsic, MirOperand,
    MirPlaceBase, MirStatement, MirStatementKind, MirTerminator, MirTerminatorKind,
};
use handroll::{FxHashMap, FxHashSet};
use radix_types::{IndexExpr, Primitive, Type};

impl Validator<'_, '_> {
    pub(crate) fn validate_program(&mut self) {
        for (id, span) in DuplicateFunctionFinder::find(self.program) {
            self.error(span, format!("duplicate MIR function id f{}", id.0));
        }
        // Build the function index once; every FunctionScope borrows it (PRP-053).
        // Prior path cloned the full map per function (O(N²) hash-map copies).
        let functions_by_id = self
            .program
            .functions
            .iter()
            .map(|function| (function.id, function))
            .collect::<FxHashMap<_, _>>();

        for function in &self.program.functions {
            self.validate_function(function, &functions_by_id);
        }
    }

    pub(crate) fn validate_function(
        &mut self,
        function: &MirFunction,
        functions_by_id: &FxHashMap<MirFunctionId, &MirFunction>,
    ) {
        // First prove declaration shape, then validate executable block bodies
        // against maps built from that shape. This keeps later diagnostics about
        // bad uses from being polluted by partially collected declarations.
        self.validate_mir_type(function.return_ty, function.span);
        if let Some(error_ty) = function.error_ty {
            self.validate_mir_type(error_ty, function.span);
        }
        for param in &function.params {
            self.validate_mir_type(param.ty, param.span);
        }
        for local in &function.locals {
            self.validate_mir_type(local.ty, local.span);
        }
        for temp in &function.temps {
            self.validate_mir_type(temp.ty, temp.span);
        }
        let shape = FunctionShape::collect(function);
        self.report_duplicate_local_ids(&shape.duplicate_param_locals);
        self.report_duplicate_local_ids(&shape.duplicate_locals);
        for (id, span) in &shape.conflicting_locals {
            self.error(
                *span,
                format!("duplicate MIR local id _{} has conflicting type", id.0),
            );
        }
        for (id, span) in &shape.duplicate_temps {
            self.error(*span, format!("duplicate MIR temp id %{}", id.0));
        }
        for (id, span) in &shape.duplicate_blocks {
            self.error(*span, format!("duplicate MIR block id bb{}", id.0));
        }
        let mut scope = FunctionScope {
            function,
            functions_by_id,
            locals: shape.locals,
            temps: shape.temps,
            blocks: shape.blocks,
            values: FxHashMap::default(),
        };

        for block in &function.blocks {
            self.validate_block(&mut scope, block);
        }

        self.validate_cfg(function, &scope);
    }

    pub(crate) fn validate_block(&mut self, scope: &mut FunctionScope<'_, '_>, block: &MirBlock) {
        scope.values.clear();
        let mut previous = None;
        for stmt in &block.statements {
            self.validate_stmt(scope, stmt, previous);
            previous = Some(stmt);
        }
        self.validate_terminator(scope, &block.terminator, previous);
    }

    pub(crate) fn validate_stmt(
        &mut self,
        scope: &mut FunctionScope<'_, '_>,
        stmt: &MirStatement,
        previous: Option<&MirStatement>,
    ) {
        match &stmt.kind {
            MirStatementKind::Assign { place, value } => {
                let place_ty = self.validate_place(scope, place, stmt.span);
                self.validate_value(scope, value);
                if let Some(place_ty) = place_ty {
                    let recorded_store_relation = previous.is_some_and(|statement| {
                        let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                            return false;
                        };
                        matches!(&call.intrinsic, crate::MirIntrinsic::TensorShapeCheck(check)
                            if !check.relations.is_empty() && call.args.contains(&crate::MirOperand::Value(value.id)))
                    });
                    if !(self.assignable(value.ty, place_ty)
                        || (recorded_store_relation
                            && self.return_shape_relation_assignable(value.ty, place_ty)))
                    {
                        self.error(value.span, "assigned value type does not match destination");
                    }
                }
                scope.values.insert(value.id, value.ty);
            }
            MirStatementKind::Call {
                destination,
                callee,
                args,
            } => {
                let sig = self.validate_callee(scope, callee, stmt.span, false);
                if let Some(sig) = &sig
                    && sig.error_ty.is_some()
                {
                    self.error(stmt.span, "ordinary call callee is failable; use try_call");
                }
                let mut generic_bindings = GenericTypeBindings::default();
                let mut shape_bindings = MirShapeBindings::default();
                self.validate_call_args(
                    scope,
                    sig.as_ref(),
                    args,
                    preceding_tensor_shape_check(previous, args),
                    stmt.span,
                    &mut generic_bindings,
                    &mut shape_bindings,
                );
                if !self.call_destination_matches_shape_check(
                    scope,
                    destination.as_ref(),
                    sig.as_ref().map(|sig| sig.return_ty),
                    args,
                    preceding_tensor_shape_check(previous, args),
                ) {
                    self.validate_optional_destination_with_shapes(
                        scope,
                        destination.as_ref(),
                        sig.as_ref().map(|sig| sig.return_ty),
                        stmt.span,
                        &mut generic_bindings,
                        &mut shape_bindings,
                    );
                }
            }
            MirStatementKind::RuntimeCall { destination, call } => {
                self.validate_runtime_call(scope, destination.as_ref(), call, stmt.span);
            }
            MirStatementKind::Construct {
                destination,
                aggregate,
            } => {
                let destination_ty = self.validate_place(scope, destination, stmt.span);
                self.validate_aggregate(scope, aggregate, stmt.span);
                if let Some(destination_ty) = destination_ty {
                    self.require_assignable(
                        aggregate.ty,
                        destination_ty,
                        stmt.span,
                        "construct destination type does not match aggregate type",
                    );
                }
            }
        }
    }

    pub(crate) fn validate_terminator(
        &mut self,
        scope: &mut FunctionScope<'_, '_>,
        terminator: &MirTerminator,
        previous: Option<&MirStatement>,
    ) {
        for target in terminator_successors(&terminator.kind) {
            self.require_block(scope, target, terminator.span);
        }

        match &terminator.kind {
            MirTerminatorKind::Return(None) => {
                if !self.is_unit_return(scope.function.return_ty)
                    && !scope.function.is_generator
                    && !scope.function.is_async
                {
                    self.error(
                        terminator.span,
                        "no-value return in value-returning function",
                    );
                }
            }
            MirTerminatorKind::Return(Some(value)) => {
                if let Some(value_ty) = self.validate_operand(scope, value, terminator.span)
                    && !self.assignable(value_ty, scope.function.return_ty)
                    && !self.return_shape_relation_assignable(value_ty, scope.function.return_ty)
                {
                    self.error(terminator.span, "return type mismatch");
                }
            }
            MirTerminatorKind::ReturnError(value) => {
                let Some(error_ty) = scope.function.error_ty else {
                    self.error(
                        terminator.span,
                        "return_error in function without alternate-exit type",
                    );
                    self.validate_operand(scope, value, terminator.span);
                    return;
                };
                if let Some(value_ty) = self.validate_operand(scope, value, terminator.span) {
                    self.require_assignable(
                        value_ty,
                        error_ty,
                        terminator.span,
                        "return_error type mismatch",
                    );
                }
            }
            MirTerminatorKind::TryCall { .. } => {
                self.validate_try_call(scope, previous, terminator);
            }
            MirTerminatorKind::ReturnThread => {
                self.error(terminator.span, "return_thread lowering pending");
            }
            MirTerminatorKind::Goto(_) | MirTerminatorKind::Unreachable => {}
            MirTerminatorKind::Branch { condition, .. } => {
                if let Some(condition_ty) = self.validate_operand(scope, condition, terminator.span)
                {
                    self.require_exact(
                        condition_ty,
                        self.primitive(Primitive::Bivalens),
                        terminator.span,
                        "branch condition is not bivalens",
                    );
                }
            }
            MirTerminatorKind::Switch { value, cases, .. } => {
                self.validate_operand(scope, value, terminator.span);
                if cases.is_empty() {
                    self.error(terminator.span, "switch has no cases");
                }
                for (i, case) in cases.iter().enumerate() {
                    if cases[..i].iter().any(|prev| prev.value == case.value) {
                        self.error(terminator.span, "duplicate switch case value");
                    }
                }
            }
        }
    }

    fn validate_try_call(
        &mut self,
        scope: &mut FunctionScope<'_, '_>,
        previous: Option<&MirStatement>,
        terminator: &MirTerminator,
    ) {
        let MirTerminatorKind::TryCall {
            destination,
            callee,
            args,
            error_place,
            ..
        } = &terminator.kind
        else {
            return;
        };
        let cursor_settle = self.is_cursor_settle_callee(scope, callee, args);
        let sig = self.validate_callee(scope, callee, terminator.span, cursor_settle);
        let mut generic_bindings = GenericTypeBindings::default();
        let mut shape_bindings = MirShapeBindings::default();
        self.validate_call_args(
            scope,
            sig.as_ref(),
            args,
            preceding_tensor_shape_check(previous, args),
            terminator.span,
            &mut generic_bindings,
            &mut shape_bindings,
        );
        if !self.call_destination_matches_shape_check(
            scope,
            destination.as_ref(),
            sig.as_ref().map(|sig| sig.return_ty),
            args,
            preceding_tensor_shape_check(previous, args),
        ) {
            self.validate_optional_destination_with_shapes(
                scope,
                destination.as_ref(),
                sig.as_ref().map(|sig| sig.return_ty),
                terminator.span,
                &mut generic_bindings,
                &mut shape_bindings,
            );
        }
        let error_place_ty = self.validate_place(scope, error_place, terminator.span);
        // The failable contract is enforced whenever the callee
        // signature is resolvable HERE. Package-MIR synthetic callees
        // (`MirCallee::Definition { source }` allocated by the faber
        // linker after analysis) resolve only in the merged
        // whole-program validation, where the linked library function
        // carries the synthetic source; deferring their check to that
        // stage matches the plain-`Call` treatment of cross-unit
        // definitions. A never-resolvable def is caught downstream the
        // same way a plain `Call` to one is: by the runner/emitter.
        if sig.is_none() && !cursor_settle && !matches!(callee, MirCallee::Definition { .. }) {
            self.error(
                terminator.span,
                "try_call callee does not have a known failable signature",
            );
        }
        // The cursor-settle shape has no signature to compare against; its
        // error place only needs to be a valid place (checked above).
        if cursor_settle {
            return;
        }
        match (sig.as_ref().and_then(|sig| sig.error_ty), error_place_ty) {
            (Some(error_ty), Some(place_ty)) => {
                self.bind_generic_type_witness(
                    error_ty,
                    place_ty,
                    &mut generic_bindings,
                    &mut shape_bindings,
                    terminator.span,
                    "try_call error place type mismatch",
                );
                if let Some(error_ty) = self.instantiate_generic_type(
                    error_ty,
                    &generic_bindings,
                    &shape_bindings,
                    terminator.span,
                ) {
                    self.require_assignable(
                        error_ty,
                        place_ty,
                        terminator.span,
                        "try_call error place type mismatch",
                    );
                }
            }
            (None, _) if sig.is_some() => {
                self.error(terminator.span, "try_call callee is not known failable");
            }
            _ => {}
        }
    }

    /// Runtime-dependent tensor extents are checked by the recorded
    /// `FunctionReturn` shape obligation. MIR validation still rejects rank,
    /// element, and fully literal mismatches here.
    pub(crate) fn return_shape_relation_assignable(
        &self,
        from: crate::MirType,
        to: crate::MirType,
    ) -> bool {
        let types = self.context.types.as_table();
        let (Type::Tensor(from_element, from_shape), Type::Tensor(to_element, to_shape)) =
            (types.get(from.semantic_id()), types.get(to.semantic_id()))
        else {
            return false;
        };
        if !types.is_assignable(*from_element, *to_element) {
            return false;
        }
        let dimensions = |shape| match types.get_index(shape) {
            IndexExpr::Tuple(items) => items.clone(),
            IndexExpr::Unspecified => Vec::new(),
            _ => vec![shape],
        };
        let from_dimensions = dimensions(*from_shape);
        let to_dimensions = dimensions(*to_shape);
        from_dimensions.len() == to_dimensions.len()
            && from_dimensions
                .iter()
                .zip(to_dimensions.iter())
                .all(|(from, to)| {
                    // Equal terms bind; a literal may pin a runtime-derived
                    // term (the recorded relation enforces it at runtime);
                    // but two affine terms over the SAME base with DIFFERENT
                    // offsets are a derived-witness mismatch and fail here
                    // (the matching-derived-witness rule).
                    types.index_equals(*from, *to)
                        || (!matches!(types.get_index(*from), IndexExpr::Literal(_))
                            || !matches!(types.get_index(*to), IndexExpr::Literal(_)))
                })
    }

    /// Validate CFG invariants: entry block existence, reachability, and
    /// termination. Called after per-block validation so successor targets
    /// have already been checked for existence.
    pub(crate) fn validate_cfg(&mut self, function: &MirFunction, scope: &FunctionScope<'_, '_>) {
        let entry = MirBlockId(0);

        // 1. Entry block must exist.
        if !scope.blocks.contains(&entry) {
            self.error(function.span, "function has no entry block bb0");
            return;
        }

        // 2. Reachability — DFS from the entry block.
        let block_map: FxHashMap<MirBlockId, &MirBlock> =
            function.blocks.iter().map(|b| (b.id, b)).collect();

        // Collect the set of all blocks that are referenced as successors
        // by at least one terminator. Blocks not referenced by any terminator
        // (e.g. join points sealed as Unreachable) are dead code created by
        // the lowerer — they are not orphan bugs.
        let mut successor_targets = FxHashSet::default();
        for block in &function.blocks {
            for succ in terminator_successors(&block.terminator.kind) {
                successor_targets.insert(succ);
            }
        }

        let mut reachable = FxHashSet::default();
        let mut stack = vec![entry];
        reachable.insert(entry);
        while let Some(current) = stack.pop() {
            if let Some(block) = block_map.get(&current) {
                for succ in terminator_successors(&block.terminator.kind) {
                    if reachable.insert(succ) {
                        stack.push(succ);
                    }
                }
            }
        }

        // A conversion recovery may be represented by an intentionally
        // detached CFG region. Its recovery operand names the temp assigned at
        // the region tail; that tail jumps back to the reachable success flow.
        // Permit only detached predecessors that flow into such an anchored
        // tail, not arbitrary orphan CFG components.
        let mut recovery_temps = FxHashSet::default();
        for block in &function.blocks {
            for statement in &block.statements {
                let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                    continue;
                };
                let MirIntrinsic::Convert(conversion) = &call.intrinsic else {
                    continue;
                };
                match conversion.recovery.as_ref() {
                    Some(MirOperand::Temp(temp)) => {
                        recovery_temps.insert(*temp);
                    }
                    Some(MirOperand::Place(place)) => {
                        if let MirPlaceBase::Temp(temp) = place.base {
                            recovery_temps.insert(temp);
                        }
                    }
                    Some(MirOperand::Value(_) | MirOperand::Constant(_)) | None => {}
                }
            }
        }

        let mut predecessors: FxHashMap<MirBlockId, Vec<MirBlockId>> = FxHashMap::default();
        for block in &function.blocks {
            for succ in terminator_successors(&block.terminator.kind) {
                predecessors.entry(succ).or_default().push(block.id);
            }
        }

        let recovery_region =
            conversion_recovery_region(function, &recovery_temps, &reachable, &predecessors);

        for block in &function.blocks {
            // Only flag blocks that are referenced as successor targets but are
            // not reachable from entry, except the detached predecessors of an
            // explicit conversion-recovery tail.
            if successor_targets.contains(&block.id)
                && !reachable.contains(&block.id)
                && !recovery_region.contains(&block.id)
            {
                self.error(
                    block.span,
                    format!("block bb{} is not reachable from entry", block.id.0),
                );
            }
        }

        // 3. Termination — every reachable block must have at least one path
        //    to Return / ReturnError / Unreachable.
        //
        //    Reverse-BFS from seed terminators: mark all blocks that can
        //    reach a seed by following predecessor edges backward. This
        //    accepts loops (the exit reaches Return) while rejecting pure
        //    Goto cycles (no path to any seed).

        // Identify seeds (blocks whose terminator is Return/ReturnError/
        // Unreachable) and reverse-BFS: follow predecessor edges backward.
        let reaches_terminator = blocks_reaching_terminator(function, &predecessors);

        for block in &function.blocks {
            if reachable.contains(&block.id) && !reaches_terminator.contains(&block.id) {
                self.error(
                    block.span,
                    format!(
                        "block bb{} does not reach a terminator \
                         (Return/ReturnError/Unreachable) on every path",
                        block.id.0,
                    ),
                );
            }
        }
    }
}

fn conversion_recovery_region(
    function: &MirFunction,
    recovery_temps: &FxHashSet<crate::MirTempId>,
    reachable: &FxHashSet<MirBlockId>,
    predecessors: &FxHashMap<MirBlockId, Vec<MirBlockId>>,
) -> FxHashSet<MirBlockId> {
    let mut recovery_region = FxHashSet::default();
    let mut recovery_stack = Vec::new();
    for block in &function.blocks {
        // The region tail either stores the recovery temp itself or, when
        // the fallback has its own branches, copies the finished recovery
        // value into the conversion destination.
        let stores_recovery = block
            .statements
            .iter()
            .any(|statement| match &statement.kind {
                MirStatementKind::Assign { place, value } => {
                    matches!(&place.base, MirPlaceBase::Temp(temp) if recovery_temps.contains(temp))
                        || matches!(
                            &value.kind,
                            crate::MirValueKind::Operand(MirOperand::Temp(temp))
                                if recovery_temps.contains(temp)
                        )
                }
                _ => false,
            });
        let joins_reachable = terminator_successors(&block.terminator.kind)
            .into_iter()
            .any(|successor| reachable.contains(&successor));
        if stores_recovery && !reachable.contains(&block.id) && joins_reachable {
            recovery_region.insert(block.id);
            recovery_stack.push(block.id);
        }
    }
    while let Some(current) = recovery_stack.pop() {
        if let Some(preds) = predecessors.get(&current) {
            for pred in preds {
                if !reachable.contains(pred) && recovery_region.insert(*pred) {
                    recovery_stack.push(*pred);
                }
            }
        }
    }
    recovery_region
}

fn blocks_reaching_terminator(
    function: &MirFunction,
    predecessors: &FxHashMap<MirBlockId, Vec<MirBlockId>>,
) -> FxHashSet<MirBlockId> {
    let mut reaches_terminator = FxHashSet::default();
    let mut stack: Vec<MirBlockId> = Vec::new();
    for block in &function.blocks {
        match &block.terminator.kind {
            MirTerminatorKind::Return(_)
            | MirTerminatorKind::ReturnError(_)
            | MirTerminatorKind::Unreachable => {
                reaches_terminator.insert(block.id);
                stack.push(block.id);
            }
            _ => {}
        }
    }
    while let Some(current) = stack.pop() {
        if let Some(preds) = predecessors.get(&current) {
            for pred in preds {
                if reaches_terminator.insert(*pred) {
                    stack.push(*pred);
                }
            }
        }
    }
    reaches_terminator
}

fn preceding_tensor_shape_check<'a>(
    previous: Option<&'a MirStatement>,
    arguments: &[crate::MirOperand],
) -> Option<&'a crate::MirRuntimeCall> {
    let Some(MirStatementKind::RuntimeCall { call, .. }) =
        previous.map(|statement| &statement.kind)
    else {
        return None;
    };
    if !matches!(&call.intrinsic, crate::MirIntrinsic::TensorShapeCheck(_))
        || call.args.get(..arguments.len())? != arguments
    {
        return None;
    }
    Some(call)
}
