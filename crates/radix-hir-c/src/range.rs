//! Range loops: `for range a‥b step s const i`, and the range product
//! `for range a‥b, c‥d const i, const j`.
//!
//! The counter is an exact `faber_i128`, as the runner's is: an inclusive walk
//! to `i64::MAX` steps past it, and only a step beyond the representation cap
//! traps (`numerus overflow`, through `faber_add` / `faber_sub`). Bounds are
//! evaluated once, in source order, before the walk; the direction comes from
//! the bounds (`start ≤ end` ascends) and a written step is a positive stride,
//! so a zero or negative one aborts at run time. When the bounds and the
//! stride all fold to compile-time constants in `int64_t`, and the one step
//! past the last visited value stays in range, the walk provably never leaves
//! `int64_t` and runs in one instead. A range product evaluates every axis's
//! bounds first, then nests the loops first-axis-outermost, so C's `break` and
//! `continue` act on the innermost axis as `rumpe` and `perge` do.

use crate::Emitter;
use crate::Local;
use crate::error::unsupported;
use crate::expr::Value;
use crate::helpers::Helper;
use crate::ty::Ty;
use radix_codegen_shared::CodegenError;
use radix_codegen_shared::names::prefixed_binding_name;
use radix_hir::{DefId, HirBlock, HirExpression, HirExpressionKind, HirRangeKind};
use radix_lexer::Symbol;

/// One bound: its C text lands in `const faber_i128 name`, and a folded
/// constant comes back for the boundedness proof.
struct RangeBound {
    /// The bound is an `int64_t` value, so a plain cast to the binder is exact.
    fits: bool,
    /// The bound as a compile-time constant, when folding produced one.
    constant: Option<i128>,
}

/// One axis: the C names its loop head reads.
struct Axis {
    counter: String,
    start: String,
    end: String,
    up: String,
    /// The stride magnitude: a variable for a written step, `1` otherwise.
    step: String,
    inclusive: bool,
    /// Both bounds are `int64_t` values, so the counter always fits an `int`
    /// binder and a plain cast is exact.
    fits: bool,
    /// Bounds and stride are compile-time constants in `int64_t` and the one
    /// step past the last visited value stays in range, so the walk provably
    /// never leaves `int64_t` and runs in one, ascending when true.
    narrow: Option<bool>,
}

impl Axis {
    fn head(&self) -> String {
        let (below, above) = if self.inclusive {
            ("<=", ">=")
        } else {
            ("<", ">")
        };
        let Self {
            counter,
            start,
            end,
            up,
            step,
            ..
        } = self;
        if let Some(rising) = self.narrow {
            let (test, stride) = if rising { (below, "+=") } else { (above, "-=") };
            let stride = if step == "1" {
                format!("{stride} {step}")
            } else {
                format!("{stride} (int64_t){step}")
            };
            return format!(
                "for (int64_t {counter} = (int64_t){start}; {counter} {test} (int64_t){end}; {counter} {stride}) {{"
            );
        }
        format!(
            "for (faber_i128 {counter} = {start}; {up} ? {counter} {below} {end} : {counter} {above} {end}; {counter} = {up} ? faber_add({counter}, {step}) : faber_sub({counter}, {step})) {{"
        )
    }
}

/// The walk direction when bounds and stride are compile-time constants that
/// fit an `int64_t` and the move past the last visited value stays in range:
/// the C for-loop tests after stepping, so that overshoot is the only value
/// the counter holds beyond the visited span.
fn bounded_walk(
    start: Option<i128>,
    end: Option<i128>,
    stride: Option<i128>,
    inclusive: bool,
) -> Option<bool> {
    let (start, end) = (start?, end?);
    let stride = stride?;
    if stride <= 0
        || i64::try_from(start).is_err()
        || i64::try_from(end).is_err()
        || i64::try_from(stride).is_err()
    {
        return None;
    }
    let rising = start <= end;
    // The last visited value: the far bound itself when inclusive, the one
    // inside it otherwise.
    let boundary = if inclusive {
        end
    } else if rising {
        end - 1
    } else {
        end + 1
    };
    let overshoot = if rising {
        boundary + stride
    } else {
        boundary - stride
    };
    i64::try_from(overshoot).is_ok().then_some(rising)
}

