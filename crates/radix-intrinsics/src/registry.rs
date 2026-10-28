//! Canonical compiler-owned receiver intrinsic catalog.
//!
//! Each row is the single source of truth for Faber spellings, arity, semantic
//! return/param shapes, and MIR classification. Backends keep target-specific
//! lowering adapters; they must not grow parallel string tables for promoted
//! intrinsics.

use radix_types::{GenericParamSort, MirAtomicOp, MirCollectionOp, MirMathFunction};

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
    /// Register-class `vector<T, size N>` (numeric elements).
    Vector,
    /// Numeric `intervallum<T>` interval.
    Intervallum,
    /// Storage-sensitive `atomic<T>` cell operations.
    Atomic,
    /// FIFO `queue<T[, N]>` collection.
    Queue,
    /// LIFO `stack<T[, N]>` collection.
    Stack,
    /// `regex` pattern carrier (compile-time literal; `consentit` match verb).
    Regex,
    /// One regex match (RD-3: no source type name; reached only through
    /// `regex.inventa` results). Owns the accessor family.
    Match,
}

/// Parameter slot shape relative to the receiver's element/key/value types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntrinsicParamKind {
    Inner,
    Key,
    Value,
    NumerusIndex,
    Textus,
    /// A `textus` or a one-parameter closure from the receiver's match to
    /// `textus` (RX-5, RD-5): the regex `muta` replacement slot. Typecheck picks
    /// the form by the argument expression; both forms return `textus`.
    TextusOrReplacer,
    SetInner,
    Predicate,
    Mapper,
    Reducer,
    Accumulator,
    /// `lista<numerus>` shape or index vectors for tensor methods.
    NumerusArray,
    /// `lista<T>` where `T` is the receiver element type (tensor flat data).
    ElementArray,
    /// `tensor<u32, [n]>` row-id vector for `gather`: a rank-1 `u32` tensor
    /// whose extent `n` travels with the value (campaign rule 11) and becomes
    /// the result's row count. Never a `lista`: an unbounded collection is not
    /// a kernel buffer (rule 12.3).
    TensorIndexIds,
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
    /// The receiver's map value type `V` (`tabula.apud`: the checked read).
    Value,
    NumericInner,
    /// Element type for floats; the 64-bit cap by the range rule for integers
    /// (d11-6 ruling 21), the same typing as the `summa` construct.
    NumericSum,
    OptionInner,
    OptionValue,
    ArrayInner,
    ArrayKey,
    ArrayValue,
    SetInner,
    SameAsReceiver,
    /// `receiver ∪ nihil` for fallible interval algebra (`inter`, `union`).
    OptionReceiver,
    /// `numerus ∪ nihil` — the argmax/argmin index-or-nihil shape (LSR-W0):
    /// the winning index, or `nihil` for an empty source (never a −1
    /// sentinel). Requires an ordered numeric element.
    OptionNumerus,
    NumerusFromFractus,
    /// `fractus<f64>` for `numerus` elements, else the matching `fractus<W>` width.
    /// Used by tensor `media` to dodge the integer-division trap.
    FractusFromElement,
    /// `tensor<same element, same shape>` for sparsa `densata` materialization.
    DenseFromSparsa,
    /// `tensor<same element, [n, last-of-receiver]>` row-gather result, where
    /// `n` is the extent of the `tensor<u32, [n]>` ids operand. Resolved to a
    /// fresh-shape tensor in the typecheck path and pinned to
    /// `[n, last-of-receiver]` by the tensor shape constraints; the MIR
    /// `TensorGather` op is shape-transforming (not `TensorGet`/`TensorSlice`).
    GatherShape,
    MapperArray,
    /// `set<U>` where `U` is the mapper return type (`copia.mappata`).
    MapperSet,
    ReducerInfer,
    /// `lista` of prefix results for a serial higher-order scan.
    ReducerArray,
    TextusArray,
    /// `match ∪ nihil` — the `regex.inventa` result (RD-3: the match type has
    /// no source name; it is only ever read through this option and inference).
    OptionMatch,
    /// `lista<match>` — the `regex.collecta` result (RD-3: no source name for
    /// the element type).
    MatchArray,
    /// `textus ∪ nihil` — a capture group's text, or `nihil` when the group did
    /// not participate or does not exist.
    OptionTextus,
    /// `cursor<littera>` — the `ascii.cursor()` / `textus.cursor()` scalar walk (D10.1).
    CursorLittera,
    /// The bound type of the first Typus generic (`formata`).
    GenericTypus,
}

/// How MIR should treat a promoted intrinsic, when applicable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntrinsicMirKind {
    Collection(MirCollectionOp),
    Atomic(MirAtomicOp),
    HigherOrderAll,
    HigherOrderAny,
    /// `reperta` (en `find`): the first element, in list order, whose
    /// predicate holds, as `T ∪ nihil`. Stops at the first match like `quilibet`.
    HigherOrderFind,
    /// `extende` (en `extend`): append every element of another list of the
    /// same element type, in order, in place. Lowered as one element-wise
    /// append per argument element; it has no single MIR collection op.
    ListExtend,
    HigherOrderFiltrata,
    HigherOrderMap,
    HigherOrderReduce,
    /// Stable sort by a key selector (D1.7): `ordina(key)` in place,
    /// `ordinata(key)` as a new `lista`. Keys are numbers or `textus`.
    HigherOrderSortBy,
    /// Base-lane target-codegen scan; MIR has no lista scan op yet.
    HigherOrderCumulata,
    /// Lowered directly in target codegen without a MIR collection op.
    CodegenOnly,
    /// Lowered as `MirIntrinsic::TensorLimes` with a resolved edge-policy payload.
    TensorLimes,
}

