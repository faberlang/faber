//! MC1 — the three-direction registry↔MIR bijection ratchet (probe-derived).
//!
//! Direction 1 (registry → owner): every `INTRINSICS` row is resolved by a
//! synthetic method call through the real module MIR lowerer — one probe per
//! admitted receiver dtype (the sweep is load-bearing: pre-`cc869ff8b`
//! `lower_potentia` armed fractus only and a single-dtype probe would have
//! stayed green on the numerus gap). A row that reaches the
//! unsupported-method fallthrough on ANY admitted dtype is a gap.
//!
//! Direction 2 (owner → admission): every `MirCollectionOp` variant is
//! registry-reachable, AIR/statement-path reachable, or on the permanent
//! internal allowlist (a design fact, not a worklist).
//!
//! Direction 3 (AIR/device allowlist → owner): every AIR-lane tensor method
//! name is registry-backed with a green MIR probe, on the permanent AIR-lane
//! class list (a design fact, not a worklist), or on the ratchet. The
//! candidate universe derives structurally from radix-semantic's admission
//! sets — no hand-assembled mirror. Pins the `mappata`/`filtrata`
//! unarmed-admission residual.
//!
//! The ratchet allowlist plus the permanent AIR-lane class list MUST equal the
//! probe-derived gap set (set equality, both directions): a new gap turns red
//! until consciously allowlisted with a unit citation; a stale entry after
//! its unit lands turns red until removed. Allowlist empty == done. Never
//! replace the probe with a test-local mirror table — that is the drift the
//! census audit (6aa998a8) caught.

use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::intrinsics::{
    INTRINSICS, IntrinsicParamKind, IntrinsicReceiver, IntrinsicReturnKind, lookup_method, mir_collection_op,
};
use crate::mir::MirLoweringOptions;
use radix_types::{GenericParamSort, MirCollectionOp};
use std::collections::BTreeSet;
use std::fmt::Write as _;

/// One synthetic-probe receiver dtype: the fn-parameter shape for the
/// immutable probe, the `varia` local for the mutable probe (imperative rows
/// need a mutable place), and the extra bindings the row's arguments need.
struct Dtype {
    /// Gap-report key suffix, e.g. `Numeric.nega/0@numerus`.
    name: &'static str,
    /// Fn-parameter list for the immutable receiver probe.
    param: String,
    /// `varia <ty> r ← <init>` local declaration for the mutable probe
    /// (imperative rows need a mutable place; a `fixum` binding or unmarked
    /// parameter is not one). Empty when the receiver cannot be constructed
    /// as a local and `extra_param` carries it as an `in` parameter.
    decl: String,
    /// Extra fn parameters that survive into the mutable probe (peer sets, or
    /// the `in` receiver of a dtype with no local constructor).
    extra_param: String,
    /// Extra bindings the row's arguments need (peer sets, flat data).
    locals: String,
    /// Element literal matching the receiver element dtype.
    el: String,
    /// Element type spelling for synthetic closure parameters.
    el_ty: String,
    /// Index-vector literal matching the receiver rank (tensor/sparsa rows).
    idx: String,
}

fn lista_dtypes() -> Vec<Dtype> {
    vec![dtype(
        "numerus",
        "lista<numerus>",
        "varia lista<numerus> r ← [1, 2, 3]",
        "",
        "",
        "1",
        "numerus",
        "0",
    )]
}

fn numeric_dtypes() -> Vec<Dtype> {
    // The Numeric family is admitted on BOTH scalar dtypes; sweeping both is
    // the law (SEM011 numerus-only rows reject fractus at the gate and the
    // sweep pins that instead of counting it green).
    vec![
        dtype("numerus", "numerus", "varia numerus r ← 5", "", "", "1", "numerus", "0"),
        dtype("fractus", "fractus", "varia fractus r ← 1.5", "", "", "1.5", "fractus", "0"),
    ]
}

