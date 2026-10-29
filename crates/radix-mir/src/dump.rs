//! Deterministic MIR text rendering.
//!
//! The dump format is an inspection and regression surface for the MIR model,
//! not a parser contract. It renders IDs, type IDs, layout IDs, blocks,
//! statements, terminators, runtime intrinsics, aggregates, and option
//! operations in stable storage order so `radix mir` output is comparable across
//! runs. Symbol names are deliberately rendered as symbol IDs because this layer
//! does not own interner presentation policy.
//!
//! INVARIANTS
//! ==========
//! - Rendering follows MIR vector order; no sorting is applied by the dumper.
//! - Numeric IDs are printed in their MIR namespace (`f`, `bb`, `_`, `%`, `v`).
//! - The dump should expose structure and type identity without implying that
//!   it is valid target code.

use crate::MirGradientOp;
use crate::text::{push_fmt, push_line};
use crate::visit::MirVisitor;

const DUMP_FUNCTION_PREFIX: &str = "f";
const DUMP_BLOCK_PREFIX: &str = "bb";
const DUMP_LOCAL_PREFIX: &str = "_";
const DUMP_TEMP_PREFIX: &str = "%";
const DUMP_VALUE_PREFIX: &str = "v";
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBinOp, MirBlock,
    MirBlockId, MirCallee, MirClosureEnvironmentId, MirCollectionOp, MirConstant, MirConversion,
    MirConversionFlavor, MirDeviceBlockOp, MirDiagnosticKind, MirFunction, MirFunctionId,
    MirIntrinsic, MirKeyValueOperand, MirLocalId, MirNamedOperand, MirOperand, MirOptionChainLink,
    MirOptionOp, MirOptionUnwrapMode, MirPlace, MirPlaceBase, MirProgram, MirProjection,
    MirProvider, MirProviderKind, MirRuntimeCall, MirStatement, MirStatementKind, MirTempId,
    MirTerminator, MirTerminatorKind, MirType, MirUnOp, MirValue, MirValueId, MirValueKind,
};

/// Render a MIR program into deterministic, human-readable inspection text.
///
/// This is the public dump boundary used by developer commands and tests. It
/// accepts already-built MIR and does not validate it; callers that need
/// structural guarantees should run `validate_program` before dumping.
#[must_use]
pub fn dump_program(program: &MirProgram) -> String {
    let mut dumper = MirDumper { out: String::new() };
    dumper.visit_program(program);
    dumper.out
}

struct MirDumper {
    out: String,
}

impl MirVisitor for MirDumper {
    fn visit_function(&mut self, function: &MirFunction) {
        // `out` is empty only before the first function; separates functions.
        if !self.out.is_empty() {
            self.out.push('\n');
        }

        let error_ty = function
            .error_ty
            .map(|error_ty| format!(" ⇥ {}", ty(error_ty)))
            .unwrap_or_default();
        push_line!(
            self.out,
            "function {} -> {}{} {{",
            function_id(function.id),
            ty(function.return_ty),
            error_ty
        );
        if !function.params.is_empty() {
            self.out.push_str("  params:\n");
            for param in &function.params {
                push_line!(self.out, "    {}: {}", local_id(param.local), ty(param.ty));
            }
        }
        if !function.locals.is_empty() {
            self.out.push_str("  locals:\n");
            for local in &function.locals {
                let mutability = if local.mutable { "var" } else { "let" };
                push_line!(
                    self.out,
                    "    {} {}: {}",
                    mutability,
                    local_id(local.id),
                    ty(local.ty)
                );
            }
        }
        if !function.temps.is_empty() {
            self.out.push_str("  temps:\n");
            for temp in &function.temps {
                push_line!(self.out, "    {}: {}", temp_id(temp.id), ty(temp.ty));
            }
        }
        for block in &function.blocks {
            self.visit_block(block);
        }
        self.out.push_str("}\n");
    }

    fn visit_block(&mut self, block: &MirBlock) {
        push_line!(self.out, "  {}:", block_id(block.id));
        for stmt in &block.statements {
            self.visit_stmt(stmt);
        }
        self.visit_terminator(&block.terminator);
    }

    fn visit_stmt(&mut self, stmt: &MirStatement) {
        self.out.push_str("    ");
        self.out.push_str(&stmt_kind(&stmt.kind));
        self.out.push('\n');
    }

    fn visit_terminator(&mut self, terminator: &MirTerminator) {
        self.out.push_str("    ");
        self.out.push_str(&terminator_kind(&terminator.kind));
        self.out.push('\n');
    }
}