impl Emitter<'_> {
    /// `for range a‥b …`: one axis, one loop.
    pub(crate) fn range_loop(
        &mut self,
        out: &mut String,
        binding: (DefId, Symbol),
        iter: &HirExpression,
        body: &HirBlock,
        mutable: bool,
    ) -> Result<(), CodegenError> {
        self.range_product(out, &[iter], &[binding], body, mutable)
    }

    /// The axes' bounds first, in order, then the nested loops.
    pub(crate) fn range_product(
        &mut self,
        out: &mut String,
        ranges: &[&HirExpression],
        bindings: &[(DefId, Symbol)],
        body: &HirBlock,
        mutable: bool,
    ) -> Result<(), CodegenError> {
        if ranges.len() != bindings.len() || ranges.is_empty() {
            return Err(unsupported(
                "range product with unequal range and binder counts",
            ));
        }
        self.support.need(Helper::Types);
        self.support.need(Helper::Add);
        self.support.need(Helper::Sub);
        self.support.need_bool();
        let base = self.depth;
        self.line(out, "{");
        self.depth = base + 1;
        let mut axes = Vec::with_capacity(ranges.len());
        for range in ranges {
            axes.push(self.range_axis(out, range)?);
        }
        for (level, (axis, (binding, name))) in axes.iter().zip(bindings).enumerate() {
            self.depth = base + 1 + level;
            self.line(out, &axis.head());
            self.depth += 1;
            let local = self.range_binder(out, axis, *binding, *name, mutable)?;
            self.locals.insert(*binding, local);
        }
        // `block` writes the body one level below the depth it is given.
        self.depth = base + axes.len();
        self.loops += 1;
        self.block(out, body)?;
        self.loops -= 1;
        for level in (0..axes.len()).rev() {
            self.depth = base + 1 + level;
            self.line(out, "}");
        }
        self.depth = base;
        self.line(out, "}");
        Ok(())
    }

    /// Evaluate one range's bounds into constants and settle its direction.
    fn range_axis(
        &mut self,
        out: &mut String,
        range: &HirExpression,
    ) -> Result<Axis, CodegenError> {
        let HirExpressionKind::Intervallum {
            start,
            end,
            step,
            kind,
        } = &range.kind
        else {
            return Err(unsupported("range loop over a value that is not a range"));
        };
        let n = self.temps;
        self.temps += 1;
        let start_name = format!("faber_a{n}");
        let end_name = format!("faber_e{n}");
        let start = self.range_bound(out, start, &start_name)?;
        let end = self.range_bound(out, end, &end_name)?;
        let (step_text, stride) = match step {
            Some(step) => {
                let step_name = format!("faber_s{n}");
                let bound = self.range_bound(out, step, &step_name)?;
                self.support.need(Helper::Trap);
                self.line(
                    out,
                    &format!(
                        "if (!({step_name} > 0)) {{ faber_abort(\"range step must be positive\"); }}"
                    ),
                );
                (step_name, bound.constant)
            }
            None => ("1".to_owned(), Some(1)),
        };
        let narrow = bounded_walk(
            start.constant,
            end.constant,
            stride,
            matches!(kind, HirRangeKind::Inclusive),
        );
        // The direction flag exists for the exact walk, which learns it at run
        // time; a bounded walk settled it at emit time.
        let up = if narrow.is_some() {
            String::new()
        } else {
            let up = format!("faber_u{n}");
            self.line(
                out,
                &format!("const bool {up} = {start_name} <= {end_name};"),
            );
            up
        };
        Ok(Axis {
            counter: format!("faber_r{n}"),
            start: start_name,
            end: end_name,
            up,
            step: step_text,
            inclusive: matches!(kind, HirRangeKind::Inclusive),
            fits: start.fits && end.fits,
            narrow,
        })
    }

    /// One bound into `const faber_i128 name`, with its folded constant.
    fn range_bound(
        &mut self,
        out: &mut String,
        expr: &HirExpression,
        name: &str,
    ) -> Result<RangeBound, CodegenError> {
        let value = self.expr(expr)?;
        if !matches!(value.value, Value::Int | Value::Exact) {
            return Err(unsupported("range bound that is not an integer"));
        }
        self.line(out, &format!("const faber_i128 {name} = {};", value.text));
        Ok(RangeBound {
            fits: value.value == Value::Int,
            constant: value.constant,
        })
    }

    /// The loop binder: the counter at the top of every pass, as an `int`.
    fn range_binder(
        &mut self,
        out: &mut String,
        axis: &Axis,
        binding: DefId,
        name: Symbol,
        mutable: bool,
    ) -> Result<Local, CodegenError> {
        let Some(kind) = self.names.binding_kind(binding) else {
            return Err(unsupported(
                "loop binding the name catalog does not classify",
            ));
        };
        let raw = self.names.resolve_symbol(name);
        let c_name = self.scope.claim(&prefixed_binding_name(kind, raw));
        self.support.need_ints();
        let value = if axis.narrow.is_some() {
            // A bounded walk's counter is an `int64_t` already.
            axis.counter.clone()
        } else if axis.fits {
            format!("(int64_t){}", axis.counter)
        } else {
            self.support.need(Helper::StoreI64);
            format!("faber_store_i64({}, \"range binding\")", axis.counter)
        };
        let declaration = self.declare(Ty::Int, &c_name, mutable)?;
        self.line(out, &format!("{declaration} = {value};"));
        Ok(Local {
            name: c_name,
            ty: Ty::Int,
            constant: None,
        })
    }
}
