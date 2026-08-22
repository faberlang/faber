//! Canonical compiler-owned receiver intrinsic catalog.
//!
//! Each row is the single source of truth for Faber spellings, arity, semantic
//! return/param shapes, and MIR classification. Backends keep target-specific
//! lowering adapters; they must not grow parallel string tables for promoted
//! intrinsics.

use radix_types::{MirAtomicOp, MirCollectionOp};

/// Compiler-owned intrinsic receiver families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntrinsicReceiver {
    Lista,
    Tabula,
    Copia,
    Textus,
    /// Bare `ascii` cord. Owns `cursor` only — do not route through Textus.
    Ascii,
    /// Shared `numerus` and `fractus` surface (`absolutum`, `signum`, ...).
    Numeric,
    /// `fractus`-only rounding helpers.
    Fractus,
    /// Indexed `tensor<T, Figura>` container (any element type; arithmetic gated separately).
    Tensor,
    /// Register-class `matrix<T, [R, C]>` value (numeric elements).
    Matrix,
    /// Sparse `sparsa<T, Figura>` container (numeric-only in v1).
    Sparsa,
    /// Register-class `vector<T, N>` (numeric elements).
    Vector,
    /// Numeric `intervallum<T>` interval.
    Intervallum,
    /// Storage-sensitive `atomic<T>` cell operations.
    Atomic,
    /// `regex` pattern carrier (compile-time literal; `consentit` match verb).
    Regex,
}

/// Parameter slot shape relative to the receiver's element/key/value types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntrinsicParamKind {
    Inner,
    Key,
    Value,
    NumerusIndex,
    Textus,
    SetInner,
    Predicate,
    Mapper,
    Reducer,
    Accumulator,
    /// `lista<numerus>` shape or index vectors for tensor methods.
    NumerusArray,
    /// `lista<T>` where `T` is the receiver element type (tensor flat data).
    ElementArray,
    /// Another value of the same receiver type (`intervallum` algebra operands).
    ReceiverSelf,
    /// `tensor<same element, fresh shape>` peer operand for broadcast elementwise
    /// arithmetic. The element is strict-unified against the receiver element; the
    /// shape is left unconstrained so broadcast can form the result shape.
    TensorPeer,
    /// `tensor<same element, fresh shape>` peer operand for rank-2 `matmul`.
    /// The element is strict-unified against the receiver element; the shape is
    /// constrained by contraction semantics (not broadcast) in the typecheck path.
    TensorMatmulPeer,
    /// Atomic operation value with the same element type as the receiver cell.
    AtomicElement,
}

/// Return-type shape resolved against receiver element/key/value context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntrinsicReturnKind {
    Vacuum,
    Bool,
    Numerus,
    Textus,
    Inner,
    NumericInner,
    OptionInner,
    OptionValue,
    ArrayInner,
    ArrayKey,
    ArrayValue,
    SetInner,
    SameAsReceiver,
    /// `receiver ∪ nihil` for fallible interval algebra (`inter`, `union`).
    OptionReceiver,
    NumerusFromFractus,
    /// `fractus<f64>` for `numerus` elements, else the matching `fractus<W>` width.
    /// Used by tensor `media` to dodge the integer-division trap.
    FractusFromElement,
    /// `tensor<same element, same shape>` for sparsa `densata` materialization.
    DenseFromSparsa,
    /// `tensor<same element, [len(ids), last-of-receiver]>` row-gather result
    /// (the CCI-1 `gather` source contract). Resolved to a fresh-shape tensor
    /// in the typecheck path; the `[T, last-of-receiver]` shape fact is
    /// carried by the HIR→AIR `validate_gather_contract` static validation and
    /// the MIR `TensorGather` op (shape-transforming — not
    /// `TensorGet`/`TensorSlice`).
    GatherShape,
    MapperArray,
    ReducerInfer,
    /// `lista` of prefix results for a serial higher-order scan.
    ReducerArray,
    TextusArray,
    /// `cursor<ascii<1>>` — the `ascii.cursor()` / later `textus.cursor()` walk.
    CursorAsciiOne,
}