fn stmt_kind(kind: &MirStatementKind) -> String {
    match kind {
        MirStatementKind::Assign { place, value } => {
            format!("{} = {}", place_fmt(place), value_fmt(value))
        }
        MirStatementKind::Call {
            destination,
            callee,
            args,
        } => {
            format!(
                "{}call {}({})",
                destination_prefix(destination.as_ref()),
                callee_fmt(callee),
                operands(args)
            )
        }
        MirStatementKind::RuntimeCall { destination, call } => {
            format!(
                "{}runtime {}",
                destination_prefix(destination.as_ref()),
                runtime_call(call)
            )
        }
        MirStatementKind::Construct {
            destination,
            aggregate,
        } => {
            format!(
                "{} = construct {}",
                place_fmt(destination),
                aggregate_fmt(aggregate)
            )
        }
    }
}

fn terminator_kind(kind: &MirTerminatorKind) -> String {
    match kind {
        MirTerminatorKind::Return(Some(value)) => format!("return {}", operand(value)),
        MirTerminatorKind::Return(None) => "return".to_owned(),
        MirTerminatorKind::ReturnThread => "return_thread".to_owned(),
        MirTerminatorKind::ReturnError(value) => format!("return_error {}", operand(value)),
        MirTerminatorKind::TryCall {
            destination,
            callee,
            args,
            ok_block,
            error_place,
            error_block,
        } => {
            format!(
                "{}try_call {}({}) ok {} error {} -> {}",
                destination_prefix(destination.as_ref()),
                callee_fmt(callee),
                operands(args),
                block_id(*ok_block),
                place_fmt(error_place),
                block_id(*error_block)
            )
        }
        MirTerminatorKind::Goto(target) => format!("goto {}", block_id(*target)),
        MirTerminatorKind::Branch {
            condition,
            then_block,
            else_block,
        } => {
            format!(
                "branch {} {} {}",
                operand(condition),
                block_id(*then_block),
                block_id(*else_block)
            )
        }
        MirTerminatorKind::Switch {
            value,
            cases,
            default,
        } => {
            let rendered_cases = cases
                .iter()
                .map(|case| format!("{}: {}", constant(&case.value), block_id(case.target)))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "switch {} [{}] default {}",
                operand(value),
                rendered_cases,
                block_id(*default)
            )
        }
        MirTerminatorKind::Unreachable => "unreachable".to_owned(),
    }
}

fn destination_prefix(destination: Option<&MirPlace>) -> String {
    destination
        .map(|place| format!("{} = ", place_fmt(place)))
        .unwrap_or_default()
}

fn value_fmt(value: &MirValue) -> String {
    match &value.kind {
        MirValueKind::Operand(operand_value) => {
            format!("{}: {}", operand(operand_value), ty(value.ty))
        }
        MirValueKind::Closure(closure) => format!(
            "closure {} env {} via {}: {}",
            function_id(closure.function),
            closure_env_id(closure.environment_id),
            operand(&closure.environment),
            ty(value.ty)
        ),
        MirValueKind::Unary { op, operand: inner } => {
            format!("{} {}: {}", unop(*op), operand(inner), ty(value.ty))
        }
        MirValueKind::Binary { op, lhs, rhs } => {
            format!(
                "{} {} {}: {}",
                operand(lhs),
                binop(*op),
                operand(rhs),
                ty(value.ty)
            )
        }
        MirValueKind::Option(op) => format!("option {}: {}", option_op(op), ty(value.ty)),
        MirValueKind::TensorExtent { tensor, axis } => {
            format!("extent {} axis {}: {}", operand(tensor), axis, ty(value.ty))
        }
    }
}

fn runtime_call(call: &MirRuntimeCall) -> String {
    format!(
        "{}({}) -> {}",
        intrinsic(&call.intrinsic),
        operands(&call.args),
        ty(call.return_ty)
    )
}

