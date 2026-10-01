//! Expansion of deferred vector operations into per-lane MIR.
//!
//! [`MirIntrinsic::VectorOp`] defers a vector method until the receiver width
//! is a literal. [`expand_vector_ops`] rewrites each one, in place, into the
//! unrolled lane statements the lowerer used to emit directly: lane-wise
//! binary values, a `construct vector` for vector results, a mul/add chain for
//! `dot`, and a branch/join split for `normalized`. This module is the single
//! home of those rules; backends and the runner never see a `VectorOp`.
//!
//! INVARIANTS
//! ==========
//! - Every lane statement is built from `self`-contained rules here; nothing
//!   in the pass calls back into HIR lowering.
//! - A site that cannot expand (non-literal width, cross at width != 3, bad
//!   swizzle lane, unresolvable operand) is left untouched and reported, so
//!   [`crate::DEFERRED_VECTOR_OP_SURVIVED`] still rejects it at validation.
//! - The result is delivered with one final `destination = result` assignment,
//!   after every lane read, so a destination that aliases an operand is safe.
//!
//! OPERAND CONTRACT
//! ================
//! Lane reads need the receiver as a place and its type. An operand must be a
//! temp, an unprojected local/temp place, or a statement-defined value. A
//! place carrying projections has no resolvable type here and is reported as
//! [`VECTOR_OP_OPERAND_UNRESOLVED`]; the producer copies such a receiver into
//! a temp first.

use crate::local_ty::operand_ty;
use crate::validate::MirValidationError;
use crate::vector_width::literal_vector_width;
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBinOp, MirBlock,
    MirBlockId, MirConstant, MirFunction, MirIntrinsic, MirOperand, MirPlace, MirPlaceBase,
    MirProjection, MirStatement, MirStatementKind, MirTemp, MirTempId, MirTerminator,
    MirTerminatorKind, MirType, MirUnOp, MirValue, MirValueId, MirValueKind, MirVectorOp,
};
use radix_lexer::Span;
use radix_types::{Primitive, Type, TypeId, TypeTable};

/// The receiver width is not a literal in `1..=4`, or no operand type resolves.
pub const VECTOR_OP_WIDTH_UNSUPPORTED: &str = "vector_op_width_unsupported";
/// `cross` needs width 3.
pub const VECTOR_CROSS_WIDTH: &str = "vector_cross_width";
/// A swizzle pattern length differs from the receiver width.
pub const VECTOR_SWIZZLE_WIDTH: &str = "vector_swizzle_width";
/// A swizzle lane index is not below the receiver width.
pub const VECTOR_SWIZZLE_LANE: &str = "vector_swizzle_lane";
/// An operand is a projected place, a constant, or untyped.
pub const VECTOR_OP_OPERAND_UNRESOLVED: &str = "vector_op_operand_unresolved";
/// The operand count differs from the op's arity.
pub const VECTOR_OP_ARITY: &str = "vector_op_arity";

/// Near-zero length (at or below this) normalizes to the zero vector.
const NORMALIZE_EPSILON: f64 = 0.000_001;

/// Expand every [`MirIntrinsic::VectorOp`] in `function` into lane statements.
///
/// Call once the function has concrete types (all shape sizes literal). Sites
/// that cannot expand stay in place and are returned as errors.
#[must_use]
pub fn expand_vector_ops(types: &TypeTable, function: &mut MirFunction) -> Vec<MirValidationError> {
    let mut errors = Vec::new();
    let mut next_value = next_value_id(function);
    let mut block = 0;
    while block < function.blocks.len() {
        let mut index = 0;
        while index < function.blocks[block].statements.len() {
            let Some(site) = Site::of(&function.blocks[block].statements[index]) else {
                index += 1;
                continue;
            };
            match expand_site(types, function, &mut next_value, block, index, &site) {
                Ok(Expansion::Inline(count)) => index += count,
                Ok(Expansion::Split) => break,
                Err(error) => {
                    errors.push(error);
                    index += 1;
                }
            }
        }
        block += 1;
    }
    errors
}

struct Site {
    op: MirVectorOp,
    args: Vec<MirOperand>,
    destination: Option<MirPlace>,
    return_ty: MirType,
    span: Span,
}

impl Site {
    fn of(statement: &MirStatement) -> Option<Self> {
        let MirStatementKind::RuntimeCall { destination, call } = &statement.kind else {
            return None;
        };
        let MirIntrinsic::VectorOp(op) = &call.intrinsic else {
            return None;
        };
        Some(Self {
            op: op.clone(),
            args: call.args.clone(),
            destination: destination.clone(),
            return_ty: call.return_ty,
            span: statement.span,
        })
    }
}