fn dtypes_for(receiver: IntrinsicReceiver) -> Vec<Dtype> {
    match receiver {
        IntrinsicReceiver::Lista => lista_dtypes(),
        IntrinsicReceiver::Numeric => numeric_dtypes(),
        IntrinsicReceiver::Fractus => vec![dtype(
            "fractus",
            "fractus",
            "varia fractus r ← 1.5",
            "",
            "",
            "1.5",
            "fractus",
            "0",
        )],
        IntrinsicReceiver::Tabula => vec![dtype(
            "textus→numerus",
            "tabula<textus, numerus>",
            "varia tabula<textus, numerus> r ← vacua",
            "",
            "",
            "1",
            "numerus",
            "0",
        )],
        IntrinsicReceiver::Copia => vec![dtype(
            "numerus",
            "copia<numerus> r, copia<numerus> altera",
            "varia copia<numerus> r ← vacua",
            "copia<numerus> altera",
            "",
            "1",
            "numerus",
            "0",
        )],
        IntrinsicReceiver::Textus => vec![dtype(
            "textus",
            "textus",
            "varia textus r ← \"abc\"",
            "",
            "",
            "\"xy\"",
            "textus",
            "0",
        )],
        IntrinsicReceiver::Ascii => vec![dtype(
            "ascii",
            "ascii",
            "varia ascii r ← \"xy\" ↦ ascii",
            "",
            "",
            "\"xy\"",
            "ascii",
            "0",
        )],
        IntrinsicReceiver::Queue => vec![dtype(
            "numerus",
            "queue<numerus>",
            "varia queue<numerus> r ← vacua",
            "",
            "",
            "1",
            "numerus",
            "0",
        )],
        IntrinsicReceiver::Stack => vec![dtype(
            "numerus",
            "stack<numerus>",
            "varia stack<numerus> r ← vacua",
            "",
            "",
            "1",
            "numerus",
            "0",
        )],
        IntrinsicReceiver::Tensor => vec![
            dtype(
                "tf32",
                "tf32[3]",
                "varia tf32[3] r ← [0.5, 0.5, 0.5] ↦ tf32[3]",
                "",
                "sit lista<f32> flat ← [0.5, 0.5, 0.5]",
                "0.5",
                "fractus",
                "[0]",
            ),
            dtype(
                "ti32",
                "ti32[3]",
                "varia ti32[3] r ← [1, 1, 1] ↦ ti32[3]",
                "",
                "sit lista<i32> flat ← [1, 1, 1]",
                "1",
                "numerus",
                "[0]",
            ),
        ],
        // Width 3: `transversum` (cross) requires width 3 (deliberate
        // ceiling, goal §5); the sweep picks the fully-admitted width.
        IntrinsicReceiver::Vector => vec![dtype(
            "vf32",
            "vf32[3]",
            "varia vf32[3] r ← [0.5, 0.5, 0.5] ↦ vf32[3]",
            "",
            "",
            "0.5",
            "fractus",
            "0",
        )],
        IntrinsicReceiver::Matrix => vec![dtype(
            "fractus",
            "matrix<fractus, [2, 2]> r",
            "varia matrix<fractus, [2, 2]> r ← [[1.0, 0.0], [0.0, 1.0]] ↦ matrix<fractus, [2, 2]>",
            "",
            "",
            "0.5",
            "fractus",
            // The coordinate lista the Matrix `accipe`/`apud`/`ponde` rows
            // take; `[0, 0]` is provably in range for `[2, 2]`.
            "[0, 0]",
        )],
        IntrinsicReceiver::Sparsa => vec![dtype(
            "fractus",
            "sparsa<fractus, [2, 3]> r",
            "varia sparsa<fractus, [2, 3]> r ← vacua",
            "",
            "",
            "0.5",
            "fractus",
            "[0, 0]",
        )],
        IntrinsicReceiver::Intervallum => vec![dtype(
            "numerus",
            "intervallum<numerus> r",
            "varia intervallum<numerus> r ← 0‥6",
            "",
            "",
            "1",
            "numerus",
            "0",
        )],
        IntrinsicReceiver::Atomic => vec![dtype(
            "i32",
            "atomic<i32> r",
            // An atomic has no constructor, so the mutable probe takes the
            // receiver as an `in` (mutable-borrow) parameter instead of a local.
            "",
            "in atomic<i32> r",
            "sit i32 v ← 1 ↦ i32",
            "v",
            "numerus",
            "0",
        )],
        // `regex` has no bare literal; a typed parameter carries the receiver.
        IntrinsicReceiver::Regex => vec![dtype(
            "regex",
            "regex r",
            "sit regex r ← \"[0-9]+\" ↦ regex",
            "",
            "",
            "\"12\"",
            "textus",
            "0",
        )],
        // A match has no source type name (RD-3): the receiver is the result
        // of `find`, held in an inferred binding and asserted non-null at the
        // call (`r!.method()`, see `probe_source`). The `regex` parameter
        // survives into the mutable probe through `extra_param`.
        IntrinsicReceiver::Match => vec![dtype(
            "match",
            "regex rx",
            "",
            "regex rx",
            "sit _ r ← rx.inventa(\"xy\")",
            "\"xy\"",
            "textus",
            "0",
        )],
    }
}

#[allow(clippy::too_many_arguments)]
fn dtype(
    name: &'static str,
    param: &str,
    decl: &str,
    extra_param: &str,
    locals: &str,
    el: &str,
    el_ty: &str,
    idx: &str,
) -> Dtype {
    Dtype {
        name,
        param: param.to_string(),
        decl: decl.to_string(),
        extra_param: extra_param.to_string(),
        locals: locals.to_string(),
        el: el.to_string(),
        el_ty: el_ty.to_string(),
        idx: idx.to_string(),
    }
}