/// How MIR should treat a promoted intrinsic, when applicable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntrinsicMirKind {
    Collection(MirCollectionOp),
    Atomic(MirAtomicOp),
    HigherOrderFiltrata,
    HigherOrderMap,
    HigherOrderReduce,
    /// Base-lane target-codegen scan; MIR has no lista scan op yet.
    HigherOrderCumulata,
    /// Lowered directly in target codegen without a MIR collection op.
    CodegenOnly,
}

/// One compiler-owned intrinsic method row.
#[derive(Debug, Clone, Copy)]
pub struct MethodIntrinsic {
    pub receiver: IntrinsicReceiver,
    pub faber_name: &'static str,
    pub arg_count: usize,
    pub params: &'static [IntrinsicParamKind],
    pub return_kind: IntrinsicReturnKind,
    pub mir_kind: IntrinsicMirKind,
}

macro_rules! intrinsic {
    ($receiver:ident, $name:expr, $args:expr, $params:expr, $ret:ident, collection $op:ident) => {
        MethodIntrinsic {
            receiver: IntrinsicReceiver::$receiver,
            faber_name: $name,
            arg_count: $args,
            params: $params,
            return_kind: IntrinsicReturnKind::$ret,
            mir_kind: IntrinsicMirKind::Collection(MirCollectionOp::$op),
        }
    };
    ($receiver:ident, $name:expr, $args:expr, $params:expr, $ret:ident, atomic $op:ident) => {
        MethodIntrinsic {
            receiver: IntrinsicReceiver::$receiver,
            faber_name: $name,
            arg_count: $args,
            params: $params,
            return_kind: IntrinsicReturnKind::$ret,
            mir_kind: IntrinsicMirKind::Atomic(MirAtomicOp::$op),
        }
    };
    ($receiver:ident, $name:expr, $args:expr, $params:expr, $ret:ident, $mir:ident) => {
        MethodIntrinsic {
            receiver: IntrinsicReceiver::$receiver,
            faber_name: $name,
            arg_count: $args,
            params: $params,
            return_kind: IntrinsicReturnKind::$ret,
            mir_kind: IntrinsicMirKind::$mir,
        }
    };
}