fn aggregate_fmt(aggregate: &MirAggregate) -> String {
    let kind = match aggregate.kind {
        MirAggregateKind::Tuple => "tuple".to_owned(),
        MirAggregateKind::Array => "array".to_owned(),
        MirAggregateKind::Record => "record".to_owned(),
        MirAggregateKind::ClosureEnvironment(id) => format!("closure_env {}", closure_env_id(id)),
        MirAggregateKind::SharedCell => "shared_cell".to_owned(),
        MirAggregateKind::Map => "map".to_owned(),
        MirAggregateKind::Set => "set".to_owned(),
        MirAggregateKind::Tensor => "tensor".to_owned(),
        MirAggregateKind::SparseTensor => "sparse_tensor".to_owned(),
        MirAggregateKind::Interval { inclusive } => if inclusive {
            "interval_inclusive"
        } else {
            "interval_exclusive"
        }
        .to_owned(),
        MirAggregateKind::Vector => "vector".to_owned(),
        MirAggregateKind::Matrix => "matrix".to_owned(),
        MirAggregateKind::Struct(def_id) => format!("struct def#{}", def_id.0),
        MirAggregateKind::EnumVariant(def_id) => format!("variant def#{}", def_id.0),
    };
    format!(
        "{kind}: {} {}",
        ty(aggregate.ty),
        aggregate_fields(&aggregate.fields)
    )
}

fn aggregate_fields(fields: &MirAggregateFields) -> String {
    match fields {
        MirAggregateFields::Ordered(items) => {
            let rendered = items
                .iter()
                .map(aggregate_item)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{rendered}]")
        }
        MirAggregateFields::Named(items) => {
            let rendered = items
                .iter()
                .map(named_operand)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{rendered}}}")
        }
        MirAggregateFields::Keyed(items) => {
            let rendered = items
                .iter()
                .map(key_value_operand)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{{rendered}}}")
        }
    }
}

fn aggregate_item(item: &MirAggregateItem) -> String {
    match item {
        MirAggregateItem::Operand(value) => operand(value),
        MirAggregateItem::Spread(value) => format!("...{}", operand(value)),
    }
}

fn named_operand(field: &MirNamedOperand) -> String {
    format!("sym#{}: {}", field.name.0, operand(&field.value))
}

fn key_value_operand(entry: &MirKeyValueOperand) -> String {
    format!("{} => {}", operand(&entry.key), operand(&entry.value))
}

fn callee_fmt(callee: &MirCallee) -> String {
    match callee {
        MirCallee::Function(id) => function_id(*id),
        MirCallee::Definition { source: def_id, .. } => format!("def#{}", def_id.0),
        MirCallee::Closure(closure) => format!(
            "closure {} env {} via {}",
            function_id(closure.function),
            closure_env_id(closure.environment_id),
            operand(&closure.environment)
        ),
        MirCallee::Value(value) => operand(value),
    }
}

fn operands(args: &[MirOperand]) -> String {
    args.iter().map(operand).collect::<Vec<_>>().join(", ")
}

fn operand(value: &MirOperand) -> String {
    match value {
        MirOperand::Place(place) => place_fmt(place),
        MirOperand::Temp(id) => temp_id(*id),
        MirOperand::Value(id) => value_id(*id),
        MirOperand::Constant(value) => constant(value),
    }
}

fn place_fmt(place: &MirPlace) -> String {
    let mut out = match place.base {
        MirPlaceBase::Local(id) => local_id(id),
        MirPlaceBase::Temp(id) => temp_id(id),
    };
    for projection in &place.projections {
        match projection {
            MirProjection::Field(field) => {
                push_fmt!(out, ".sym#{}", field.0);
            }
            MirProjection::VariantField { variant, field } => {
                push_fmt!(out, ".def#{}.sym#{}", variant.0, field.0);
            }
            MirProjection::ClosureCapture {
                environment,
                source,
            } => {
                push_fmt!(out, ".{}.def#{}", closure_env_id(*environment), source.0);
            }
            MirProjection::SharedCell => out.push_str(".cell"),
            MirProjection::Index(index) | MirProjection::ProvenIndex(index) => {
                push_fmt!(out, "[{}]", operand(index));
            }
            MirProjection::VectorLane(index) => {
                push_fmt!(out, ".lane{index}");
            }
            MirProjection::MatrixCell { row, column } => {
                push_fmt!(out, ".cell{row}_{column}");
            }
        }
    }
    out
}