/// Static `=` default for a catalog generic. Interned at typecheck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogGenericDefault {
    /// `valor` — the sermo view payload default.
    Valor,
}

/// Static catalog generic declaration.
///
/// Catalog rows cannot hold interned [`radix_types::GenericParamDescriptor`]
/// values at static init. Typecheck interns these names into descriptors and
/// then uses the shared generic binder. An empty list on a row means explicit
/// angle-bracket arguments are illegal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CatalogGenericParam {
    pub name: &'static str,
    pub sort: GenericParamSort,
    pub default: Option<CatalogGenericDefault>,
}

impl CatalogGenericParam {
    #[must_use]
    pub const fn typus(name: &'static str) -> Self {
        Self {
            name,
            sort: GenericParamSort::Typus,
            default: None,
        }
    }

    #[must_use]
    pub const fn typus_default_valor(name: &'static str) -> Self {
        Self {
            name,
            sort: GenericParamSort::Typus,
            default: Some(CatalogGenericDefault::Valor),
        }
    }

    #[must_use]
    pub const fn magnitudo(name: &'static str) -> Self {
        Self {
            name,
            sort: GenericParamSort::Magnitudo,
            default: None,
        }
    }
}

/// One compiler-owned intrinsic method row.
#[derive(Debug, Clone, Copy)]
pub struct MethodIntrinsic {
    pub receiver: IntrinsicReceiver,
    pub faber_name: &'static str,
    pub generic: &'static [CatalogGenericParam],
    pub arg_count: usize,
    pub params: &'static [IntrinsicParamKind],
    pub return_kind: IntrinsicReturnKind,
    pub mir_kind: IntrinsicMirKind,
    pub missing_generic_issue: Option<&'static str>,
    pub excess_generic_issue: Option<&'static str>,
    pub value_arity_issue: Option<&'static str>,
    /// The row declares a `⇥ textus` alternate exit: a call needs a handler, a
    /// `⊥` default or an enclosing `⇥` (D17.8), exactly like a user function that
    /// declares `⇥`. The failure value is the runtime failure text.
    pub failable: bool,
    /// A failable row's index-argument position. When a static range proof
    /// shows that argument within the receiver's extents, the call cannot
    /// fail and needs no handler (the "a literal proven to fit cannot fail"
    /// rule of numeric conversion); a provably out-of-range index is a
    /// compile-time `index_provably_out_of_bounds`. `None` on rows that are
    /// not failable or have no provable range.
    pub range_checked_arg: Option<usize>,
    /// A row whose argument at this index is an `axis` marker slot (need
    /// bc775857): the call declares it with the `axis` word, and the word on
    /// any other argument is `axis_marker_unexpected`. `None` on rows with no
    /// axis argument.
    pub axis_arg: Option<usize>,
}

impl MethodIntrinsic {
    #[must_use]
    pub fn generic_arity(&self) -> usize {
        self.generic.len()
    }

    /// Whether this row reads or removes an end of a list: `primus`,
    /// `ultimus`, `decapita`, `detrahe`. On a list length range whose
    /// minimum is one or more (`list<T, size 1…∞>`, need) the list is
    /// never empty, so these return `T` instead of `T ∪ none`; the removals
    /// report a violated minimum on `⇥`.
    #[must_use]
    pub fn reads_list_end(&self) -> bool {
        matches!(
            self.faber_name,
            "primus" | "ultimus" | "decapita" | "detrahe"
        )
    }
}

/// One catalog row with every policy field at its macro default. Arms of
/// [`intrinsic!`] supply only the MIR classification; a row that needs a
/// non-default policy field spells that field explicitly instead.
macro_rules! intrinsic_row {
    ($receiver:ident, $name:expr, $args:expr, $params:expr, $ret:ident, $mir_kind:expr) => {
        MethodIntrinsic {
            receiver: IntrinsicReceiver::$receiver,
            faber_name: $name,
            generic: &[],
            arg_count: $args,
            params: $params,
            return_kind: IntrinsicReturnKind::$ret,
            mir_kind: $mir_kind,
            missing_generic_issue: None,
            excess_generic_issue: None,
            value_arity_issue: None,
            failable: false,
            range_checked_arg: None,
            axis_arg: None,
        }
    };
}