/// Per-row overrides where the generic param-kind synthesis cannot know the
/// shape contract (rank-2 gathers, constructors, normalization triples). Some
/// rows also need a different receiver shape than the family default. Returns
/// `(param_override, args)`.
fn row_override(key: &str, dtype: &Dtype) -> Option<(Option<String>, Vec<String>)> {
    let el = dtype.el.clone();
    let idx = dtype.idx.clone();
    // Element sugar for this dtype (`tf32` / `ti32`).
    let sugar = dtype.name;
    let (param, args): (Option<String>, Vec<String>) = match key {
        "Tensor.forma/1" => (None, vec!["[3]".into()]),
        // A broadcast view needs a target shape the receiver `[3]` stretches to.
        "Tensor.expande/1" => (None, vec!["[2, 3]".into()]),
        // The axis-0 slice shape is inferred from literal `start, end, step`; a
        // zero step has no shape (SEM014), so the probe supplies a unit stride.
        "Tensor.sectio/3" => (None, vec!["0".into(), "3".into(), "1".into()]),
        // `strue` constructs from a rank-0 seed (corpus gather exemplar shape).
        "Tensor.strue/2" => (Some(format!("{sugar}[0] r")), vec!["flat".into(), "[3]".into()]),
        "Tensor.crea/2" => (None, vec![el.clone(), "[3]".into()]),
        // Row gather selects rows of a `[rows, D]` table by a `tensor<u32, [n]>` id vector.
        "Tensor.gather/1" => (Some(format!("{sugar}[4, 3] r, tensor<u32, [4]> ids")), vec!["ids".into()]),
        "Tensor.matmul/1" => (Some(format!("{sugar}[2, 3] r, {sugar}[3, 2] p")), vec!["p".into()]),
        "Tensor.rms_norm/3" => (None, vec!["axis 0".into(), el.clone(), "r".into()]),
        "Tensor.rope_norm/2" => (Some(format!("{sugar}[2, 4, 3] r")), vec!["0".into(), "r".into()]),
        "Tensor.rope_half/2" => (Some(format!("{sugar}[2, 4, 3] r")), vec!["0".into(), "r".into()]),
        "Tensor.laminatio/2" => (None, vec!["axis 0".into(), el.clone()]),
        "Tensor.laminatio/3" => (None, vec!["axis 0".into(), el.clone(), "r".into()]),
        "Tensor.laminatio/4" => (None, vec!["axis 0".into(), el.clone(), "r".into(), "r".into()]),
        "Tensor.crux_entropia/1" => (None, vec!["r".into()]),
        // Matrix-vector application takes a conformable vector operand.
        "Matrix.applica/1" => (Some("matrix<fractus, [2, 2]> r, vf64[2] v".to_string()), vec!["v".into()]),
        // Same-width swizzle mask (width 3 receiver).
        "Vector.swizzle/1" => (None, vec!["\"xyz\"".into()]),
        "Tensor.pone/2" | "Sparsa.pone/2" => (None, vec![idx, el]),
        // The list join takes another list of the receiver's element type;
        // the receiver itself is one (`xs.extend(xs)` is legal).
        "Lista.extende/1" => (None, vec!["r".into()]),
        _ => return None,
    };
    Some((param, args))
}

fn arg_expr(kind: IntrinsicParamKind, dtype: &Dtype, receiver_expr: &str) -> String {
    match kind {
        IntrinsicParamKind::Inner | IntrinsicParamKind::AtomicElement => dtype.el.clone(),
        IntrinsicParamKind::Key => "\"k\"".into(),
        IntrinsicParamKind::Value => dtype.el.clone(),
        IntrinsicParamKind::NumerusIndex => "0".into(),
        IntrinsicParamKind::Textus | IntrinsicParamKind::TextusOrReplacer => "\"xy\"".into(),
        IntrinsicParamKind::SetInner => "altera".into(),
        IntrinsicParamKind::Predicate => format!("{} x ∴ x ≻ {}", dtype.el_ty, dtype.el),
        IntrinsicParamKind::Mapper => format!("{} x ∴ x * 2", dtype.el_ty),
        IntrinsicParamKind::Reducer => {
            format!("({} collectus, {} x) ∴ collectus + x", dtype.el_ty, dtype.el_ty)
        }
        IntrinsicParamKind::Accumulator => {
            if dtype.el_ty == "fractus" {
                "0.0".into()
            } else {
                "0".into()
            }
        }
        IntrinsicParamKind::NumerusArray => dtype.idx.clone(),
        IntrinsicParamKind::ElementArray => "flat".into(),
        IntrinsicParamKind::TensorIndexIds => "ids".into(),
        IntrinsicParamKind::ReceiverSelf => receiver_expr.to_string(),
        IntrinsicParamKind::TensorPeer | IntrinsicParamKind::TensorMatmulPeer => receiver_expr.to_string(),
    }
}

fn row_key(row: &crate::intrinsics::MethodIntrinsic) -> String {
    format!("{:?}.{}/{}", row.receiver, row.faber_name, row.arg_count)
}