fn constant(value: &MirConstant) -> String {
    match value {
        MirConstant::Int(value) => format!("const int {value}"),
        MirConstant::UInt(value) => format!("const uint {value}"),
        MirConstant::Float(value) => format!("const float {value:?}"),
        MirConstant::String(symbol) => format!("const string sym#{}", symbol.0),
        MirConstant::Ascii(symbol) => format!("const ascii sym#{}", symbol.0),
        MirConstant::Bool(value) => format!("const bool {value}"),
        MirConstant::Nil => "const nil".to_owned(),
        MirConstant::Unit => "const unit".to_owned(),
        MirConstant::Octeti(bytes) => format!("const octeti [{}]", bytes.len()),
        MirConstant::Regex { pattern, .. } => format!("const regex sym#{}", pattern.0),
        MirConstant::Function(id) => format!("const function {}", function_id(*id)),
        MirConstant::ShapeSize { shape, axis } => {
            format!("const shape_size shape#{} axis {axis:?}", shape.0)
        }
        MirConstant::Magnus(digits) => format!("const magnus {digits}"),
    }
}

fn intrinsic(value: &MirIntrinsic) -> String {
    match value {
        MirIntrinsic::Diagnostic(kind) => format!("diagnostic {}", diagnostic_kind(*kind)),
        MirIntrinsic::Assert => "assert".to_owned(),
        MirIntrinsic::FormatString { template } => {
            format!("format_string template sym#{}", template.0)
        }
        MirIntrinsic::FormatValue { spec } => format!("format_value spec sym#{}", spec.0),
        MirIntrinsic::Convert(conversion) => conversion_intrinsic(conversion),
        MirIntrinsic::Collection(op) => format!("collection {}", collection_op(*op)),
        MirIntrinsic::Atomic(op) => format!("atomic {}", op.spelling()),
        MirIntrinsic::Panic => "panic".to_owned(),
        MirIntrinsic::Provider(provider) => provider_intrinsic(provider),
        MirIntrinsic::SermoOpen => "sermo_open".to_owned(),
        MirIntrinsic::SermoSetOpener => "sermo_set_opener".to_owned(),
        MirIntrinsic::Sermo(op) => format!("sermo_{}", op.spelling()),

        MirIntrinsic::ReadLine => "read_line".to_owned(),
        MirIntrinsic::ReadAll => "read_all".to_owned(),
        MirIntrinsic::Cede => "cede".to_owned(),
        MirIntrinsic::DeviceBlock(op) => device_block_spelling(*op),
        MirIntrinsic::GpuBuiltin(builtin) => format!("gpu_builtin {}", builtin.spelling()),
        MirIntrinsic::CursorStream(cursor) if cursor.stepped => {
            format!("cursor_stream stepped def#{}", cursor.callee.0)
        }
        MirIntrinsic::CursorStream(cursor) => format!("cursor_stream def#{}", cursor.callee.0),
        MirIntrinsic::Gradient(op) => format!("gradient_{}_op", gradient_op(*op)),
        MirIntrinsic::TypeCheck(op) => format!(
            "type_check positive={} type#{}",
            op.positive,
            op.ty.semantic_id().0
        ),
        MirIntrinsic::TensorShapeCheck(check) => {
            format!("tensor_shape_check relations={}", check.relations.len())
        }
        MirIntrinsic::TensorShapeSize(expression) => {
            format!("tensor_shape_size {expression:?}")
        }
        MirIntrinsic::TensorLimes(policy) => format!("tensor_limes {}", edge_policy(policy)),
        MirIntrinsic::FilumReduce(op) => format!(
            "filum_reduce op={} form={} lane_width={} term_binder={} coordinates={}{}{}",
            op.op.name(),
            op.form.name(),
            op.lane_width,
            op.term_binder_name.map_or(0, |s| s.0),
            op.coordinates.len(),
            op.term_captures
                .map_or_else(String::new, |captures| format!(
                    " term_fn_captures={captures}"
                )),
            op.group.as_ref().map_or_else(String::new, |group| format!(
                " group_shape={:?} group_key_captures={:?}",
                group.dimensions,
                group
                    .keys
                    .iter()
                    .map(|key| key.captures)
                    .collect::<Vec<_>>()
            ))
        ),
        MirIntrinsic::Scan(op) => format!(
            "scan op={} total={:?} reverse={} rank={} term_ty={} result_ty={} term_captures={} group_axes={}",
            op.op.name(),
            op.total,
            op.reverse,
            op.rank,
            op.term_ty.semantic_id().0,
            op.result_element_ty.semantic_id().0,
            op.term_captures,
            op.group.as_ref().map_or(0, |group| group.axes.len())
        ),
        MirIntrinsic::Duplicate => "duplicate".to_owned(),
        MirIntrinsic::Pow => "pow".to_owned(),
        MirIntrinsic::TensorLift(op) => format!("tensor_lift {}", op.name()),
        MirIntrinsic::VectorOp(op) => format!("vector_op {}", op.name()),
    }
}