macro_rules! intrinsic {
    ($receiver:ident, $name:expr, $args:expr, $params:expr, $ret:ident, collection $op:ident) => {
        intrinsic_row!(
            $receiver,
            $name,
            $args,
            $params,
            $ret,
            IntrinsicMirKind::Collection(MirCollectionOp::$op)
        )
    };
    ($receiver:ident, $name:expr, $args:expr, $params:expr, $ret:ident, scalar_math $function:ident) => {
        intrinsic_row!(
            $receiver,
            $name,
            $args,
            $params,
            $ret,
            IntrinsicMirKind::Collection(MirCollectionOp::ScalarMath(MirMathFunction::$function))
        )
    };
    ($receiver:ident, $name:expr, $args:expr, $params:expr, $ret:ident, math $function:ident) => {
        intrinsic_row!(
            $receiver,
            $name,
            $args,
            $params,
            $ret,
            IntrinsicMirKind::Collection(MirCollectionOp::TensorMath(MirMathFunction::$function))
        )
    };
    ($receiver:ident, $name:expr, $args:expr, $params:expr, $ret:ident, atomic $op:ident) => {
        intrinsic_row!(
            $receiver,
            $name,
            $args,
            $params,
            $ret,
            IntrinsicMirKind::Atomic(MirAtomicOp::$op)
        )
    };
    ($receiver:ident, $name:expr, $args:expr, $params:expr, $ret:ident, $mir:ident) => {
        intrinsic_row!(
            $receiver,
            $name,
            $args,
            $params,
            $ret,
            IntrinsicMirKind::$mir
        )
    };
}

/// The type-expression constructor's dispatch row. The Tensor receiver row
/// retired (tensor-conversions C3), but the constructor path — check lane,
/// MIR `collection_method_op`, Rust emit — still lowers through
/// `TensorCreate` and needs this entry's param/return contract. Not in
/// [`INTRINSICS`]: the method surface stays retired.
pub static TENSOR_CONSTRUCTOR_ENTRY: MethodIntrinsic = intrinsic!(
    Tensor,
    "crea",
    2,
    &[IntrinsicParamKind::Inner, IntrinsicParamKind::NumerusArray],
    SameAsReceiver,
    collection TensorCreate
);