enum Expansion {
    /// The call became this many statements in its own block.
    Inline(usize),
    /// The call split its block; the rest of the block moved to a join block.
    Split,
}

/// What the lane builders need to know about one site.
struct Shape {
    width: u8,
    element: MirType,
}

/// A receiver or peer operand, resolved to lane-readable storage.
enum Source {
    Place(MirPlace),
    /// A statement-defined value: copied into a temp before lane reads.
    Value(MirOperand, MirType),
}

fn site_error(site: &Site, issue: &'static str, message: &'static str) -> MirValidationError {
    MirValidationError::with_issue(site.span, issue, message)
}

fn unresolved(site: &Site) -> MirValidationError {
    site_error(
        site,
        VECTOR_OP_OPERAND_UNRESOLVED,
        "vector op operand is not a temp, plain place or value",
    )
}

fn expand_site(
    types: &TypeTable,
    function: &mut MirFunction,
    next_value: &mut u32,
    block: usize,
    index: usize,
    site: &Site,
) -> Result<Expansion, MirValidationError> {
    if site.args.len() != site.op.arity() {
        return Err(site_error(
            site,
            VECTOR_OP_ARITY,
            "vector op operand count differs from its arity",
        ));
    }
    let mut sources = Vec::with_capacity(site.args.len());
    let mut receiver_ty = None;
    for arg in &site.args {
        let (source, ty) = resolve_source(function, arg).ok_or_else(|| unresolved(site))?;
        receiver_ty.get_or_insert(ty);
        sources.push(source);
    }
    let receiver_ty = receiver_ty.ok_or_else(|| unresolved(site))?;
    let shape = resolve_shape(types, site, receiver_ty)?;
    if matches!(site.op, MirVectorOp::Cross) && shape.width != 3 {
        return Err(site_error(
            site,
            VECTOR_CROSS_WIDTH,
            "vector cross requires width 3",
        ));
    }
    if let MirVectorOp::Swizzle(lanes) = &site.op {
        check_swizzle(site, lanes, shape.width)?;
    }

    let mut head = Lanes::new(&mut function.temps, next_value, site.span);
    let places: Vec<MirPlace> = sources
        .into_iter()
        .map(|source| match source {
            Source::Place(place) => place,
            Source::Value(operand, ty) => {
                let temp = head.temp(ty);
                head.assign(MirPlace::temp(temp), operand, ty);
                MirPlace::temp(temp)
            }
        })
        .collect();
    let lhs = &places[0];
    // The final lane write may target the destination directly unless a lane
    // read could still see it: the destination base is also a source base.
    let direct = site
        .destination
        .as_ref()
        .filter(|destination| places.iter().all(|source| source.base != destination.base));
    // Binary ops always carry a peer (arity checked above); unary ops ignore it.
    let rhs = places.get(1).unwrap_or(lhs);

    let result = match &site.op {
        MirVectorOp::Elementwise(op) => elementwise(
            &mut head,
            lhs,
            rhs,
            shape.width,
            *op,
            shape.element,
            site.return_ty,
        ),
        MirVectorOp::Dot => dot(&mut head, lhs, rhs, shape.width, shape.element),
        MirVectorOp::Cross => cross(&mut head, lhs, rhs, shape.element, site.return_ty),
        MirVectorOp::Length => longitudo(&mut head, lhs, shape.width, shape.element),
        MirVectorOp::Crea => crea(&mut head, shape.width, shape.element, site.return_ty, types),
        MirVectorOp::Swizzle(lanes) => swizzle(&mut head, lhs, lanes, site.return_ty),
        MirVectorOp::Normalized => {
            let split = NormalizedSplit {
                block,
                index,
                shape: &shape,
                site,
                types,
            };
            normalized(&mut function.blocks, head, lhs, &split, direct);
            return Ok(Expansion::Split);
        }
    };
    if let Some(destination) = &site.destination {
        if direct.is_none() || !head.retarget(&result, destination) {
            head.assign(destination.clone(), result, site.return_ty);
        }
    }
    let statements = head.statements;
    let count = statements.len();
    function.blocks[block]
        .statements
        .splice(index..=index, statements);
    Ok(Expansion::Inline(count))
}