fn edge_policy(policy: &crate::MirEdgePolicy) -> String {
    match policy {
        crate::MirEdgePolicy::Builtin(crate::MirBuiltinEdgePolicy::Clamp) => "clamp".to_owned(),
        crate::MirEdgePolicy::Builtin(crate::MirBuiltinEdgePolicy::Reflect) => "reflect".to_owned(),
        crate::MirEdgePolicy::Builtin(crate::MirBuiltinEdgePolicy::Wrap) => "wrap".to_owned(),
        crate::MirEdgePolicy::Custom {
            policy_def,
            remap,
            transform,
        } => format!(
            "custom def#{} remap {} transform {}",
            policy_def.0,
            function_id(*remap),
            transform.map_or_else(|| "none".to_owned(), function_id),
        ),
    }
}

fn gradient_op(op: MirGradientOp) -> &'static str {
    match op {
        MirGradientOp::Create => "create",
        MirGradientOp::Accumulate => "accumulate",
        MirGradientOp::Read => "read",
        MirGradientOp::Zero => "zero",
    }
}

fn option_op(op: &MirOptionOp) -> String {
    match op {
        MirOptionOp::None => "none".to_owned(),
        MirOptionOp::Some(value) => format!("some({})", operand(value)),
        MirOptionOp::IsNil(value) => format!("is_nil({})", operand(value)),
        MirOptionOp::IsNotNil(value) => format!("is_not_nil({})", operand(value)),
        MirOptionOp::Unwrap { value, mode } => {
            format!("unwrap_{}({})", option_unwrap_mode(*mode), operand(value))
        }
        MirOptionOp::Coalesce { value, fallback } => {
            format!("coalesce({}, {})", operand(value), operand(fallback))
        }
        MirOptionOp::Chain { base, link } => {
            format!("chain({}, {})", operand(base), option_chain_link(link))
        }
    }
}

fn option_unwrap_mode(mode: MirOptionUnwrapMode) -> &'static str {
    match mode {
        MirOptionUnwrapMode::Assert => "assert",
        MirOptionUnwrapMode::Assume => "assume",
    }
}

fn option_chain_link(link: &MirOptionChainLink) -> String {
    match link {
        MirOptionChainLink::Field(field) => format!(".sym#{}", field.0),
        MirOptionChainLink::VariantField { variant, field } => {
            format!(".def#{}.sym#{}", variant.0, field.0)
        }
        MirOptionChainLink::Index(index) => format!("[{}]", operand(index)),
        MirOptionChainLink::Call { callee, args } => {
            format!("call {}({})", callee_fmt(callee), operands(args))
        }
    }
}

fn diagnostic_kind(kind: MirDiagnosticKind) -> &'static str {
    match kind {
        MirDiagnosticKind::Nota => "nota",
        MirDiagnosticKind::Vide => "vide",
        MirDiagnosticKind::Mone => "mone",
        MirDiagnosticKind::Scribe => "scribe",
    }
}