/// Full promoted intrinsic catalog. Keep rows sorted by receiver, then name.
pub static INTRINSICS: &[MethodIntrinsic] = &[
    // atomic
    intrinsic!(
        Atomic,
        "compare_exchange",
        2,
        &[IntrinsicParamKind::AtomicElement, IntrinsicParamKind::AtomicElement],
        Bool,
        atomic CompareExchange
    ),
    intrinsic!(Atomic, "exchange", 1, &[IntrinsicParamKind::AtomicElement], Inner, atomic Exchange),
    intrinsic!(Atomic, "load", 0, &[], Inner, atomic Load),
    intrinsic!(Atomic, "store", 1, &[IntrinsicParamKind::AtomicElement], Vacuum, atomic Store),
    // lista
    intrinsic!(Lista, "accipe", 1, &[IntrinsicParamKind::NumerusIndex], OptionInner, collection Index),
    intrinsic!(Lista, "addita", 1, &[IntrinsicParamKind::Inner], ArrayInner, collection AppendImmutable),
    intrinsic!(Lista, "appende", 1, &[IntrinsicParamKind::Inner], Vacuum, collection Append),
    intrinsic!(Lista, "continet", 1, &[IntrinsicParamKind::Inner], Bool, collection Contains),
    intrinsic!(
        Lista,
        "cumulata",
        2,
        &[IntrinsicParamKind::Reducer, IntrinsicParamKind::Accumulator],
        ReducerArray,
        HigherOrderCumulata
    ),
    intrinsic!(Lista, "decapita", 0, &[], OptionInner, collection RemoveFirst),
    intrinsic!(
        Lista,
        "filtrata",
        1,
        &[IntrinsicParamKind::Predicate],
        ArrayInner,
        HigherOrderFiltrata
    ),
    intrinsic!(Lista, "inversa", 0, &[], ArrayInner, collection Reverse),
    intrinsic!(Lista, "inverte", 0, &[], Vacuum, collection ReverseInPlace),
    intrinsic!(Lista, "longitudo", 0, &[], Numerus, collection Length),
    intrinsic!(
        Lista,
        "mappata",
        1,
        &[IntrinsicParamKind::Mapper],
        MapperArray,
        HigherOrderMap
    ),
    intrinsic!(Lista, "omissa", 1, &[IntrinsicParamKind::NumerusIndex], ArrayInner, collection DropFirst),
    intrinsic!(Lista, "ordina", 0, &[], Vacuum, CodegenOnly),
    intrinsic!(Lista, "ordinata", 0, &[], ArrayInner, collection Sort),
    intrinsic!(Lista, "prima", 1, &[IntrinsicParamKind::NumerusIndex], ArrayInner, collection Take),
    intrinsic!(Lista, "primus", 0, &[], OptionInner, collection First),
    intrinsic!(
        Lista,
        "reducta",
        2,
        &[IntrinsicParamKind::Reducer, IntrinsicParamKind::Accumulator],
        ReducerInfer,
        HigherOrderReduce
    ),
    intrinsic!(Lista, "detrahe", 0, &[], OptionInner, collection RemoveLast),
    intrinsic!(
        Lista,
        "sectio",
        2,
        &[IntrinsicParamKind::NumerusIndex, IntrinsicParamKind::NumerusIndex],
        ArrayInner,
        collection Slice
    ),
    intrinsic!(Lista, "summa", 0, &[], NumericInner, collection Sum),
    intrinsic!(Lista, "ultima", 1, &[IntrinsicParamKind::NumerusIndex], ArrayInner, collection TakeLast),
    intrinsic!(Lista, "ultimus", 0, &[], OptionInner, collection Last),
    intrinsic!(Lista, "vacua", 0, &[], Bool, collection IsEmpty),
    // tabula
    intrinsic!(Tabula, "accipe", 1, &[IntrinsicParamKind::Key], OptionValue, collection Index),
    intrinsic!(Tabula, "claves", 0, &[], ArrayKey, collection Keys),
    intrinsic!(Tabula, "dele", 1, &[IntrinsicParamKind::Key], Bool, collection Delete),
    intrinsic!(Tabula, "habet", 1, &[IntrinsicParamKind::Key], Bool, collection Contains),
    intrinsic!(Tabula, "longitudo", 0, &[], Numerus, collection Length),
    intrinsic!(
        Tabula,
        "pone",
        2,
        &[IntrinsicParamKind::Key, IntrinsicParamKind::Value],
        Vacuum,
        collection Put
    ),
    intrinsic!(Tabula, "vacua", 0, &[], Bool, collection IsEmpty),
    intrinsic!(Tabula, "valores", 0, &[], ArrayValue, collection Values),
    // copia
    intrinsic!(Copia, "adde", 1, &[IntrinsicParamKind::Inner], Vacuum, collection Append),
    intrinsic!(Copia, "dele", 1, &[IntrinsicParamKind::Inner], Bool, collection Delete),
    intrinsic!(Copia, "differentia", 1, &[IntrinsicParamKind::SetInner], SetInner, collection Difference),
    intrinsic!(Copia, "habet", 1, &[IntrinsicParamKind::Inner], Bool, collection Contains),
    intrinsic!(Copia, "intersectio", 1, &[IntrinsicParamKind::SetInner], SetInner, collection Intersection),
    intrinsic!(Copia, "longitudo", 0, &[], Numerus, collection Length),
    intrinsic!(Copia, "subcopia", 1, &[IntrinsicParamKind::SetInner], Bool, collection IsSubset),
    intrinsic!(Copia, "supercopia", 1, &[IntrinsicParamKind::SetInner], Bool, collection IsSuperset),
    intrinsic!(Copia, "symmetrica", 1, &[IntrinsicParamKind::SetInner], SetInner, collection SymmetricDifference),
    intrinsic!(Copia, "unio", 1, &[IntrinsicParamKind::SetInner], SetInner, collection Union),
    intrinsic!(Copia, "vacua", 0, &[], Bool, collection IsEmpty),
    // ascii
    intrinsic!(Ascii, "cursor", 0, &[], CursorAsciiOne, collection CordCursor),
    // textus
    intrinsic!(Textus, "accipe", 1, &[IntrinsicParamKind::NumerusIndex], Textus, collection Index),
    intrinsic!(Textus, "continet", 1, &[IntrinsicParamKind::Textus], Bool, collection Contains),
    intrinsic!(Textus, "cursor", 0, &[], CursorAsciiOne, collection CordCursor),
    intrinsic!(Textus, "divide", 1, &[IntrinsicParamKind::Textus], TextusArray, collection Split),
    intrinsic!(Textus, "finis", 1, &[IntrinsicParamKind::Textus], Bool, collection EndsWith),
    intrinsic!(Textus, "initium", 1, &[IntrinsicParamKind::Textus], Bool, collection StartsWith),
    intrinsic!(Textus, "longitudo", 0, &[], Numerus, collection Length),
    intrinsic!(Textus, "maiuscula", 0, &[], Textus, collection Uppercase),
    intrinsic!(Textus, "minuscula", 0, &[], Textus, collection Lowercase),
    intrinsic!(Textus, "muta", 2, &[IntrinsicParamKind::Textus, IntrinsicParamKind::Textus], Textus, collection Replace),
    intrinsic!(Textus, "recide", 0, &[], Textus, collection Trim),
    intrinsic!(Textus, "sectio", 2, &[IntrinsicParamKind::NumerusIndex, IntrinsicParamKind::NumerusIndex], Textus, collection Slice),
    intrinsic!(Textus, "vacua", 0, &[], Bool, collection IsEmpty),
    // numeric (numerus + fractus)
    intrinsic!(Numeric, "absolutum", 0, &[], SameAsReceiver, collection Absolute),
    intrinsic!(
        Numeric,
        "addita",
        1,
        &[IntrinsicParamKind::Inner],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "adde",
        1,
        &[IntrinsicParamKind::Inner],
        Vacuum,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "alterna",
        1,
        &[IntrinsicParamKind::Inner],
        Vacuum,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "alternata",
        1,
        &[IntrinsicParamKind::Inner],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(Numeric, "complementa", 0, &[], Vacuum, CodegenOnly),
    intrinsic!(
        Numeric,
        "complementata",
        0,
        &[],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "coniuncta",
        1,
        &[IntrinsicParamKind::Inner],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "coniunge",
        1,
        &[IntrinsicParamKind::Inner],
        Vacuum,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "disiuncta",
        1,
        &[IntrinsicParamKind::Inner],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "disiunge",
        1,
        &[IntrinsicParamKind::Inner],
        Vacuum,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "dextra",
        1,
        &[IntrinsicParamKind::Inner],
        Vacuum,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "dextrata",
        1,
        &[IntrinsicParamKind::Inner],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "divida",
        1,
        &[IntrinsicParamKind::Inner],
        Vacuum,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "divisa",
        1,
        &[IntrinsicParamKind::Inner],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "maior",
        1,
        &[IntrinsicParamKind::Inner],
        Bool,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "maxime",
        1,
        &[IntrinsicParamKind::Inner],
        Bool,
        CodegenOnly
    ),
    intrinsic!(Numeric, "maximus", 1, &[IntrinsicParamKind::Inner], SameAsReceiver, collection Maximum),
    intrinsic!(
        Numeric,
        "minime",
        1,
        &[IntrinsicParamKind::Inner],
        Bool,
        CodegenOnly
    ),
    intrinsic!(Numeric, "minimus", 1, &[IntrinsicParamKind::Inner], SameAsReceiver, collection Minimum),
    intrinsic!(
        Numeric,
        "minor",
        1,
        &[IntrinsicParamKind::Inner],
        Bool,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "modula",
        1,
        &[IntrinsicParamKind::Inner],
        Vacuum,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "modulata",
        1,
        &[IntrinsicParamKind::Inner],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "multiplica",
        1,
        &[IntrinsicParamKind::Inner],
        Vacuum,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "multiplicata",
        1,
        &[IntrinsicParamKind::Inner],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(Numeric, "nega", 0, &[], Vacuum, CodegenOnly),
    intrinsic!(Numeric, "negativa", 0, &[], SameAsReceiver, CodegenOnly),
    intrinsic!(
        Numeric,
        "potentia",
        1,
        &[IntrinsicParamKind::Inner],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(Numeric, "signum", 0, &[], SameAsReceiver, collection Signum),
    intrinsic!(
        Numeric,
        "sinistra",
        1,
        &[IntrinsicParamKind::Inner],
        Vacuum,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "sinistrata",
        1,
        &[IntrinsicParamKind::Inner],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "subtrahe",
        1,
        &[IntrinsicParamKind::Inner],
        Vacuum,
        CodegenOnly
    ),
    intrinsic!(
        Numeric,
        "subtracta",
        1,
        &[IntrinsicParamKind::Inner],
        SameAsReceiver,
        CodegenOnly
    ),
    // fractus-only
    intrinsic!(
        Fractus,
        "approximata",
        2,
        &[IntrinsicParamKind::Inner, IntrinsicParamKind::Inner],
        Bool,
        collection Approximate
    ),
    intrinsic!(Fractus, "pavimentum", 0, &[], NumerusFromFractus, collection Floor),
    intrinsic!(Fractus, "rotunda", 0, &[], NumerusFromFractus, collection Round),
    intrinsic!(Fractus, "tectum", 0, &[], NumerusFromFractus, collection Ceiling),
    intrinsic!(Fractus, "trunca", 0, &[], NumerusFromFractus, collection Truncate),
    intrinsic!(Sparsa, "accipe", 1, &[IntrinsicParamKind::NumerusArray], Inner, collection SparseGet),
    intrinsic!(Sparsa, "densata", 0, &[], DenseFromSparsa, collection SparseDensify),
    intrinsic!(Sparsa, "longitudo", 0, &[], Numerus, collection Length),
    intrinsic!(Sparsa, "magnitudines", 0, &[], ArrayInner, collection TensorShape),
    intrinsic!(Sparsa, "nonnihil", 0, &[], Numerus, collection SparseNonzeroCount),
    intrinsic!(
        Sparsa,
        "ponde",
        2,
        &[IntrinsicParamKind::NumerusArray, IntrinsicParamKind::Inner],
        Vacuum,
        collection SparseSet
    ),
    intrinsic!(Tensor, "longitudo", 0, &[], Numerus, collection Length),
    intrinsic!(Tensor, "magnitudines", 0, &[], ArrayInner, collection TensorShape),
    intrinsic!(
        Tensor,
        "forma",
        1,
        &[IntrinsicParamKind::NumerusArray],
        SameAsReceiver,
        collection TensorReshape
    ),
    intrinsic!(
        Tensor,
        "accipe",
        1,
        &[IntrinsicParamKind::NumerusArray],
        OptionInner,
        collection TensorGet
    ),
    intrinsic!(
        Tensor,
        "addita",
        1,
        &[IntrinsicParamKind::TensorPeer],
        SameAsReceiver,
        collection TensorAdd
    ),
    // S6-C2: the bounded bias-broadcast add. Same tensor-peer surface as the
    // general `addita`, but the result shape is the receiver shape and the
    // explicit rank-extension contract ([B,D] + [D]) is enforced at the
    // HIR→AIR construction site (lower.rs `validate_addita_bias_contract`).
    intrinsic!(
        Tensor,
        "addita_bias",
        1,
        &[IntrinsicParamKind::TensorPeer],
        SameAsReceiver,
        collection TensorAdd
    ),
    intrinsic!(
        Tensor,
        "subtrahe",
        1,
        &[IntrinsicParamKind::TensorPeer],
        SameAsReceiver,
        collection TensorSub
    ),
    intrinsic!(
        Tensor,
        "multiplica",
        1,
        &[IntrinsicParamKind::TensorPeer],
        SameAsReceiver,
        collection TensorMul
    ),
    intrinsic!(
        Tensor,
        "divisio",
        1,
        &[IntrinsicParamKind::TensorPeer],
        SameAsReceiver,
        collection TensorDiv
    ),
    intrinsic!(
        Tensor,
        "nega",
        0,
        &[],
        SameAsReceiver,
        collection TensorNeg
    ),
    intrinsic!(
        Tensor,
        "matmul",
        1,
        &[IntrinsicParamKind::TensorMatmulPeer],
        SameAsReceiver,
        collection TensorMatMul
    ),
    intrinsic!(Tensor, "summa", 0, &[], NumericInner, collection Sum),
    intrinsic!(Tensor, "media", 0, &[], FractusFromElement, collection TensorMean),
    intrinsic!(
        Tensor,
        "ponde",
        2,
        &[IntrinsicParamKind::NumerusArray, IntrinsicParamKind::Inner],
        Vacuum,
        collection TensorSet
    ),
    intrinsic!(Tensor, "reple", 1, &[IntrinsicParamKind::Inner], SameAsReceiver, collection TensorFill),
    intrinsic!(
        Tensor,
        "crea",
        2,
        &[IntrinsicParamKind::Inner, IntrinsicParamKind::NumerusArray],
        SameAsReceiver,
        collection TensorCreate
    ),
    intrinsic!(
        Tensor,
        "strue",
        2,
        &[IntrinsicParamKind::ElementArray, IntrinsicParamKind::NumerusArray],
        SameAsReceiver,
        collection TensorFromFlat
    ),
    intrinsic!(Tensor, "planata", 0, &[], ArrayInner, collection TensorFlatten),
    // CCI-1: the bounded common-case intrinsic rows (RMSNorm, RoPE, row
    // gather). Source spellings/signatures frozen in
    // docs/archived/common-case-intrinsics/common-case-intrinsics-contract.md;
    // typed fail-closed contracts enforced at the HIR→AIR construction site
    // (air/lower.rs + radix-air nodes.rs validate_*_contract).
    intrinsic!(
        Tensor,
        "gather",
        1,
        &[IntrinsicParamKind::NumerusArray],
        GatherShape,
        collection TensorGather
    ),
    intrinsic!(
        Tensor,
        "rms_norm",
        3,
        &[
            IntrinsicParamKind::NumerusIndex,
            IntrinsicParamKind::Inner,
            IntrinsicParamKind::TensorPeer,
        ],
        SameAsReceiver,
        collection TensorRmsNorm
    ),
    intrinsic!(
        Tensor,
        "rope_norm",
        2,
        &[IntrinsicParamKind::NumerusIndex, IntrinsicParamKind::NumerusIndex],
        SameAsReceiver,
        collection TensorRope
    ),
    intrinsic!(
        Tensor,
        "gelu",
        0,
        &[],
        SameAsReceiver,
        collection TensorGelu
    ),
    intrinsic!(
        Tensor,
        "activatio_relu",
        0,
        &[],
        SameAsReceiver,
        collection TensorRelu
    ),
    intrinsic!(
        Tensor,
        "crux_entropia",
        1,
        &[IntrinsicParamKind::TensorPeer],
        NumericInner,
        collection TensorCruxEntropia
    ),
    intrinsic!(
        Tensor,
        "activatio_softmax",
        0,
        &[],
        SameAsReceiver,
        collection TensorSoftmax
    ),
    intrinsic!(
        Tensor,
        "transpone",
        0,
        &[],
        SameAsReceiver,
        collection TensorTranspose
    ),
    intrinsic!(
        Tensor,
        "laminatio",
        2,
        &[IntrinsicParamKind::NumerusIndex, IntrinsicParamKind::Inner],
        SameAsReceiver,
        collection TensorLayerNorm
    ),
    intrinsic!(
        Tensor,
        "laminatio",
        3,
        &[
            IntrinsicParamKind::NumerusIndex,
            IntrinsicParamKind::Inner,
            IntrinsicParamKind::TensorPeer,
        ],
        SameAsReceiver,
        collection TensorLayerNorm
    ),
    intrinsic!(
        Tensor,
        "laminatio",
        4,
        &[
            IntrinsicParamKind::NumerusIndex,
            IntrinsicParamKind::Inner,
            IntrinsicParamKind::TensorPeer,
            IntrinsicParamKind::TensorPeer,
        ],
        SameAsReceiver,
        collection TensorLayerNorm
    ),
    intrinsic!(
        Tensor,
        "materialize",
        0,
        &[],
        SameAsReceiver,
        collection TensorMaterialize
    ),
    intrinsic!(
        Tensor,
        "sectio",
        2,
        &[IntrinsicParamKind::NumerusIndex, IntrinsicParamKind::NumerusIndex],
        SameAsReceiver,
        collection TensorSlice
    ),
    // matrix
    intrinsic!(
        Matrix,
        "addita",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Matrix,
        "subtrahe",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Matrix,
        "multiplica",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Matrix,
        "divida",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Matrix,
        "matmul",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    // `applica` param/return types are specialized in typecheck (vector peer /
    // vector result from matrix rows×columns); registry row only owns spelling.
    intrinsic!(
        Matrix,
        "applica",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    // Square 2×2 / 4×4 affine inverse; singular → nihil (OptionReceiver).
    intrinsic!(Matrix, "inversa", 0, &[], OptionReceiver, CodegenOnly),
    intrinsic!(
        Vector,
        "addita",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Vector,
        "subtrahe",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Vector,
        "multiplica",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Vector,
        "divida",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Vector,
        "productum",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        Inner,
        CodegenOnly
    ),
    intrinsic!(
        Vector,
        "transversum",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(Vector, "longitudo", 0, &[], Inner, CodegenOnly),
    intrinsic!(Vector, "normalizata", 0, &[], SameAsReceiver, CodegenOnly),
    intrinsic!(
        Vector,
        "swizzle",
        1,
        &[IntrinsicParamKind::Textus],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(Vector, "crea", 0, &[], SameAsReceiver, CodegenOnly),
    // intervallum
    intrinsic!(Intervallum, "coercere", 1, &[IntrinsicParamKind::ReceiverSelf], SameAsReceiver, collection IntervalClamp),
    intrinsic!(Intervallum, "continet", 1, &[IntrinsicParamKind::Inner], Bool, collection IntervalContains),
    intrinsic!(
        Intervallum,
        "inter",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        OptionReceiver,
        collection IntervalIntersect
    ),
    intrinsic!(
        Intervallum,
        "union",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        OptionReceiver,
        collection IntervalUnion
    ),
    intrinsic!(Intervallum, "longitudo", 0, &[], Numerus, collection IntervalLength),
    // regex
    // G12: `consentit` (consentire, "to agree/match") — 3rd-person match verb
    // on the `regex` receiver, same pattern as the textus predicates
    // (`continet`/`initium`/`finis`). Spelling recorded in the G12-U1 unit
    // report (Open Q #3 default).
    intrinsic!(Regex, "consentit", 1, &[IntrinsicParamKind::Textus], Bool, collection RegexMatches),
];