fn resolve_source(function: &MirFunction, arg: &MirOperand) -> Option<(Source, MirType)> {
    match arg {
        MirOperand::Place(place) if place.projections.is_empty() => {
            let ty = operand_ty(function, arg)?;
            Some((Source::Place(place.clone()), ty))
        }
        MirOperand::Temp(temp) => Some((
            Source::Place(MirPlace::temp(*temp)),
            function.temp_ty(*temp)?,
        )),
        MirOperand::Value(_) => {
            let ty = operand_ty(function, arg)?;
            Some((Source::Value(arg.clone(), ty), ty))
        }
        MirOperand::Place(_) | MirOperand::Constant(_) => None,
    }
}

/// Peel aliases to the underlying type row.
fn unalias(types: &TypeTable, mut ty: TypeId) -> &Type {
    loop {
        match types.get(ty) {
            Type::Alias(_, inner) => ty = *inner,
            other => return other,
        }
    }
}

fn resolve_shape(
    types: &TypeTable,
    site: &Site,
    receiver_ty: MirType,
) -> Result<Shape, MirValidationError> {
    let unsupported = || {
        site_error(
            site,
            VECTOR_OP_WIDTH_UNSUPPORTED,
            "vector op width is not a literal in 1..=4",
        )
    };
    let Type::Vector(_, width_index) = unalias(types, receiver_ty.semantic_id()) else {
        return Err(unsupported());
    };
    let width = literal_vector_width(*width_index, types)
        .filter(|width| (1..=4).contains(width))
        .ok_or_else(unsupported)?;
    let element = if site.op.returns_vector() {
        match unalias(types, site.return_ty.semantic_id()) {
            Type::Vector(element, _) => MirType::semantic(*element),
            _ => return Err(unsupported()),
        }
    } else {
        site.return_ty
    };
    Ok(Shape { width, element })
}

fn check_swizzle(site: &Site, lanes: &[u8], width: u8) -> Result<(), MirValidationError> {
    if lanes.len() != usize::from(width) {
        return Err(site_error(
            site,
            VECTOR_SWIZZLE_WIDTH,
            "vector swizzle pattern length differs from the receiver width",
        ));
    }
    if lanes.iter().any(|lane| *lane >= width) {
        return Err(site_error(
            site,
            VECTOR_SWIZZLE_LANE,
            "vector swizzle lane is not below the receiver width",
        ));
    }
    Ok(())
}

/// One past the largest value id the function defines.
fn next_value_id(function: &MirFunction) -> u32 {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match &statement.kind {
            MirStatementKind::Assign { value, .. } => Some(value.id.0 + 1),
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

/// Statement builder over a function's temp table and value-id counter.
struct Lanes<'a> {
    temps: &'a mut Vec<MirTemp>,
    next_value: &'a mut u32,
    statements: Vec<MirStatement>,
    span: Span,
}

impl<'a> Lanes<'a> {
    fn new(temps: &'a mut Vec<MirTemp>, next_value: &'a mut u32, span: Span) -> Self {
        Self {
            temps,
            next_value,
            statements: Vec::new(),
            span,
        }
    }

    /// A builder for another block that shares this one's temps and ids.
    fn fork(&mut self) -> Lanes<'_> {
        Lanes {
            temps: &mut *self.temps,
            next_value: &mut *self.next_value,
            statements: Vec::new(),
            span: self.span,
        }
    }

    fn temp(&mut self, ty: MirType) -> MirTempId {
        let id = MirTempId(u32::try_from(self.temps.len()).unwrap_or(u32::MAX));
        self.temps.push(MirTemp {
            id,
            ty,
            span: self.span,
        });
        id
    }

    fn value(&mut self, kind: MirValueKind, ty: MirType) -> MirValue {
        let id = MirValueId(*self.next_value);
        *self.next_value += 1;
        MirValue {
            id,
            kind,
            ty,
            span: self.span,
        }
    }

    fn assign(&mut self, place: MirPlace, operand: MirOperand, ty: MirType) {
        let value = self.value(MirValueKind::Operand(operand), ty);
        self.statements.push(MirStatement {
            kind: MirStatementKind::Assign { place, value },
            span: self.span,
        });
    }

    fn assign_temp(&mut self, kind: MirValueKind, ty: MirType) -> MirOperand {
        let temp = self.temp(ty);
        let value = self.value(kind, ty);
        self.statements.push(MirStatement {
            kind: MirStatementKind::Assign {
                place: MirPlace::temp(temp),
                value,
            },
            span: self.span,
        });
        MirOperand::Temp(temp)
    }

    fn binary(
        &mut self,
        op: MirBinOp,
        lhs: MirOperand,
        rhs: MirOperand,
        ty: MirType,
    ) -> MirOperand {
        self.assign_temp(MirValueKind::Binary { op, lhs, rhs }, ty)
    }

    /// Redirect the statement that defined fresh temp `result` to write
    /// `destination` instead, dropping the temp. `false` when `result` is not
    /// the last statement's fresh temp.
    fn retarget(&mut self, result: &MirOperand, destination: &MirPlace) -> bool {
        let MirOperand::Temp(fresh) = result else {
            return false;
        };
        let Some(last) = self.statements.last_mut() else {
            return false;
        };
        let defined = match &mut last.kind {
            MirStatementKind::Assign { place, .. } => place,
            MirStatementKind::Construct { destination, .. } => destination,
            _ => return false,
        };
        if *defined != MirPlace::temp(*fresh)
            || self.temps.last().map(|temp| temp.id) != Some(*fresh)
        {
            return false;
        }
        *defined = destination.clone();
        self.temps.pop();
        true
    }

    fn construct_vector(&mut self, lanes: Vec<MirOperand>, ty: MirType) -> MirOperand {
        let temp = self.temp(ty);
        self.statements.push(MirStatement {
            kind: MirStatementKind::Construct {
                destination: MirPlace::temp(temp),
                aggregate: MirAggregate {
                    kind: MirAggregateKind::Vector,
                    ty,
                    fields: MirAggregateFields::Ordered(
                        lanes.into_iter().map(MirAggregateItem::Operand).collect(),
                    ),
                },
            },
            span: self.span,
        });
        MirOperand::Temp(temp)
    }
}