fn probe_source(row: &crate::intrinsics::MethodIntrinsic, dtype: &Dtype, mutable: bool) -> String {
    let key = row_key(row);
    let sugar = dtype.name;
    let (param_override, override_args) = row_override(&key, dtype).map_or((None, Vec::new()), |(p, a)| (p, a));
    let (mut param, locals) = if let Some(param) = param_override {
        (param, dtype.locals.clone())
    } else if dtype.param.is_empty() {
        ("r".to_string(), dtype.locals.clone())
    } else if dtype.param.contains(" r") {
        // Multi-fragment param lists (peer sets) already carry their names.
        (dtype.param.clone(), dtype.locals.clone())
    } else {
        (format!("{} r", dtype.param), dtype.locals.clone())
    };
    let mut decl_locals = String::new();
    if mutable {
        // Imperative rows need a mutable place: a `varia` local, or an `in`
        // parameter for a receiver with no local constructor.
        if !dtype.decl.is_empty() {
            decl_locals = format!("{}\n", dtype.decl);
        }
        param.clone_from(&dtype.extra_param);
    }
    let args = if override_args.is_empty() {
        row.params
            .iter()
            .map(|p| arg_expr(*p, dtype, "r"))
            .collect()
    } else {
        override_args
    };
    // `limes` is only defined on a shifted view and takes an edge-policy
    // marker (not a receiver dtype) as its generic argument.
    let is_limes = key == "Tensor.limes/0";
    let generic_args = row
        .generic
        .iter()
        .map(|generic| match generic.sort {
            GenericParamSort::Typus if is_limes => "clampa".to_owned(),
            GenericParamSort::Typus => dtype.param.clone(),
            // The tensor probe has width three, so two is an admitted even
            // dimension for the generic RoPE row; a size argument carries
            // the (Latin) `magnitudo` marker.
            GenericParamSort::Magnitudo => "magnitudo 2".to_owned(),
        })
        .collect::<Vec<_>>();
    let generic_suffix = if generic_args.is_empty() {
        String::new()
    } else {
        format!("<{}>", generic_args.join(", "))
    };
    // The `match ∪ nihil` binding is asserted non-null at the call.
    let receiver = if row.receiver == IntrinsicReceiver::Match {
        "r!"
    } else {
        "r"
    };
    let receiver = if is_limes { "r.disloco([1])" } else { receiver };
    let call = format!("{receiver}.{}{}({})", row.faber_name, generic_suffix, args.join(", "));
    let body = if row.return_kind == IntrinsicReturnKind::Vacuum {
        format!("    {call}")
    } else if key == "Tensor.gather/1" {
        // Gather's shape-transforming result cannot be inferred from the
        // `_` binding; pin the result shape like the corpus exemplar.
        format!("    sit {sugar}[4, 3] ex ← {call}")
    } else if row.failable {
        // A failable read (`apud`, need; unit sb272) reports on `⇥`
        // and needs a `⊥` default, a `fac`/`cape` handler or an enclosing `⇥`:
        // the probe supplies a handler so it meets method admission, not
        // SEM011 failable_call_requires_handler.
        format!("    fac {{\n        sit _ ex ← {call}\n    }} cape err {{\n    }}")
    } else {
        format!("    sit _ ex ← {call}")
    };
    let mut src = String::new();
    let _ = writeln!(src, "functio proba({param}) → vacuum {{");
    if !decl_locals.is_empty() {
        let _ = write!(src, "    {decl_locals}");
    }
    if !locals.is_empty() {
        let _ = writeln!(src, "    {locals}");
    }
    let _ = writeln!(src, "{body}");
    let _ = writeln!(src, "}}");
    let _ = writeln!(src);
    let _ = writeln!(src, "incipit {{");
    let _ = writeln!(src, "    nota 1");
    let _ = writeln!(src, "}}");
    src
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProbeOutcome {
    /// The call typechecked and lowered through the module MIR lowerer.
    Lowered,
    /// Semantic analysis rejected the call on this dtype (not admitted here).
    NotAdmitted,
    /// The call reached MIR lowering and fell through the method dispatch
    /// chain to the unsupported-method diagnostic.
    MethodFallthrough,
    /// The dispatch chain resolved an owner and emitted MIR; a deeper
    /// lowering stage rejected the emitted shape (deliberate ceilings like
    /// the lista scan op). Not a registry→owner gap.
    OwnedDeeper(String),
    /// Harness defect: the probe source failed for reasons unrelated to
    /// method admission. Fail loudly so the generator is fixed, never skipped.
    Unexpected(String),
}

fn probe(source: &str, session: &Session) -> ProbeOutcome {
    let analysis = analyze_source(session, "bijection-probe.fab", source, AnalyzeRequest::default());
    let analysis = match analysis {
        Ok(analysis) => analysis,
        Err(diagnostics) => {
            let text: Vec<String> = diagnostics
                .iter()
                .map(|d| {
                    let issues: Vec<String> = d.args.iter().map(|a| a.value.clone()).collect();
                    format!("{} [{}]", d.message, issues.join(","))
                })
                .collect();
            let joined = text.join("; ");
            // Admission rejections talk about the method; anything else is a
            // generator defect.
            let mentions_method = diagnostics.iter().any(|d| {
                let lower = d.message.to_lowercase();
                lower.contains("method")
                    || lower.contains("intrinsic")
                    || d.args.iter().any(|a| {
                        a.value.contains("method")
                            || a.value.contains("intrinsic")
                            || a.value.contains("bitwise")
                            // K5: the float-only lifted math rows reject integer
                            // tensor dtypes at the element gate.
                            || a.value == "tensor_math_float_element_required"
                    })
            });
            if mentions_method {
                return ProbeOutcome::NotAdmitted;
            }
            return ProbeOutcome::Unexpected(joined);
        }
    };

    match crate::mir::lower_analyzed_unit(analysis.application_view(), MirLoweringOptions::default()) {
        Ok(_) => ProbeOutcome::Lowered,
        Err(errors) => {
            let issues = errors.iter().map(|e| e.issue.as_str()).collect::<Vec<_>>();
            let joined = issues.join("; ");
            if errors.iter().any(|e| e.issue.contains("method_call")) {
                // Both fallthrough shapes: the dispatch-chain miss
                // ("method call before runtime/provider MIR lowering") and the
                // generic expression miss ("method calls before receiver-aware
                // MIR lowering").
                ProbeOutcome::MethodFallthrough
            } else {
                // The dispatch chain resolved an owner and emitted MIR; a
                // deeper unsupported surface (e.g. the lista scan op,
                // `collection cumulata before array MIR lowering`) is a
                // different lane's design state, not a registry→owner gap.
                ProbeOutcome::OwnedDeeper(joined)
            }
        }
    }
}

/// The unit-cited ratchet allowlist. Law: this set plus the permanent
/// AIR-lane class list (`AIR_LANE_CLASSES`) EQUALS the probe-derived gap set
/// (direction 1 ∪ direction 3). Each row cites the unit that removes it — MC2
/// armed the numeric-unary family, MC3 armed `Vector.crea`. The 11 AIR-lane
/// rows were dispositioned by MC4 onto `AIR_LANE_CLASSES` (ruled design
/// surface, not worklist entries). MC3 emptied the ratchet; S0-G9
/// (a7d731ec0) re-grew it by one with a semantic-only Matrix `transpone`
/// admission; SCR-04's `lower_matrix_transpone` unit emptied it again, and
/// 9b076c25-4 emptied it a third time by lowering `Tensor.formata` onto
/// `MirCollectionOp::TensorReshape` (Q-X10) — an empty ratchet is the armed
/// state, not drift.
const RATCHET: &[(&str, &str)] = &[];

/// Permanent AIR-lane class list (goal mir-completeness §1 direction 3) — the
/// 11 DOCUMENT-AS-DESIGNED rows moved off the ratchet by MC4. A design fact,
/// not a worklist (same list-kind as the permanent `MirCollectionOp`
/// allowlist): ruled AIR-lane method surface with no MIR method owner
/// (7fd87619 closed allowlist; 94786e02 intrinsic route). `mappata`/
/// `filtrata` keep their unarmed-admission residual pinned here so the class
/// cannot silently grow.
const AIR_LANE_CLASSES: &[&str] = &[
    "AIR.maxima",
    "AIR.minima",
    // LSR-W0-SEMANTIC: identity-table twin rows with a Lista (host) registry
    // owner but no Tensor row and no AirTensorOp mapping — the device method
    // family adopts them mechanically in a later wave; kernel-lane calls
    // fail closed in AIR lowering today.
    "AIR.argmaxima",
    "AIR.argminima",
    // K5 armed `exponentia`, `logarithmus` and `radix` as registry rows.
    "AIR.activatio_tanh",
    "AIR.summa_axis",
    "AIR.reducta",
    "AIR.cumulata",
    "AIR.mappata",
    "AIR.filtrata",
];

/// Known-armed control per receiver family: if a control probe fails, the
/// harness is broken and every skip in that family is unproven.
const FAMILY_CONTROLS: &[(&str, &str)] = &[
    ("Atomic", "load/0"),
    ("Lista", "longitudo/0"),
    ("Tabula", "pone/2"),
    ("Copia", "habet/1"),
    ("Textus", "longitudo/0"),
    ("Ascii", "cursor/0"),
    ("Queue", "appende/1"),
    ("Stack", "appende/1"),
    ("Numeric", "absolutum/0"),
    ("Fractus", "rotunda/0"),
    ("Tensor", "summa/0"),
    ("Vector", "longitudo/0"),
    ("Matrix", "matmul/1"),
    ("Sparsa", "longitudo/0"),
    ("Intervallum", "longitudo/0"),
    ("Regex", "consentit/1"),
    ("Match", "inventum/0"),
];

/// Registry rows that stay only so the checker can answer a retired call with
/// its named diagnostic instead of an arity error: `(row key, diagnostic)`.
/// `.sorted(key)` (la `.ordinata(key)`) retired in favour of the sort
/// expression (operator ruling O2, need; unit sb245).
const RETIRED_ROWS: &[(&str, &str)] = &[("Lista.ordinata/1", "sorted_key_retired")];

fn run_family_controls(session: &Session) {
    for (family, control) in FAMILY_CONTROLS {
        let (name, arity) = control.split_once('/').expect("control shape name/arity");
        let arity: usize = arity.parse().expect("control arity");
        let row = INTRINSICS
            .iter()
            .find(|row| format!("{:?}", row.receiver) == *family && row.faber_name == name && row.arg_count == arity)
            .unwrap_or_else(|| panic!("control row {family}.{control} missing from registry"));
        let outcomes: Vec<ProbeOutcome> = dtypes_for(row.receiver)
            .iter()
            .flat_map(|d| [false, true].map(|mutable| probe(&probe_source(row, d, mutable), session)))
            .collect();
        assert!(
            outcomes
                .iter()
                .any(|o| matches!(o, ProbeOutcome::Lowered | ProbeOutcome::OwnedDeeper(_))),
            "family control {family}.{control} did not lower — probe harness is broken for the {family} family (outcomes: {outcomes:?})"
        );
    }
}

/// Direction 1: sweep every registry row × admitted receiver dtype through the
/// real module MIR lowerer; return the set of rows that hit the
/// unsupported-method fallthrough on at least one admitted dtype.
fn probe_registry_gaps(session: &Session) -> BTreeSet<String> {
    let mut gaps = BTreeSet::new();
    let mut lowered_rows = 0usize;
    for row in INTRINSICS {
        let mut any_admitted = false;
        let mut any_fallthrough = false;
        for dtype in dtypes_for(row.receiver) {
            // Immutable first; the mutable receiver (`in`) is only probed when
            // the immutable one was not admitted (imperative rows require a
            // mutable receiver, value rows do not).
            for mutable in [false, true] {
                match probe(&probe_source(row, &dtype, mutable), session) {
                    ProbeOutcome::Lowered | ProbeOutcome::OwnedDeeper(_) => {
                        any_admitted = true;
                        break;
                    }
                    ProbeOutcome::NotAdmitted => {}
                    ProbeOutcome::MethodFallthrough => {
                        any_fallthrough = true;
                        break;
                    }
                    // A registry row kept only so the checker answers with a
                    // named retirement diagnostic is owner-resolved by that
                    // diagnostic: the probe meets the retirement, not a gap.
                    ProbeOutcome::Unexpected(why)
                        if RETIRED_ROWS
                            .iter()
                            .any(|(key, code)| *key == row_key(row) && why.contains(code)) =>
                    {
                        any_admitted = true;
                        break;
                    }
                    ProbeOutcome::Unexpected(why) => panic!(
                        "probe harness defect at {} on {} (mutable={mutable}): {why}\nsource:\n{}",
                        row_key(row),
                        dtype.name,
                        probe_source(row, &dtype, mutable)
                    ),
                }
            }
        }
        if any_fallthrough {
            gaps.insert(row_key(row));
        } else if any_admitted {
            lowered_rows += 1;
        } else {
            // A row with neither a green probe nor a fallthrough was never
            // admitted on any swept dtype — either the row is dead surface or
            // the generator is wrong for it. Both need eyes; fail loudly.
            let sources: String = dtypes_for(row.receiver)
                .iter()
                .flat_map(|d| [probe_source(row, d, false), probe_source(row, d, true)])
                .collect::<Vec<_>>()
                .join("\n---\n");
            panic!(
                "row {} never admitted on any swept dtype — classify it:\n{sources}",
                row_key(row)
            );
        }
    }
    // Every registry row is either owner-resolved or a gap — no row may
    // vanish from the probe surface entirely.
    assert_eq!(
        lowered_rows + gaps.len(),
        INTRINSICS.len(),
        "probe surface must partition the registry (lowered + gaps == rows)"
    );
    gaps
}

#[cfg(feature = "device")]
fn probe_air_gaps(registry_gaps: &BTreeSet<String>) -> BTreeSet<String> {
    // Candidate AIR-lane tensor method vocabulary: the radix-semantic
    // admission row sets (`is_allowed_tensor_intrinsic` ∪
    // `is_allowed_device_method`), derived structurally through their
    // `#[doc(hidden)]` accessors. No hand-assembled mirror: allowlist growth
    // in radix-semantic flows straight into direction 3 and must be
    // consciously allowlisted or registry-backed.
    let mut candidates: BTreeSet<&str> = BTreeSet::new();
    candidates.extend(radix_semantic::passes::air_tensor_intrinsics::allowed_tensor_intrinsics());
    candidates.extend(radix_semantic::passes::fragment_definition::allowed_device_methods());
    let mut gaps = BTreeSet::new();
    for name in candidates {
        // Armed := a registry Tensor row for the name whose direction-1 probe
        // is green (collection op, dedicated dispatch like `lower_tensor_silu`,
        // or the AIR-lane reducta/cumulata dispatch which has no Tensor
        // registry row at all).
        let tensor_row_green = (0..=4).any(|arity| {
            lookup_method(IntrinsicReceiver::Tensor, name, arity)
                .is_some_and(|row| !registry_gaps.contains(&row_key(row)))
        });
        if !tensor_row_green {
            gaps.insert(format!("AIR.{name}"));
        }
    }
    gaps
}

#[cfg(feature = "device")]
#[test]
fn bijection_ratchet_equals_probe_derived_gap_set() {
    let session = Session::new(Config::default().with_latin_locale());
    run_family_controls(&session);
    let registry_gaps = probe_registry_gaps(&session);

    let mut derived: BTreeSet<String> = registry_gaps.clone();
    #[cfg(feature = "device")]
    {
        derived.extend(probe_air_gaps(&registry_gaps));
    }

    // Coverage law: every probe-derived gap is covered by exactly one of the
    // unit-cited ratchet (a worklist entry that must shrink to empty) or the
    // permanent AIR-lane class list (a design fact). Set equality holds both
    // ways: a new gap turns red until consciously covered; a stale entry
    // after its unit lands turns red until removed.
    let ratchet: BTreeSet<&str> = RATCHET.iter().map(|(key, _)| *key).collect();
    let air_classes: BTreeSet<&str> = AIR_LANE_CLASSES.iter().copied().collect();
    assert!(
        ratchet.is_disjoint(&air_classes),
        "ratchet and AIR-lane class list must be disjoint — a row is either a worklist entry or a design fact, never both"
    );
    let expected: BTreeSet<&str> = ratchet.union(&air_classes).copied().collect();
    let derived_ref: BTreeSet<&str> = derived.iter().map(std::string::String::as_str).collect();

    let mut report = String::new();
    if derived_ref != expected {
        let mut extra: Vec<&str> = derived_ref.difference(&expected).copied().collect();
        let mut missing: Vec<&str> = expected.difference(&derived_ref).copied().collect();
        extra.sort_unstable();
        missing.sort_unstable();
        let _ = write!(
            report,
            "ratchet != probe-derived gap set\n  new gaps (cite a unit in RATCHET or classify onto AIR_LANE_CLASSES): {extra:?}\n  stale rows (unit landed — remove from RATCHET or AIR_LANE_CLASSES): {missing:?}"
        );
    }
    assert!(derived_ref == expected, "{report}");

    // Every ratchet row carries a unit citation (the banned-pattern budget
    // shape: explicit names + citations, never a bare count).
    for (key, unit) in RATCHET {
        assert!(!unit.is_empty(), "ratchet row {key} missing its unit citation");
    }
}

/// Direction 2: every `MirCollectionOp` variant is registry-reachable,
/// AIR/statement-path reachable, or on the permanent internal allowlist.
///
/// The permanent lists are design facts (goal mir-completeness §1): the four
/// array operator variants and `IntervalMaterialize` are conversion/operator
/// surfaces; the six device-internal tensors are target-lane vocabulary. The
/// AIR statement path (`reducta via maxima ex … apud [i]` etc.) lowers via `HirReductionOp`
/// and the runner twins `TensorMax`/`TensorMin`/`TensorSumAxis`.
#[test]
fn every_mir_collection_op_variant_is_reachable_or_permanently_allowed() {
    // Typed enumeration (keeps the list compile-checked against the enum).
    let mut all: Vec<MirCollectionOp> = vec![
        MirCollectionOp::ArrayAdd,
        MirCollectionOp::ArraySub,
        MirCollectionOp::ArrayMul,
        MirCollectionOp::ArrayDiv,
        MirCollectionOp::Append,
        MirCollectionOp::AppendImmutable,
        MirCollectionOp::Put,
        MirCollectionOp::Delete,
        MirCollectionOp::Index,
        MirCollectionOp::IsEmpty,
        MirCollectionOp::Length,
        MirCollectionOp::Contains,
        MirCollectionOp::First,
        MirCollectionOp::Last,
        MirCollectionOp::RemoveFirst,
        MirCollectionOp::RemoveLast,
        MirCollectionOp::Reverse,
        MirCollectionOp::ReverseInPlace,
        MirCollectionOp::Sort,
        MirCollectionOp::Union,
        MirCollectionOp::Intersection,
        MirCollectionOp::Difference,
        MirCollectionOp::SymmetricDifference,
        MirCollectionOp::IsSubset,
        MirCollectionOp::IsSuperset,
        MirCollectionOp::Slice,
        MirCollectionOp::Take,
        MirCollectionOp::TakeLast,
        MirCollectionOp::DropFirst,
        MirCollectionOp::StartsWith,
        MirCollectionOp::EndsWith,
        MirCollectionOp::Uppercase,
        MirCollectionOp::Lowercase,
        MirCollectionOp::Trim,
        MirCollectionOp::Split,
        MirCollectionOp::Replace,
        MirCollectionOp::Absolute,
        MirCollectionOp::Signum,
        MirCollectionOp::Minimum,
        MirCollectionOp::Maximum,
        MirCollectionOp::Approximate,
        MirCollectionOp::Sum,
        // LSR-W0-SEMANTIC: the lista method twins — reachable through the
        // method registry probe surface (`.argmax()` / `.argmin()`).
        MirCollectionOp::ArgMax,
        MirCollectionOp::ArgMin,
        MirCollectionOp::TensorSumAxis,
        MirCollectionOp::Round,
        MirCollectionOp::Floor,
        MirCollectionOp::Ceiling,
        MirCollectionOp::Truncate,
        MirCollectionOp::TensorShape,
        // Q-X1: the named rank and norm methods (`rank`, `norm`).
        MirCollectionOp::TensorRank,
        MirCollectionOp::TensorNorm,
        MirCollectionOp::TensorReshape,
        MirCollectionOp::TensorGet,
        MirCollectionOp::TensorSet,
        MirCollectionOp::TensorFill,
        MirCollectionOp::TensorCreate,
        MirCollectionOp::TensorFromFlat,
        MirCollectionOp::TensorFlatten,
        MirCollectionOp::TensorMaterialize,
        MirCollectionOp::TensorSlice,
        // Bracket-range syntax surface (t[1..2, 0..3]): lowers from HIR slice
        // expressions, not a method row.
        MirCollectionOp::TensorSliceNd,
        // K6 zero-copy views: registry rows `expande` and `disloco`, probed
        // green by the direction-1 sweep (reachable, not permanent).
        MirCollectionOp::TensorExpand,
        MirCollectionOp::TensorShift,
        // `target ⇇ value`: a statement surface lowered from HIR `CopyInto`,
        // not a method row (permanent allowlist below).
        MirCollectionOp::TensorCopyInto,
        MirCollectionOp::TensorAdd,
        MirCollectionOp::TensorSub,
        MirCollectionOp::TensorMul,
        MirCollectionOp::TensorMatMul,
        MirCollectionOp::TensorMean,
        MirCollectionOp::TensorMax,
        MirCollectionOp::TensorMin,
        MirCollectionOp::TensorLayerNorm,
        MirCollectionOp::TensorRmsNorm,
        MirCollectionOp::TensorRope,
        MirCollectionOp::TensorRopeHalf,
        MirCollectionOp::TensorGather,
        MirCollectionOp::TensorCausalMaskedSoftmax,
        MirCollectionOp::TensorQuantizedMatMul,
        MirCollectionOp::TensorGroupedMatMul,
        MirCollectionOp::TensorBatchedAttention,
        MirCollectionOp::TensorSsmConv1d,
        MirCollectionOp::TensorSsmScan,
        MirCollectionOp::TensorDiv,
        MirCollectionOp::TensorNeg,
        MirCollectionOp::TensorGelu,
        MirCollectionOp::TensorRelu,
        MirCollectionOp::TensorSoftmax,
        MirCollectionOp::TensorTranspose,
        MirCollectionOp::TensorCruxEntropia,
        MirCollectionOp::SparseNonzeroCount,
        MirCollectionOp::SparseDensify,
        MirCollectionOp::SparseGet,
        MirCollectionOp::SparseSet,
        MirCollectionOp::IntervalIntersect,
        MirCollectionOp::IntervalUnion,
        MirCollectionOp::IntervalLength,
        MirCollectionOp::IntervalContains,
        MirCollectionOp::IntervalClamp,
        MirCollectionOp::IntervalMaterialize,
        MirCollectionOp::Keys,
        MirCollectionOp::Values,
        MirCollectionOp::RegexMatches,
        MirCollectionOp::RegexFind,
        MirCollectionOp::RegexFindAll,
        MirCollectionOp::RegexSplit,
        MirCollectionOp::RegexReplace,
        MirCollectionOp::TextEscape,
        MirCollectionOp::RegexReplaceEach,
        MirCollectionOp::MatchText,
        MirCollectionOp::MatchStart,
        MirCollectionOp::MatchEnd,
        MirCollectionOp::MatchGroup,
        MirCollectionOp::MatchNamed,
        MirCollectionOp::CordCursor,
    ];
    // K5: one payload variant per lifted math function, all registry rows.
    all.extend(radix_types::MirMathFunction::ALL.map(MirCollectionOp::TensorMath));
    // A2-LIFT G01: the same functions as scalar float methods (not `Abs`,
    // whose scalar form is the numeric `Absolute` row).
    all.extend(radix_types::MirMathFunction::SCALAR.map(MirCollectionOp::ScalarMath));

    // Permanent internal allowlist — a design fact, not a worklist.
    let permanent: &[MirCollectionOp] = &[
        // Operator surface (lista glyph arithmetic → array kernels).
        MirCollectionOp::ArrayAdd,
        MirCollectionOp::ArraySub,
        MirCollectionOp::ArrayMul,
        MirCollectionOp::ArrayDiv,
        // Conversion surface (intervallum → tensor materialization).
        MirCollectionOp::IntervalMaterialize,
        // Statement surface: `target ⇇ value` lowers from HIR `CopyInto`.
        MirCollectionOp::TensorCopyInto,
        // Bracket-range syntax surface (t[1..2, 0..3]): lowers from HIR slice
        // expressions, not a method row.
        MirCollectionOp::TensorSliceNd,
        // Lowering composite: the closure form of regex `muta` maps its closure
        // over the matches in a lowered loop, then joins the results (RX-5);
        // the registry row of `muta` names the literal op, not this one.
        MirCollectionOp::RegexReplaceEach,
        // Constructor + conversion surface (tensor-conversions C3): the
        // type-expression constructor dispatches its `crea`-shaped call
        // without a registry row, and the `lista ↦ tensor` rows emit the op
        // directly — the retired Tensor receiver rows no longer carry them.
        MirCollectionOp::TensorCreate,
        MirCollectionOp::TensorFromFlat,
        // Internal device vocabulary (target-lane owned).
        MirCollectionOp::TensorQuantizedMatMul,
        MirCollectionOp::TensorGroupedMatMul,
        MirCollectionOp::TensorBatchedAttention,
        MirCollectionOp::TensorSsmConv1d,
        MirCollectionOp::TensorSsmScan,
    ];
    // AIR/statement-path reachable: statement heads (`reducta via maxima ex … apud [i]`,
    // `reducta via summa ex … apud [k]`) and device fragments lower through
    // `HirReductionOp`/runner twins, not the method allowlist.
    let air_statement_path: &[MirCollectionOp] = &[
        MirCollectionOp::TensorMax,
        MirCollectionOp::TensorMin,
        MirCollectionOp::TensorSumAxis,
    ];

    let registry_ops: Vec<MirCollectionOp> = INTRINSICS.iter().filter_map(mir_collection_op).collect();
    // Mirror guard: every registry-produced op is a real, listed variant.
    for op in &registry_ops {
        assert!(all.contains(op), "registry produces unlisted variant {op:?}");
    }

    for op in &all {
        let reachable = registry_ops.contains(op) || permanent.contains(op) || air_statement_path.contains(op);
        assert!(
            reachable,
            "MirCollectionOp::{op:?} has no admission path (not registry-reachable, not AIR statement-path, not permanent)"
        );
    }
}