fn conversion_intrinsic(conversion: &MirConversion) -> String {
    let params = if conversion.params.is_empty() {
        String::new()
    } else {
        format!(
            " hints [{}]",
            conversion
                .params
                .iter()
                .map(|symbol| format!("sym#{}", symbol.0))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let recovery = conversion
        .recovery
        .as_ref()
        .map(|value| format!(" recovery {}", operand(value)))
        .unwrap_or_default();
    let defaults = if conversion.struct_defaults.is_empty() {
        String::new()
    } else {
        format!(
            " defaults [{}]",
            conversion
                .struct_defaults
                .iter()
                .map(|defaults| {
                    let fields = defaults
                        .fields
                        .iter()
                        .map(|field| format!("sym#{}={}", field.name.0, operand(&field.value)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("def#{} {{{}}}", defaults.def_id.0, fields)
                })
                .collect::<Vec<_>>()
                .join("; ")
        )
    };
    format!(
        "convert {} {} -> {}{}{}{}",
        conversion_flavor(conversion.flavor),
        ty(conversion.source_ty),
        ty(conversion.target_ty),
        params,
        recovery,
        defaults
    )
}

fn conversion_flavor(flavor: MirConversionFlavor) -> &'static str {
    match flavor {
        MirConversionFlavor::Cast => "cast",
        MirConversionFlavor::Runtime => "runtime",
        MirConversionFlavor::Proven => "proven",
    }
}

fn tensor_math_dump_name(function: radix_types::MirMathFunction) -> &'static str {
    use radix_types::MirMathFunction;
    match function {
        MirMathFunction::Abs => "tensor_math_abs",
        MirMathFunction::Sqrt => "tensor_math_sqrt",
        MirMathFunction::Cbrt => "tensor_math_cbrt",
        MirMathFunction::Exp => "tensor_math_exp",
        MirMathFunction::Ln => "tensor_math_ln",
        MirMathFunction::Log10 => "tensor_math_log10",
        MirMathFunction::Sin => "tensor_math_sin",
        MirMathFunction::Cos => "tensor_math_cos",
        MirMathFunction::Tan => "tensor_math_tan",
    }
}

fn scalar_math_dump_name(function: radix_types::MirMathFunction) -> &'static str {
    use radix_types::MirMathFunction;
    match function {
        MirMathFunction::Abs => "scalar_math_abs",
        MirMathFunction::Sqrt => "scalar_math_sqrt",
        MirMathFunction::Cbrt => "scalar_math_cbrt",
        MirMathFunction::Exp => "scalar_math_exp",
        MirMathFunction::Ln => "scalar_math_ln",
        MirMathFunction::Log10 => "scalar_math_log10",
        MirMathFunction::Sin => "scalar_math_sin",
        MirMathFunction::Cos => "scalar_math_cos",
        MirMathFunction::Tan => "scalar_math_tan",
    }
}

fn collection_op(op: MirCollectionOp) -> &'static str {
    match op {
        MirCollectionOp::ArrayAdd => "array_add",
        MirCollectionOp::ArraySub => "array_sub",
        MirCollectionOp::ArrayMul => "array_mul",
        MirCollectionOp::ArrayDiv => "array_div",
        MirCollectionOp::ArrayMod => "array_mod",
        MirCollectionOp::ArrayTrueDiv => "array_true_div",
        MirCollectionOp::ArrayNeg => "array_neg",
        MirCollectionOp::ArrayLt => "array_lt",
        MirCollectionOp::ArrayGt => "array_gt",
        MirCollectionOp::ArrayLtEq => "array_lt_eq",
        MirCollectionOp::ArrayGtEq => "array_gt_eq",
        MirCollectionOp::ArrayValueEq => "array_value_eq",
        MirCollectionOp::ArrayValueNotEq => "array_value_not_eq",
        MirCollectionOp::ArrayAnd => "array_and",
        MirCollectionOp::ArrayOr => "array_or",
        MirCollectionOp::ArrayNot => "array_not",
        MirCollectionOp::Append => "append",
        MirCollectionOp::AppendImmutable => "append_immutable",
        MirCollectionOp::Put => "put",
        MirCollectionOp::Delete => "delete",
        MirCollectionOp::Index => "index",
        MirCollectionOp::IsEmpty => "is_empty",
        MirCollectionOp::Length => "length",
        MirCollectionOp::Contains => "contains",
        MirCollectionOp::First => "first",
        MirCollectionOp::Last => "last",
        MirCollectionOp::RemoveFirst => "remove_first",
        MirCollectionOp::RemoveLast => "remove_last",
        MirCollectionOp::Reverse => "reverse",
        MirCollectionOp::ReverseInPlace => "reverse_in_place",
        MirCollectionOp::Sort => "sort",
        MirCollectionOp::SortByKeys => "sort_by_keys",
        MirCollectionOp::Compare => "compare",
        MirCollectionOp::Union => "union",
        MirCollectionOp::Intersection => "intersection",
        MirCollectionOp::Difference => "difference",
        MirCollectionOp::SymmetricDifference => "symmetric_difference",
        MirCollectionOp::IsSubset => "is_subset",
        MirCollectionOp::IsSuperset => "is_superset",
        MirCollectionOp::Slice => "slice",
        MirCollectionOp::Take => "take",
        MirCollectionOp::TakeLast => "take_last",
        MirCollectionOp::DropFirst => "drop_first",
        MirCollectionOp::StartsWith => "starts_with",
        MirCollectionOp::EndsWith => "ends_with",
        MirCollectionOp::Uppercase => "uppercase",
        MirCollectionOp::Lowercase => "lowercase",
        MirCollectionOp::Trim => "trim",
        MirCollectionOp::Split => "split",
        MirCollectionOp::Replace => "replace",
        MirCollectionOp::Absolute => "absolute",
        MirCollectionOp::Signum => "signum",
        MirCollectionOp::Minimum => "minimum",
        MirCollectionOp::Maximum => "maximum",
        MirCollectionOp::Approximate => "approximate",
        MirCollectionOp::Sum => "sum",
        MirCollectionOp::ArgMax => "argmax",
        MirCollectionOp::ArgMin => "argmin",
        MirCollectionOp::TensorSumAxis => "tensor_sum_axis",
        MirCollectionOp::Round => "round",
        MirCollectionOp::Floor => "floor",
        MirCollectionOp::Ceiling => "ceiling",
        MirCollectionOp::Truncate => "truncate",
        MirCollectionOp::TensorShape => "tensor_shape",
        MirCollectionOp::TensorReshape => "tensor_reshape",
        MirCollectionOp::TensorGet => "tensor_get",
        MirCollectionOp::TensorSet => "tensor_set",
        MirCollectionOp::TensorFill => "tensor_fill",
        MirCollectionOp::TensorCreate => "tensor_create",
        MirCollectionOp::TensorFromFlat => "tensor_from_flat",
        MirCollectionOp::TensorFlatten => "tensor_flatten",
        MirCollectionOp::TensorMaterialize => "tensor_materialize",
        MirCollectionOp::TensorSlice => "tensor_slice",
        MirCollectionOp::TensorSliceNd => "tensor_slice_nd",
        MirCollectionOp::TensorRank => "tensor_rank",
        MirCollectionOp::TensorNorm => "tensor_norm",
        MirCollectionOp::TensorExpand => "tensor_expand",
        MirCollectionOp::TensorShift => "tensor_shift",
        MirCollectionOp::TensorCopyInto => "tensor_copy_into",
        MirCollectionOp::TensorAdd => "tensor_add",
        MirCollectionOp::TensorSub => "tensor_sub",
        MirCollectionOp::TensorMul => "tensor_mul",
        MirCollectionOp::TensorMatMul => "tensor_matmul",
        MirCollectionOp::TensorMean => "tensor_mean",
        MirCollectionOp::TensorMax => "tensor_max",
        MirCollectionOp::TensorMin => "tensor_min",
        MirCollectionOp::TensorLayerNorm => "tensor-layernorm",
        MirCollectionOp::TensorRmsNorm => "tensor-rms-norm",
        MirCollectionOp::TensorRope => "tensor-rope",
        MirCollectionOp::TensorRopeHalf => "tensor-rope-half",
        MirCollectionOp::TensorGather => "tensor-gather",
        MirCollectionOp::TensorScatterAdd => "tensor-scatter-add",
        MirCollectionOp::TensorCausalMaskedSoftmax => "tensor-causal-masked-softmax",
        MirCollectionOp::TensorQuantizedMatMul => "tensor-quantized-matmul",
        MirCollectionOp::TensorGroupedMatMul => "tensor-grouped-matmul",
        MirCollectionOp::TensorBatchedAttention => "tensor-batched-attention",
        MirCollectionOp::TensorSsmConv1d => "tensor-ssm-conv1d",
        MirCollectionOp::TensorSsmScan => "tensor-ssm-scan",
        MirCollectionOp::TensorDiv => "tensor_div",
        MirCollectionOp::TensorNeg => "tensor_neg",
        MirCollectionOp::TensorMath(function) => tensor_math_dump_name(function),
        MirCollectionOp::ScalarMath(function) => scalar_math_dump_name(function),
        MirCollectionOp::TensorGelu => "tensor_gelu",
        MirCollectionOp::TensorRelu => "tensor_relu",
        MirCollectionOp::TensorSoftmax => "softmax",
        MirCollectionOp::TensorCruxEntropia => "tensor_crux_entropia",
        MirCollectionOp::TensorTranspose => "tensor_transpose",
        MirCollectionOp::SparseNonzeroCount => "sparse_nonzero_count",
        MirCollectionOp::SparseDensify => "sparse_densify",
        MirCollectionOp::SparseGet => "sparse_get",
        MirCollectionOp::SparseSet => "sparse_set",
        MirCollectionOp::IntervalIntersect => "interval_intersect",
        MirCollectionOp::IntervalUnion => "interval_union",
        MirCollectionOp::IntervalLength => "interval_length",
        MirCollectionOp::IntervalContains => "interval_contains",
        MirCollectionOp::IntervalClamp => "interval_clamp",
        MirCollectionOp::IntervalMaterialize => "interval_materialize",
        MirCollectionOp::Keys => "keys",
        MirCollectionOp::Values => "values",
        MirCollectionOp::RegexMatches => "regex_matches",
        MirCollectionOp::RegexFind => "regex_find",
        MirCollectionOp::RegexFindAll => "regex_find_all",
        MirCollectionOp::RegexSplit => "regex_split",
        MirCollectionOp::RegexReplace => "regex_replace",
        MirCollectionOp::TextEscape => "text_escape",
        MirCollectionOp::RegexReplaceEach => "regex_replace_each",
        MirCollectionOp::MatchText => "match_text",
        MirCollectionOp::MatchStart => "match_start",
        MirCollectionOp::MatchEnd => "match_end",
        MirCollectionOp::MatchGroup => "match_group",
        MirCollectionOp::MatchNamed => "match_named",
        MirCollectionOp::CordCursor => "cord_cursor",
    }
}