fn lane(place: &MirPlace, lane: u8) -> MirOperand {
    let mut projections = place.projections.clone();
    projections.push(MirProjection::VectorLane(lane));
    MirOperand::Place(MirPlace {
        base: match place.base {
            MirPlaceBase::Local(id) => MirPlaceBase::Local(id),
            MirPlaceBase::Temp(id) => MirPlaceBase::Temp(id),
        },
        projections,
    })
}

fn elementwise(
    b: &mut Lanes<'_>,
    lhs: &MirPlace,
    rhs: &MirPlace,
    width: u8,
    op: MirBinOp,
    element: MirType,
    vector: MirType,
) -> MirOperand {
    let fields = (0..width)
        .map(|i| b.binary(op, lane(lhs, i), lane(rhs, i), element))
        .collect();
    b.construct_vector(fields, vector)
}

/// `(lhs_a, rhs_a, lhs_b, rhs_b)` per result lane: `l[a]*r[a'] - l[b]*r[b']`.
const CROSS_LANES: [(u8, u8, u8, u8); 3] = [(1, 2, 2, 1), (2, 0, 0, 2), (0, 1, 1, 0)];

fn cross(
    b: &mut Lanes<'_>,
    lhs: &MirPlace,
    rhs: &MirPlace,
    element: MirType,
    vector: MirType,
) -> MirOperand {
    let fields = CROSS_LANES
        .into_iter()
        .map(|(lhs_a, rhs_a, lhs_b, rhs_b)| {
            let a = b.binary(MirBinOp::Mul, lane(lhs, lhs_a), lane(rhs, rhs_a), element);
            let c = b.binary(MirBinOp::Mul, lane(lhs, lhs_b), lane(rhs, rhs_b), element);
            b.binary(MirBinOp::Sub, a, c, element)
        })
        .collect();
    b.construct_vector(fields, vector)
}

fn dot(
    b: &mut Lanes<'_>,
    lhs: &MirPlace,
    rhs: &MirPlace,
    width: u8,
    element: MirType,
) -> MirOperand {
    let mut products = (0..width)
        .map(|i| b.binary(MirBinOp::Mul, lane(lhs, i), lane(rhs, i), element))
        .collect::<Vec<_>>()
        .into_iter();
    // Width is at least 1, so the first product exists.
    let mut sum = products
        .next()
        .unwrap_or(MirOperand::Constant(MirConstant::Float(0.0)));
    for product in products {
        sum = b.binary(MirBinOp::Add, sum, product, element);
    }
    sum
}

/// `||v|| = sqrt(dot(v, v))`.
fn longitudo(b: &mut Lanes<'_>, vector: &MirPlace, width: u8, element: MirType) -> MirOperand {
    let sum_squares = dot(b, vector, vector, width, element);
    b.assign_temp(
        MirValueKind::Unary {
            op: MirUnOp::Sqrt,
            operand: sum_squares,
        },
        element,
    )
}

/// Every lane is the element zero at the receiver's width.
fn crea(
    b: &mut Lanes<'_>,
    width: u8,
    element: MirType,
    vector: MirType,
    types: &TypeTable,
) -> MirOperand {
    let fields = (0..width)
        .map(|_| {
            b.assign_temp(
                MirValueKind::Operand(MirOperand::Constant(numeric_zero_constant(
                    element.semantic_id(),
                    types,
                ))),
                element,
            )
        })
        .collect();
    b.construct_vector(fields, vector)
}