/// Full promoted intrinsic catalog. Rows stay grouped by receiver; order
/// within a receiver is presentation only, because every lookup keys on
/// `(receiver, name, generic arity, value arity)`.
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
    // The sinking contract's method form: `xs.sink(v)` is one
    // identity with the `⇜` operator twin, and it lowers to the same append.
    intrinsic!(Lista, "immerge", 1, &[IntrinsicParamKind::Inner], Vacuum, collection Append),
    // need: the `at` read (la `apud`). `c[i]` dies, `c.get(i)` is
    // `T ∪ none`, `c.at(i)` reports an out-of-range index / missing key on `⇥`
    // (default form `⊥`). The error payload is the runtime failure text, the
    // same `textus` the tensor `accipe` row declares (enum code: need).
    MethodIntrinsic {
        failable: true,
        ..intrinsic!(Lista, "apud", 1, &[IntrinsicParamKind::NumerusIndex], Inner, collection Index)
    },
    // LSR-W0-SEMANTIC: the argmax/argmin method twins beside the reduction
    // family. First-index tie law (also the parallel-combine law), empty →
    // nihil composing with `vel`/`coalesce`, NaN law pinned to `max from`'s
    // implemented `f64::max` fold — stated once in the `ArgMax`/`ArgMin`
    // `MirCollectionOp` docs and pinned by the owning-crate contract tests.
    intrinsic!(Lista, "argmaxima", 0, &[], OptionNumerus, collection ArgMax),
    intrinsic!(Lista, "argminima", 0, &[], OptionNumerus, collection ArgMin),
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
    // `reperta` (en `find`): predicate search. Its own name and one signature;
    // `primus` (the unconditional first element) is unchanged. The result is
    // `T ∪ nihil` even for a bounded list of minimum size one: a match is not
    // guaranteed.
    intrinsic!(
        Lista,
        "reperta",
        1,
        &[IntrinsicParamKind::Predicate],
        OptionInner,
        HigherOrderFind
    ),
    // `extende` (en `extend`): the list join. One signature: another list of the
    // receiver's element type (a bounded list joins an unbounded one). It
    // mutates the receiver, so the receiver must be `mut`.
    intrinsic!(
        Lista,
        "extende",
        1,
        &[IntrinsicParamKind::ElementArray],
        Vacuum,
        ListExtend
    ),
    intrinsic!(Lista, "inversa", 0, &[], ArrayInner, collection Reverse),
    intrinsic!(Lista, "inverte", 0, &[], Vacuum, collection ReverseInPlace),
    intrinsic!(Lista, "longitudo", 0, &[], Numerus, collection Length),
    intrinsic!(Lista, "maximus", 1, &[IntrinsicParamKind::Inner], SameAsReceiver, collection Maximum),
    intrinsic!(Lista, "minimus", 1, &[IntrinsicParamKind::Inner], SameAsReceiver, collection Minimum),
    intrinsic!(
        Lista,
        "mappata",
        1,
        &[IntrinsicParamKind::Mapper],
        MapperArray,
        HigherOrderMap
    ),
    intrinsic!(Lista, "omissa", 1, &[IntrinsicParamKind::NumerusIndex], ArrayInner, collection DropFirst),
    intrinsic!(
        Lista,
        "omnia",
        1,
        &[IntrinsicParamKind::Predicate],
        Bool,
        HigherOrderAll
    ),
    intrinsic!(Lista, "ordina", 0, &[], Vacuum, CodegenOnly),
    intrinsic!(
        Lista,
        "ordina",
        1,
        &[IntrinsicParamKind::Mapper],
        Vacuum,
        HigherOrderSortBy
    ),
    intrinsic!(Lista, "ordinata", 0, &[], ArrayInner, collection Sort),
    intrinsic!(
        Lista,
        "ordinata",
        1,
        &[IntrinsicParamKind::Mapper],
        ArrayInner,
        HigherOrderSortBy
    ),
    // need: the `take` trim (`prima`) reports an out-of-range count
    // on `⇥`, the same fault family as the `at` read: the payload is data
    // (count and list length), never prose.
    MethodIntrinsic {
        failable: true,
        ..intrinsic!(Lista, "prima", 1, &[IntrinsicParamKind::NumerusIndex], ArrayInner, collection Take)
    },
    intrinsic!(Lista, "primus", 0, &[], OptionInner, collection First),
    intrinsic!(
        Lista,
        "quilibet",
        1,
        &[IntrinsicParamKind::Predicate],
        Bool,
        HigherOrderAny
    ),
    intrinsic!(
        Lista,
        "reducta",
        2,
        &[IntrinsicParamKind::Reducer, IntrinsicParamKind::Accumulator],
        ReducerInfer,
        HigherOrderReduce
    ),
    intrinsic!(Lista, "detrahe", 0, &[], OptionInner, collection RemoveLast),
    // need: the `slice` window (`sectio`) reports out-of-range
    // bounds on `⇥`, the same fault family as the `at` read: the payload is
    // data (start, end, list length), never prose. `Textus "sectio"` below
    // reports the same way (Q-B3).
    MethodIntrinsic {
        failable: true,
        ..intrinsic!(
            Lista,
            "sectio",
            2,
            &[IntrinsicParamKind::NumerusIndex, IntrinsicParamKind::NumerusIndex],
            ArrayInner,
            collection Slice
        )
    },
    intrinsic!(Lista, "summa", 0, &[], NumericSum, collection Sum),
    intrinsic!(Lista, "ultima", 1, &[IntrinsicParamKind::NumerusIndex], ArrayInner, collection TakeLast),
    intrinsic!(Lista, "ultimus", 0, &[], OptionInner, collection Last),
    intrinsic!(Lista, "vacua", 0, &[], Bool, collection IsEmpty),
    // tabula
    intrinsic!(Tabula, "accipe", 1, &[IntrinsicParamKind::Key], OptionValue, collection Index),
    // need: `apud` is the checked read; a missing key reports on `⇥`
    // (default form `⊥`). The lista row above states the read split and payload.
    MethodIntrinsic {
        failable: true,
        ..intrinsic!(Tabula, "apud", 1, &[IntrinsicParamKind::Key], Value, collection Index)
    },
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
    intrinsic!(Copia, "immerge", 1, &[IntrinsicParamKind::Inner], Vacuum, collection Append),
    intrinsic!(Copia, "dele", 1, &[IntrinsicParamKind::Inner], Bool, collection Delete),
    intrinsic!(Copia, "differentia", 1, &[IntrinsicParamKind::SetInner], SetInner, collection Difference),
    intrinsic!(
        Copia,
        "filtrata",
        1,
        &[IntrinsicParamKind::Predicate],
        SetInner,
        HigherOrderFiltrata
    ),
    intrinsic!(Copia, "habet", 1, &[IntrinsicParamKind::Inner], Bool, collection Contains),
    intrinsic!(Copia, "intersectio", 1, &[IntrinsicParamKind::SetInner], SetInner, collection Intersection),
    intrinsic!(Copia, "longitudo", 0, &[], Numerus, collection Length),
    intrinsic!(
        Copia,
        "mappata",
        1,
        &[IntrinsicParamKind::Mapper],
        MapperSet,
        HigherOrderMap
    ),
    intrinsic!(Copia, "subcopia", 1, &[IntrinsicParamKind::SetInner], Bool, collection IsSubset),
    intrinsic!(Copia, "supercopia", 1, &[IntrinsicParamKind::SetInner], Bool, collection IsSuperset),
    intrinsic!(Copia, "symmetrica", 1, &[IntrinsicParamKind::SetInner], SetInner, collection SymmetricDifference),
    intrinsic!(Copia, "unio", 1, &[IntrinsicParamKind::SetInner], SetInner, collection Union),
    intrinsic!(Copia, "vacua", 0, &[], Bool, collection IsEmpty),
    // ascii
    intrinsic!(Ascii, "cursor", 0, &[], CursorLittera, collection CordCursor),
    // textus
    intrinsic!(Textus, "accipe", 1, &[IntrinsicParamKind::NumerusIndex], OptionTextus, collection Index),
    // need: `apud` is the checked read; an out-of-range index reports
    // on `⇥` (default form `⊥`). The lista row above states the read split and payload.
    MethodIntrinsic {
        failable: true,
        ..intrinsic!(Textus, "apud", 1, &[IntrinsicParamKind::NumerusIndex], Textus, collection Index)
    },
    intrinsic!(Textus, "continet", 1, &[IntrinsicParamKind::Textus], Bool, collection Contains),
    intrinsic!(Textus, "cursor", 0, &[], CursorLittera, collection CordCursor),
    intrinsic!(Textus, "divide", 1, &[IntrinsicParamKind::Textus], TextusArray, collection Split),
    intrinsic!(Textus, "finis", 1, &[IntrinsicParamKind::Textus], Bool, collection EndsWith),
    intrinsic!(Textus, "initium", 1, &[IntrinsicParamKind::Textus], Bool, collection StartsWith),
    intrinsic!(Textus, "longitudo", 0, &[], Numerus, collection Length),
    intrinsic!(Textus, "maiuscula", 0, &[], Textus, collection Uppercase),
    intrinsic!(Textus, "minuscula", 0, &[], Textus, collection Lowercase),
    intrinsic!(Textus, "munita", 0, &[], Textus, collection TextEscape),
    intrinsic!(Textus, "muta", 2, &[IntrinsicParamKind::Textus, IntrinsicParamKind::Textus], Textus, collection Replace),
    intrinsic!(Textus, "recide", 0, &[], Textus, collection Trim),
    // Q-B3 (operator ruling 2026-10-15): the text window reports an
    // out-of-range bound on `⇥` exactly like the `lista` window above; it no
    // longer clamps.
    MethodIntrinsic {
        failable: true,
        ..intrinsic!(
            Textus,
            "sectio",
            2,
            &[IntrinsicParamKind::NumerusIndex, IntrinsicParamKind::NumerusIndex],
            Textus,
            collection Slice
        )
    },
    intrinsic!(Textus, "vacua", 0, &[], Bool, collection IsEmpty),
    // queue (FIFO)
    intrinsic!(Queue, "appende", 1, &[IntrinsicParamKind::Inner], Vacuum, collection Append),
    intrinsic!(Queue, "decapita", 0, &[], OptionInner, collection RemoveFirst),
    intrinsic!(Queue, "longitudo", 0, &[], Numerus, collection Length),
    // stack (LIFO)
    intrinsic!(Stack, "appende", 1, &[IntrinsicParamKind::Inner], Vacuum, collection Append),
    intrinsic!(Stack, "detrahe", 0, &[], OptionInner, collection RemoveLast),
    intrinsic!(Stack, "longitudo", 0, &[], Numerus, collection Length),
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
    // Scalar built-in math functions on a float (the kernel spelling
    // `x.sqrt()`, `x.sin()`): the same functions, Latin names and IEEE
    // semantics as the tensor lift below. Scalar `absolutum` is the numeric
    // row above.
    intrinsic!(Fractus, "radix", 0, &[], SameAsReceiver, scalar_math Sqrt),
    intrinsic!(Fractus, "radix_cubica", 0, &[], SameAsReceiver, scalar_math Cbrt),
    intrinsic!(Fractus, "exponentia", 0, &[], SameAsReceiver, scalar_math Exp),
    intrinsic!(Fractus, "logarithmus", 0, &[], SameAsReceiver, scalar_math Ln),
    intrinsic!(Fractus, "logarithmus_decimalis", 0, &[], SameAsReceiver, scalar_math Log10),
    intrinsic!(Fractus, "sinus", 0, &[], SameAsReceiver, scalar_math Sin),
    intrinsic!(Fractus, "cosinus", 0, &[], SameAsReceiver, scalar_math Cos),
    intrinsic!(Fractus, "tangens", 0, &[], SameAsReceiver, scalar_math Tan),
    // sparsa
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
    // tensor
    // Q-X1 (operator ruling 2026-10-15): `longitudo` is the element count on
    // every collection; the number of extents and the Euclidean norm are the
    // named methods `rank` and `norm`.
    intrinsic!(Tensor, "longitudo", 0, &[], Numerus, collection Length),
    intrinsic!(Tensor, "rank", 0, &[], Numerus, collection TensorRank),
    intrinsic!(Tensor, "norm", 0, &[], Inner, collection TensorNorm),
    intrinsic!(Tensor, "magnitudines", 0, &[], ArrayInner, collection TensorShape),
    intrinsic!(
        Tensor,
        "forma",
        1,
        &[IntrinsicParamKind::NumerusArray],
        SameAsReceiver,
        collection TensorReshape
    ),
    // K11-b: the method form is the checked read; an out-of-range index is
    // reported on `⇥` (default form `⊥`). The index form (`t[i]`) shares the MIR
    // op but has no error channel.
    MethodIntrinsic {
        failable: true,
        range_checked_arg: Some(0),
        ..intrinsic!(
            Tensor,
            "accipe",
            1,
            &[IntrinsicParamKind::NumerusArray],
            Inner,
            collection TensorGet
        )
    },
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
        "subtracta",
        1,
        &[IntrinsicParamKind::TensorPeer],
        SameAsReceiver,
        collection TensorSub
    ),
    intrinsic!(
        Tensor,
        "multiplicata",
        1,
        &[IntrinsicParamKind::TensorPeer],
        SameAsReceiver,
        collection TensorMul
    ),
    intrinsic!(
        Tensor,
        "divisa",
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
    intrinsic!(Tensor, "maximus", 1, &[IntrinsicParamKind::Inner], SameAsReceiver, collection Maximum),
    intrinsic!(Tensor, "minimus", 1, &[IntrinsicParamKind::Inner], SameAsReceiver, collection Minimum),
    intrinsic!(Tensor, "summa", 0, &[], NumericSum, collection Sum),
    intrinsic!(Tensor, "media", 0, &[], FractusFromElement, collection TensorMean),
    // K11: the method form is the checked store; an out-of-range index is
    // reported on `⇥`. The index form (`t[i] ← v`) shares the MIR op but has no
    // error channel.
    MethodIntrinsic {
        failable: true,
        range_checked_arg: Some(0),
        ..intrinsic!(
            Tensor,
            "ponde",
            2,
            &[IntrinsicParamKind::NumerusArray, IntrinsicParamKind::Inner],
            Vacuum,
            collection TensorSet
        )
    },
    intrinsic!(Tensor, "reple", 1, &[IntrinsicParamKind::Inner], SameAsReceiver, collection TensorFill),
    MethodIntrinsic {
        receiver: IntrinsicReceiver::Tensor,
        faber_name: "formata",
        generic: &[CatalogGenericParam::typus("T")],
        arg_count: 0,
        params: &[],
        return_kind: IntrinsicReturnKind::GenericTypus,
        // Q-X10: the zero-arg generic view is a live reshape — the receiver's
        // literal extents lower to the `lista<numerus>` shape operand the
        // runner's `tensor_reshape` expects (ERR_FORMA_RESHAPE_COUNT guards
        // the element count). The one-arg `forma(shape)` method keeps its own
        // path through the shared op.
        mir_kind: IntrinsicMirKind::Collection(MirCollectionOp::TensorReshape),
        missing_generic_issue: Some("tensor_formata_complete_type_arg_required"),
        excess_generic_issue: None,
        value_arity_issue: Some("tensor_formata_forbids_value_args"),
        failable: false,
        range_checked_arg: None,
        axis_arg: None,
    },
    intrinsic!(Tensor, "planata", 0, &[], ArrayInner, collection TensorFlatten),
    // CCI-1: the bounded common-case intrinsic rows (RMSNorm, RoPE, row
    // gather). Source spellings/signatures frozen in
    // docs/archived/common-case-intrinsics/common-case-intrinsics-contract.md;
    // typed fail-closed contracts enforced at the HIR→AIR construction site
    // (air/lower.rs + radix-air-nodes/src/nodes.rs validate_*_contract).
    intrinsic!(
        Tensor,
        "gather",
        1,
        &[IntrinsicParamKind::TensorIndexIds],
        GatherShape,
        collection TensorGather
    ),
    MethodIntrinsic {
        axis_arg: Some(0),
        ..intrinsic!(
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
        )
    },
    MethodIntrinsic {
        receiver: IntrinsicReceiver::Tensor,
        faber_name: "rope_norm",
        generic: &[CatalogGenericParam::magnitudo("dim")],
        arg_count: 2,
        params: &[
            IntrinsicParamKind::NumerusIndex,
            IntrinsicParamKind::TensorPeer,
        ],
        return_kind: IntrinsicReturnKind::SameAsReceiver,
        mir_kind: IntrinsicMirKind::Collection(MirCollectionOp::TensorRope),
        missing_generic_issue: Some("rope_norm_dim_required"),
        excess_generic_issue: Some("rope_norm_generic_arity"),
        value_arity_issue: Some("rope_norm_value_arity"),
        failable: false,
        range_checked_arg: None,
        axis_arg: None,
    },
    MethodIntrinsic {
        receiver: IntrinsicReceiver::Tensor,
        faber_name: "rope_half",
        generic: &[CatalogGenericParam::magnitudo("dim")],
        arg_count: 2,
        params: &[
            IntrinsicParamKind::NumerusIndex,
            IntrinsicParamKind::TensorPeer,
        ],
        return_kind: IntrinsicReturnKind::SameAsReceiver,
        mir_kind: IntrinsicMirKind::Collection(MirCollectionOp::TensorRopeHalf),
        missing_generic_issue: Some("rope_half_dim_required"),
        excess_generic_issue: Some("rope_half_generic_arity"),
        value_arity_issue: Some("rope_half_value_arity"),
        failable: false,
        range_checked_arg: None,
        axis_arg: None,
    },
    intrinsic!(
        Tensor,
        "gelu",
        0,
        &[],
        SameAsReceiver,
        collection TensorGelu
    ),
    // GI3-1: SiLU is lowered by composition (`x / (1 + exp(-x))`) through
    // existing tensor elementwise operations; it owns no MIR collection op.
    intrinsic!(Tensor, "silu", 0, &[], SameAsReceiver, CodegenOnly),
    // K5 (gpu-reset rule 15): the built-in scalar math functions lifted
    // elementwise over a tensor. IEEE per element (domain miss is NaN,
    // overflow is infinity); `absolutum` takes any numeric element, the rest
    // take float elements only. User functions are never lifted. `radix`,
    // `exponentia` and `logarithmus` are the names the AIR lane already
    // carries for sqrt, exp and ln.
    intrinsic!(Tensor, "absolutum", 0, &[], SameAsReceiver, math Abs),
    intrinsic!(Tensor, "radix", 0, &[], SameAsReceiver, math Sqrt),
    intrinsic!(Tensor, "radix_cubica", 0, &[], SameAsReceiver, math Cbrt),
    intrinsic!(Tensor, "exponentia", 0, &[], SameAsReceiver, math Exp),
    intrinsic!(Tensor, "logarithmus", 0, &[], SameAsReceiver, math Ln),
    intrinsic!(Tensor, "logarithmus_decimalis", 0, &[], SameAsReceiver, math Log10),
    intrinsic!(Tensor, "sinus", 0, &[], SameAsReceiver, math Sin),
    intrinsic!(Tensor, "cosinus", 0, &[], SameAsReceiver, math Cos),
    intrinsic!(Tensor, "tangens", 0, &[], SameAsReceiver, math Tan),
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
        "softmax_causal",
        0,
        &[],
        SameAsReceiver,
        collection TensorCausalMaskedSoftmax
    ),
    intrinsic!(
        Tensor,
        "transpone",
        0,
        &[],
        SameAsReceiver,
        collection TensorTranspose
    ),
    MethodIntrinsic {
        axis_arg: Some(0),
        ..intrinsic!(
            Tensor,
            "laminatio",
            2,
            &[IntrinsicParamKind::NumerusIndex, IntrinsicParamKind::Inner],
            SameAsReceiver,
            collection TensorLayerNorm
        )
    },
    MethodIntrinsic {
        axis_arg: Some(0),
        ..intrinsic!(
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
        )
    },
    MethodIntrinsic {
        axis_arg: Some(0),
        ..intrinsic!(
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
        )
    },
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
    intrinsic!(
        Tensor,
        "sectio",
        3,
        &[
            IntrinsicParamKind::NumerusIndex,
            IntrinsicParamKind::NumerusIndex,
            IntrinsicParamKind::NumerusIndex,
        ],
        SameAsReceiver,
        collection TensorSlice
    ),
    intrinsic!(
        Tensor,
        "expande",
        1,
        &[IntrinsicParamKind::NumerusArray],
        SameAsReceiver,
        collection TensorExpand
    ),
    intrinsic!(
        Tensor,
        "disloco",
        1,
        &[IntrinsicParamKind::NumerusArray],
        SameAsReceiver,
        collection TensorShift
    ),
    MethodIntrinsic {
        receiver: IntrinsicReceiver::Tensor,
        faber_name: "limes",
        generic: &[CatalogGenericParam::typus("Policy")],
        arg_count: 0,
        params: &[],
        return_kind: IntrinsicReturnKind::SameAsReceiver,
        mir_kind: IntrinsicMirKind::TensorLimes,
        missing_generic_issue: Some("tensor_limes_policy_required"),
        excess_generic_issue: Some("tensor_limes_policy_arity"),
        value_arity_issue: Some("tensor_limes_value_arity"),
        failable: false,
        range_checked_arg: None,
        axis_arg: None,
    },
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
        "subtracta",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Matrix,
        "multiplicata",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Matrix,
        "divisa",
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
    // Rank-2 transpose: [R, C] → [C, R]. The return type is specialized in
    // typecheck (SameAsReceiver would keep the receiver shape).
    intrinsic!(Matrix, "transpone", 0, &[], SameAsReceiver, CodegenOnly),
    // Cellwise extremum (`⤒`/`⤓`) against a scalar or a same-shape matrix.
    intrinsic!(
        Matrix,
        "maximus",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Matrix,
        "minimus",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    // Q-X1: `longitudo` is the element count (rows times columns).
    intrinsic!(Matrix, "longitudo", 0, &[], Numerus, collection Length),
    // Ruling 2026-10-15 / need: the three Matrix coordinate reads.
    // `m.get([r, c])` is the nullable read (`T ∪ none`, see the lista
    // `accipe` above); `m.at([r, c])` and `m.set([r, c], v)` report an
    // out-of-range coordinate on `⇥` and share the Tensor `TensorGet` /
    // `TensorSet` MIR ops.
    intrinsic!(
        Matrix,
        "accipe",
        1,
        &[IntrinsicParamKind::NumerusArray],
        OptionInner,
        collection TensorGet
    ),
    MethodIntrinsic {
        failable: true,
        range_checked_arg: Some(0),
        ..intrinsic!(
            Matrix,
            "apud",
            1,
            &[IntrinsicParamKind::NumerusArray],
            Inner,
            collection TensorGet
        )
    },
    MethodIntrinsic {
        failable: true,
        range_checked_arg: Some(0),
        ..intrinsic!(
            Matrix,
            "ponde",
            2,
            &[IntrinsicParamKind::NumerusArray, IntrinsicParamKind::Inner],
            Vacuum,
            collection TensorSet
        )
    },
    // vector
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
        "subtracta",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Vector,
        "multiplicata",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Vector,
        "divisa",
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
    // Lanewise extremum (`⤒`/`⤓`) against a scalar or a same-shape vector;
    // typecheck owns the peer rule, MIR lowering unrolls the lanes.
    intrinsic!(
        Vector,
        "maximus",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    intrinsic!(
        Vector,
        "minimus",
        1,
        &[IntrinsicParamKind::ReceiverSelf],
        SameAsReceiver,
        CodegenOnly
    ),
    // Q-X1: `longitudo` is the lane count; the Euclidean norm is `norm`.
    intrinsic!(Vector, "longitudo", 0, &[], Numerus, CodegenOnly),
    intrinsic!(Vector, "norm", 0, &[], Inner, CodegenOnly),
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
    // RX-3b: `find` and the match accessors. Latin names per L-6: `inventa`
    // (ruled), then the accepted defaults `inventum`, `principium`, `terminus`,
    // `coetus`, `nominatus`.
    intrinsic!(Regex, "inventa", 1, &[IntrinsicParamKind::Textus], OptionMatch, collection RegexFind),
    // RX-4a: the two bulk ops. `collecta` is the accepted default; `divide` is
    // the existing Latin name on a new receiver.
    intrinsic!(Regex, "collecta", 1, &[IntrinsicParamKind::Textus], MatchArray, collection RegexFindAll),
    intrinsic!(Regex, "divide", 1, &[IntrinsicParamKind::Textus], TextusArray, collection RegexSplit),
    // RX-4b: literal replace on the `regex` receiver (the existing Latin name
    // `muta` on a new receiver). RX-5 (RD-5): the second slot also accepts a
    // `(match) → textus` closure; one row, the form is picked by the argument.
    // `escape` is a method on `textus` (RD-6), the accepted default `munita`.
    intrinsic!(Regex, "muta", 2, &[IntrinsicParamKind::Textus, IntrinsicParamKind::TextusOrReplacer], Textus, collection RegexReplace),
    // match
    intrinsic!(Match, "inventum", 0, &[], Textus, collection MatchText),
    intrinsic!(Match, "principium", 0, &[], Numerus, collection MatchStart),
    intrinsic!(Match, "terminus", 0, &[], Numerus, collection MatchEnd),
    intrinsic!(Match, "coetus", 1, &[IntrinsicParamKind::NumerusIndex], OptionTextus, collection MatchGroup),
    intrinsic!(Match, "nominatus", 1, &[IntrinsicParamKind::Textus], OptionTextus, collection MatchNamed),
];

/// How a namespace/struct catalog row publishes its result after generic binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamespaceReturnKind {
    /// The bound first Typus argument (`tensor.creata` / `tensor.structa`).
    GenericTypus,
    /// `tuus<T>` directional view.
    SermoTuus,
    /// `meus<T>` directional view.
    SermoMeus,
}