fn provider_intrinsic(provider: &MirProvider) -> String {
    match provider.kind {
        MirProviderKind::Package => {}
        MirProviderKind::Kernel(module) => {
            return format!("kernel {}::sym#{}", module.name(), provider.name.0);
        }
    }
    let module = provider
        .module
        .iter()
        .map(|symbol| format!("sym#{}", symbol.0))
        .collect::<Vec<_>>()
        .join("/");
    if module.is_empty() {
        format!("provider sym#{}", provider.name.0)
    } else {
        format!("provider {module}::sym#{}", provider.name.0)
    }
}

fn binop(op: MirBinOp) -> &'static str {
    match op {
        MirBinOp::Add => "+",
        MirBinOp::Sub => "-",
        MirBinOp::Mul => "*",
        MirBinOp::Div => "/",
        MirBinOp::Mod => "%",
        MirBinOp::Eq => "==",
        MirBinOp::NotEq => "!=",
        MirBinOp::Lt => "<",
        MirBinOp::Gt => ">",
        MirBinOp::LtEq => "<=",
        MirBinOp::GtEq => ">=",
        MirBinOp::And => "and",
        MirBinOp::Or => "or",
        MirBinOp::Coalesce => "coalesce",
        MirBinOp::BitAnd => "&",
        MirBinOp::BitOr => "|",
        MirBinOp::BitXor => "^",
        MirBinOp::Shl => "<<",
        MirBinOp::Shr => ">>",
    }
}