/// Numeric zero of an element type: `0.0` for floats, `0` otherwise.
///
/// Also the lowerer's dead-slot fill for padded tensor conversions, so the
/// rule exists once.
#[must_use]
pub fn numeric_zero_constant(element: TypeId, types: &TypeTable) -> MirConstant {
    match unalias(types, element) {
        Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {
            MirConstant::Float(0.0)
        }
        _ => MirConstant::Int(0),
    }
}

fn swizzle(b: &mut Lanes<'_>, source: &MirPlace, lanes: &[u8], vector: MirType) -> MirOperand {
    let fields = lanes.iter().map(|index| lane(source, *index)).collect();
    b.construct_vector(fields, vector)
}

struct NormalizedSplit<'a> {
    block: usize,
    index: usize,
    shape: &'a Shape,
    site: &'a Site,
    types: &'a TypeTable,
}

/// Unit vector; a length at or below [`NORMALIZE_EPSILON`] yields the zero
/// vector. The call's block splits: the head keeps everything before the call
/// and branches to a scale block or a zero block, and a join block takes the
/// result assignment plus everything that followed the call, including the
/// original terminator.
fn normalized(
    blocks: &mut Vec<MirBlock>,
    mut head: Lanes<'_>,
    vector: &MirPlace,
    split: &NormalizedSplit<'_>,
    direct: Option<&MirPlace>,
) {
    let NormalizedSplit {
        block,
        index,
        shape,
        site,
        types,
    } = *split;
    let (width, element, vector_ty, span) = (shape.width, shape.element, site.return_ty, site.span);

    let length = longitudo(&mut head, vector, width, element);
    let epsilon = MirOperand::Constant(MirConstant::Float(NORMALIZE_EPSILON));
    let nonzero = head.assign_temp(
        MirValueKind::Binary {
            op: MirBinOp::Gt,
            lhs: length.clone(),
            rhs: epsilon,
        },
        MirType::semantic(types.primitive(Primitive::Bivalens)),
    );
    // Both arms write one place: the destination when no lane read can see
    // it, otherwise a fresh temp that the join copies to the destination.
    let result_place = match direct {
        Some(destination) => destination.clone(),
        None => MirPlace::temp(head.temp(vector_ty)),
    };

    let scale_statements = {
        let mut b = head.fork();
        let scaled = (0..width)
            .map(|i| b.binary(MirBinOp::Div, lane(vector, i), length.clone(), element))
            .collect();
        let scaled = b.construct_vector(scaled, vector_ty);
        b.assign(result_place.clone(), scaled, vector_ty);
        b.statements
    };
    let zero_statements = {
        let mut b = head.fork();
        let zeros = crea(&mut b, width, element, vector_ty, types);
        b.assign(result_place.clone(), zeros, vector_ty);
        b.statements
    };

    let join_head = {
        let mut b = head.fork();
        if let (None, Some(destination)) = (direct, &site.destination) {
            b.assign(
                destination.clone(),
                MirOperand::Place(result_place.clone()),
                vector_ty,
            );
        }
        b.statements
    };

    let first_new = blocks
        .iter()
        .map(|existing| existing.id.0 + 1)
        .max()
        .unwrap_or(0);
    let (scale_id, zero_id, join_id) = (
        MirBlockId(first_new),
        MirBlockId(first_new + 1),
        MirBlockId(first_new + 2),
    );

    let mut tail = blocks[block].statements.split_off(index);
    tail.remove(0);
    let original_terminator = std::mem::replace(
        &mut blocks[block].terminator,
        MirTerminator {
            kind: MirTerminatorKind::Branch {
                condition: nonzero,
                then_block: scale_id,
                else_block: zero_id,
            },
            span,
        },
    );
    blocks[block].statements.extend(head.statements);

    let goto_join = || MirTerminator {
        kind: MirTerminatorKind::Goto(join_id),
        span,
    };
    blocks.push(MirBlock {
        id: scale_id,
        statements: scale_statements,
        terminator: goto_join(),
        span,
    });
    blocks.push(MirBlock {
        id: zero_id,
        statements: zero_statements,
        terminator: goto_join(),
        span,
    });
    let mut join = join_head;
    join.extend(tail);
    blocks.push(MirBlock {
        id: join_id,
        statements: join,
        terminator: original_terminator,
        span,
    });
}