/// Compiler-owned namespace or named-struct intrinsic row.
///
/// Namespace constructors are not fake receivers. They share the catalog
/// generic descriptor shape and binder with [`MethodIntrinsic`].
#[derive(Debug, Clone, Copy)]
pub struct NamespaceIntrinsic {
    pub namespace: &'static str,
    pub faber_name: &'static str,
    pub generic: &'static [CatalogGenericParam],
    pub arg_count: usize,
    pub missing_generic_issue: Option<&'static str>,
    pub excess_generic_issue: Option<&'static str>,
    pub value_arity_issue: Option<&'static str>,
    pub diagnostic_method: Option<&'static str>,
    pub return_kind: NamespaceReturnKind,
}

/// Directional views of a `sermo` conversation (`meus` outbound, `tuus`
/// inbound).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SermoViewKind {
    Meus,
    Tuus,
}

/// One locale-projected method of a directional sermo view.
///
/// These methods have bespoke typing in the checker, so the row carries only
/// the canonical Latin name: it is the identity the reader packs' `[intrinsics]`
/// rows key on (an en file writes `t.get()`, never `t.accipe()`).
///
/// `da` (the send view's push) is deliberately absent: the Sink<T> rename owns
/// it, and until then it keeps its single spelling in every locale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SermoViewMethod {
    pub view: SermoViewKind,
    pub faber_name: &'static str,
}