fn unop(op: MirUnOp) -> &'static str {
    match op {
        MirUnOp::Neg => "neg",
        MirUnOp::Not => "not",
        MirUnOp::BitNot => "bit_not",
        MirUnOp::IsNil => "is_nil",
        MirUnOp::IsNotNil => "is_not_nil",
        MirUnOp::Sqrt => "sqrt",
        MirUnOp::Relu => "relu",
        MirUnOp::Gelu => "gelu",
        MirUnOp::Tanh => "tanh",
        MirUnOp::Exp => "exp",
        MirUnOp::Log => "log",
        MirUnOp::F16Round => "f16-round",
    }
}

fn ty(value: MirType) -> String {
    match value.layout_id() {
        Some(layout) => format!("ty#{} layout#{}", value.semantic_id().0, layout.0),
        None => format!("ty#{}", value.semantic_id().0),
    }
}

fn function_id(id: MirFunctionId) -> String {
    format!("{DUMP_FUNCTION_PREFIX}{}", id.0)
}

fn closure_env_id(id: MirClosureEnvironmentId) -> String {
    format!("env{}", id.0)
}

fn block_id(id: MirBlockId) -> String {
    format!("{DUMP_BLOCK_PREFIX}{}", id.0)
}

fn local_id(id: MirLocalId) -> String {
    format!("{DUMP_LOCAL_PREFIX}{}", id.0)
}

fn temp_id(id: MirTempId) -> String {
    format!("{DUMP_TEMP_PREFIX}{}", id.0)
}

fn value_id(id: MirValueId) -> String {
    format!("{DUMP_VALUE_PREFIX}{}", id.0)
}

fn device_block_spelling(op: MirDeviceBlockOp) -> String {
    match op {
        MirDeviceBlockOp::Enter { omnia, handler } => {
            format!(
                "device_block_enter {} handler=b{}",
                if omnia { "omnia" } else { "plain" },
                handler.0
            )
        }
        MirDeviceBlockOp::Exit { error_edge } => {
            format!(
                "device_block_exit{}",
                if error_edge { " error" } else { "" }
            )
        }
        MirDeviceBlockOp::Deliver { sync } => {
            format!("device_deliver{}", if sync { " sync" } else { "" })
        }
    }
}