/// Locale-projected methods of the directional sermo views.
pub static SERMO_VIEW_METHODS: &[SermoViewMethod] = &[
    SermoViewMethod {
        view: SermoViewKind::Meus,
        faber_name: "fini",
    },
    SermoViewMethod {
        view: SermoViewKind::Tuus,
        faber_name: "accipe",
    },
    SermoViewMethod {
        view: SermoViewKind::Tuus,
        faber_name: "cursor",
    },
    SermoViewMethod {
        view: SermoViewKind::Tuus,
        faber_name: "exhauri",
    },
    SermoViewMethod {
        view: SermoViewKind::Tuus,
        faber_name: "fini",
    },
];

/// Namespace and sermo-view catalog. Empty generic lists are illegal here.
pub static NAMESPACE_INTRINSICS: &[NamespaceIntrinsic] = &[
    NamespaceIntrinsic {
        namespace: "sermo",
        faber_name: "meus",
        generic: &[CatalogGenericParam::typus_default_valor("T")],
        arg_count: 0,
        missing_generic_issue: None,
        excess_generic_issue: Some("sermo_view_payload_type_arg_count"),
        value_arity_issue: None,
        diagnostic_method: Some("sermo.meus"),
        return_kind: NamespaceReturnKind::SermoMeus,
    },
    NamespaceIntrinsic {
        namespace: "sermo",
        faber_name: "tuus",
        generic: &[CatalogGenericParam::typus_default_valor("T")],
        arg_count: 0,
        missing_generic_issue: None,
        excess_generic_issue: Some("sermo_view_payload_type_arg_count"),
        value_arity_issue: None,
        diagnostic_method: Some("sermo.tuus"),
        return_kind: NamespaceReturnKind::SermoTuus,
    },
    NamespaceIntrinsic {
        namespace: "tensor",
        faber_name: "creata",
        generic: &[CatalogGenericParam::typus("T")],
        arg_count: 1,
        missing_generic_issue: Some("tensor_creata_complete_type_arg_required"),
        excess_generic_issue: None,
        value_arity_issue: None,
        diagnostic_method: None,
        return_kind: NamespaceReturnKind::GenericTypus,
    },
    NamespaceIntrinsic {
        namespace: "tensor",
        faber_name: "structa",
        generic: &[CatalogGenericParam::typus("T")],
        arg_count: 1,
        missing_generic_issue: None,
        excess_generic_issue: None,
        value_arity_issue: None,
        diagnostic_method: None,
        return_kind: NamespaceReturnKind::GenericTypus,
    },
];
