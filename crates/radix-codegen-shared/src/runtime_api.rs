//! The canonical runtime-helper table, its per-target spelling rule, the
//! reserved-name set and the emission-time import recorder.
//!
//! Every helper a generated program calls lives in a per-target runtime
//! package. A helper's canonical id is `<namespace>.<name>`: both ASCII
//! `lower_snake_case`, except a type, whose name is `UpperCamelCase`. One rule
//! per target derives the emitted spelling from that id, so no emitter hand
//! spells a helper:
//!
//! | Target | Function | Type |
//! | --- | --- | --- |
//! | Rust | `faber::<ns>::<name>` | `faber::<Name>` |
//! | TypeScript | `<ns>.<lowerCamel(name)>` | `<ns>.<UpperCamel(name)>` |
//! | Go | `rt.<UpperCamel(ns)><UpperCamel(name)>` | same |
//! | Swift | `<UpperCamel(ns)>.<lowerCamel(name)>` | `<UpperCamel(ns)>.<UpperCamel(name)>` |
//! | Python | `<ns>.<name>` | `<ns>.<UpperCamel(name)>` |
//!
//! The Go rule is flat, so two ids could in principle spell alike
//! (`exact`+`store_i` against a hypothetical `exact_store`+`i`); the tests
//! assert that every target's spelling is injective over a slice.
//!
//! A row carries no body, parameter list or trap text: bodies are target code
//! in each runtime package. A row exists only for a helper some runtime package
//! carries, so each namespace unit adds its rows to the real [`TABLE`] with the
//! runtime it moves.
//!
//! Store-family helpers (`StoreFn`) enforce a width limit and may only be
//! spelled at a store site: [`spell`] refuses them and [`spell_store`], which
//! demands the [`HirStoreSite`], is the one way to spell one.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::BTreeSet;

use radix_hir::HirStoreSite;
use radix_target::Target;

/// The Go import path of the flat runtime package (`package rt`).
pub const GO_RUNTIME_IMPORT: &str = "faber/rt";

/// `display.verum`: a bivalens as `verum` / `falsum`.
pub const DISPLAY_VERUM: RuntimeItem = RuntimeItem {
    ns: Namespace::Display,
    name: "verum",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirGo],
};

/// `display.list`: a list as `[a, b, c]` through an element renderer.
pub const DISPLAY_LIST: RuntimeItem = RuntimeItem {
    ns: Namespace::Display,
    name: "list",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirGo],
};

/// `display.valor`: a boxed valor / json value with Faber display semantics.
pub const DISPLAY_VALOR: RuntimeItem = RuntimeItem {
    ns: Namespace::Display,
    name: "valor",
    kind: RuntimeKind::Fn,
    targets: &[Target::HirGo],
};

/// The targets whose runtime package carries the `exact` namespace today.
const EXACT_TARGETS: &[Target] = &[Target::HirRust];

const fn exact_row(name: &'static str, kind: RuntimeKind) -> RuntimeItem {
    RuntimeItem {
        ns: Namespace::Exact,
        name,
        kind,
        targets: EXACT_TARGETS,
    }
}

/// `exact` rows (T1-R1): the 64-bit-window integer carrier of the Rust runtime
/// (`faber::exact`). `EXACT_STORE` is the only store-family row: the width limit
/// applies at a store site and nowhere else.
pub const EXACT_APPROX_INT: RuntimeItem = exact_row("approx_int", RuntimeKind::Fn);
pub const EXACT_APPROX_F32: RuntimeItem = exact_row("approx_f32", RuntimeKind::Fn);
pub const EXACT_APPROX_F64: RuntimeItem = exact_row("approx_f64", RuntimeKind::Fn);
pub const EXACT_CMP_INT_FLOAT: RuntimeItem = exact_row("cmp_int_float", RuntimeKind::Fn);
pub const EXACT_CMP_SCALED_FLOAT: RuntimeItem = exact_row("cmp_scaled_float", RuntimeKind::Fn);
pub const EXACT_DIV: RuntimeItem = exact_row("div", RuntimeKind::Fn);
pub const EXACT_FIT: RuntimeItem = exact_row("fit", RuntimeKind::Fn);
pub const EXACT_INT: RuntimeItem = exact_row("int", RuntimeKind::Fn);
pub const EXACT_POW: RuntimeItem = exact_row("pow", RuntimeKind::Fn);
pub const EXACT_REM: RuntimeItem = exact_row("rem", RuntimeKind::Fn);
pub const EXACT_SHL: RuntimeItem = exact_row("shl", RuntimeKind::Fn);
pub const EXACT_SHR: RuntimeItem = exact_row("shr", RuntimeKind::Fn);
pub const EXACT_STORE: RuntimeItem = exact_row("store", RuntimeKind::StoreFn);

/// The targets whose runtime package carries the `dec` namespace today.
const DEC_TARGETS: &[Target] = &[Target::HirRust];

const fn dec_row(name: &'static str) -> RuntimeItem {
    RuntimeItem {
        ns: Namespace::Dec,
        name,
        kind: RuntimeKind::Fn,
        targets: DEC_TARGETS,
    }
}

/// `dec` rows (T1-R2): the exact decimal (`d64`) carrier of the Rust runtime
/// (`faber::dec`). An unstored decimal is an `(i128, u32)` carrier/scale pair;
/// `STORE` and `TRY_STORE` round it half-even to scale 8 into a stored `i64`.
/// Both are plain rows, not `StoreFn`: they are spelled where an unstored
/// decimal computation is consumed as a stored carrier, which has no
/// `HirStoreSite` at the call (a negation, a list element, a binary result).
pub const DEC_ADD: RuntimeItem = dec_row("add");
pub const DEC_CMP: RuntimeItem = dec_row("cmp");
pub const DEC_DIV: RuntimeItem = dec_row("div");
pub const DEC_F64: RuntimeItem = dec_row("f64");
pub const DEC_FMT: RuntimeItem = dec_row("fmt");
pub const DEC_FMT_LIST: RuntimeItem = dec_row("fmt_list");
pub const DEC_FMT_OPTION: RuntimeItem = dec_row("fmt_option");
pub const DEC_INT: RuntimeItem = dec_row("int");
pub const DEC_MUL: RuntimeItem = dec_row("mul");
pub const DEC_NEG: RuntimeItem = dec_row("neg");
pub const DEC_OF: RuntimeItem = dec_row("of");
pub const DEC_PARSE: RuntimeItem = dec_row("parse");
pub const DEC_REM: RuntimeItem = dec_row("rem");
pub const DEC_RHE: RuntimeItem = dec_row("rhe");
pub const DEC_ROUND: RuntimeItem = dec_row("round");
pub const DEC_STORE: RuntimeItem = dec_row("store");
pub const DEC_SUB: RuntimeItem = dec_row("sub");
pub const DEC_TRY_PARSE: RuntimeItem = dec_row("try_parse");
pub const DEC_TRY_STORE: RuntimeItem = dec_row("try_store");

/// The targets whose runtime package carries the `inf` namespace today.
const INF_TARGETS: &[Target] = &[Target::HirRust];

const fn inf_row(name: &'static str, kind: RuntimeKind) -> RuntimeItem {
    RuntimeItem {
        ns: Namespace::Inf,
        name,
        kind,
        targets: INF_TARGETS,
    }
}

/// `inf` rows (T1-R3): the wrappers over `faber::Magnus` of the Rust runtime
/// (`faber::inf`), the traps and conversions its operators do not spell
/// themselves. `INF_STORE` is the store-family row: the width limit of an `inf`
/// landing in a bounded slot applies at a store site and nowhere else.
pub const INF_BIG: RuntimeItem = inf_row("big", RuntimeKind::Fn);
pub const INF_APPROX: RuntimeItem = inf_row("approx", RuntimeKind::Fn);
pub const INF_CMP_DEC: RuntimeItem = inf_row("cmp_dec", RuntimeKind::Fn);
pub const INF_CMP_FLOAT: RuntimeItem = inf_row("cmp_float", RuntimeKind::Fn);
pub const INF_DIV: RuntimeItem = inf_row("div", RuntimeKind::Fn);
pub const INF_F32: RuntimeItem = inf_row("f32", RuntimeKind::Fn);
pub const INF_NARROW: RuntimeItem = inf_row("narrow", RuntimeKind::Fn);
pub const INF_PACK: RuntimeItem = inf_row("pack", RuntimeKind::Fn);
pub const INF_POW: RuntimeItem = inf_row("pow", RuntimeKind::Fn);
pub const INF_REM: RuntimeItem = inf_row("rem", RuntimeKind::Fn);
pub const INF_SHL: RuntimeItem = inf_row("shl", RuntimeKind::Fn);
pub const INF_SHR: RuntimeItem = inf_row("shr", RuntimeKind::Fn);
pub const INF_STORE: RuntimeItem = inf_row("store", RuntimeKind::StoreFn);
pub const INF_TO_DEC: RuntimeItem = inf_row("to_dec", RuntimeKind::Fn);
pub const INF_TRUE_DIV: RuntimeItem = inf_row("true_div", RuntimeKind::Fn);

/// The targets whose runtime package carries each `regex` row. Rows differ by
/// target: the Rust runtime carries the match operations as free functions in
/// `faber::regex`, the Go runtime carries a constructor plus methods, the Swift
/// runtime a `Regex` namespace, so each row lists exactly the targets that
/// consume it.
const REGEX_RUST_SWIFT_PY: &[Target] = &[Target::HirRust, Target::HirSwift, Target::HirPython];
const REGEX_RUST_PY: &[Target] = &[Target::HirRust, Target::HirPython];
const REGEX_GO: &[Target] = &[Target::HirGo];
const REGEX_GO_SWIFT: &[Target] = &[Target::HirGo, Target::HirSwift];
const REGEX_TS: &[Target] = &[Target::HirTypeScript];
const REGEX_SWIFT_PY: &[Target] = &[Target::HirSwift, Target::HirPython];
const REGEX_GO_TS_SWIFT_PY: &[Target] = &[
    Target::HirGo,
    Target::HirTypeScript,
    Target::HirSwift,
    Target::HirPython,
];
const REGEX_ALL: &[Target] = &[
    Target::HirRust,
    Target::HirGo,
    Target::HirTypeScript,
    Target::HirSwift,
    Target::HirPython,
];

const fn regex_row(
    name: &'static str,
    kind: RuntimeKind,
    targets: &'static [Target],
) -> RuntimeItem {
    RuntimeItem {
        ns: Namespace::Regex,
        name,
        kind,
        targets,
    }
}

/// `regex` rows (RX-7): the operations on a compiled `regex` and on a match,
/// as the Rust runtime carries them (`faber::regex`). A compiled pattern and a
/// match are the `faber::Regex` / `faber::Match` carriers; every operation is
/// total because the conversion `↦ regex` is the one place a pattern is
/// compiled or rejected. Offsets are code points.
pub const REGEX_MATCHES: RuntimeItem = regex_row("matches", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);
pub const REGEX_FIND: RuntimeItem = regex_row("find", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);
pub const REGEX_FIND_ALL: RuntimeItem = regex_row("find_all", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);
pub const REGEX_SPLIT: RuntimeItem = regex_row("split", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);
pub const REGEX_REPLACE: RuntimeItem = regex_row("replace", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);
pub const REGEX_REPLACE_WITH: RuntimeItem =
    regex_row("replace_with", RuntimeKind::Fn, REGEX_RUST_PY);
pub const REGEX_MATCH_TEXT: RuntimeItem =
    regex_row("match_text", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);
pub const REGEX_MATCH_START: RuntimeItem =
    regex_row("match_start", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);
pub const REGEX_MATCH_END: RuntimeItem =
    regex_row("match_end", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);
pub const REGEX_MATCH_GROUP: RuntimeItem =
    regex_row("match_group", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);
pub const REGEX_MATCH_NAMED: RuntimeItem =
    regex_row("match_named", RuntimeKind::Fn, REGEX_RUST_SWIFT_PY);

/// `regex` rows (RX-9): the Go carrier of the RE2-dialect regex. `Regex` is the
/// compiled value, `Match` one successful match; the operations on them are
/// methods of those types, so only the constructor and the two helpers with no
/// receiver of that type are functions.
pub const REGEX_REGEX: RuntimeItem = regex_row("Regex", RuntimeKind::Type, REGEX_GO_TS_SWIFT_PY);
pub const REGEX_MATCH: RuntimeItem = regex_row("Match", RuntimeKind::Type, REGEX_GO_TS_SWIFT_PY);
pub const REGEX_NEW: RuntimeItem = regex_row("new", RuntimeKind::Fn, REGEX_GO);
pub const REGEX_ESCAPE: RuntimeItem = regex_row("escape", RuntimeKind::Fn, REGEX_ALL);
pub const REGEX_REPLACE_EACH: RuntimeItem =
    regex_row("replace_each", RuntimeKind::Fn, REGEX_GO_SWIFT);

/// `regex` rows (RX-8): the TypeScript runtime package's `regex` namespace
/// (`@faber/runtime/regex`). `Regex` is the compiled pattern, `Match` one
/// successful match (it has no source-level type name, RD-3), `RegexError` the
/// rejection of a pattern a conversion throws, and `escape` the `textus`
/// method `munita`. The operations are members of the `Regex` and `Match`
/// types, not table rows.
pub const REGEX_ERROR: RuntimeItem = regex_row("RegexError", RuntimeKind::Type, REGEX_TS);

/// `regex` rows (RX-10a): the Swift runtime (`Regex.Regex`, `Regex.Match`,
/// `Regex.compile`, ...). Swift consumes the same operation rows as the Rust
/// target (as free functions of the `Regex` namespace) plus the carrier types,
/// `replace_each` and `escape`; `compile` is its constructor.
pub const REGEX_COMPILE: RuntimeItem = regex_row("compile", RuntimeKind::Fn, REGEX_SWIFT_PY);

/// The canonical helper table: a row for every helper some runtime package
/// carries. Each namespace unit adds its rows with the runtime it moves.
pub const TABLE: &[RuntimeItem] = &[
    DEC_ADD,
    DEC_CMP,
    DEC_DIV,
    DEC_F64,
    DEC_FMT,
    DEC_FMT_LIST,
    DEC_FMT_OPTION,
    DEC_INT,
    DEC_MUL,
    DEC_NEG,
    DEC_OF,
    DEC_PARSE,
    DEC_REM,
    DEC_RHE,
    DEC_ROUND,
    DEC_STORE,
    DEC_SUB,
    DEC_TRY_PARSE,
    DEC_TRY_STORE,
    DISPLAY_LIST,
    DISPLAY_VALOR,
    DISPLAY_VERUM,
    EXACT_APPROX_INT,
    EXACT_APPROX_F32,
    EXACT_APPROX_F64,
    EXACT_CMP_INT_FLOAT,
    EXACT_CMP_SCALED_FLOAT,
    EXACT_DIV,
    EXACT_FIT,
    EXACT_INT,
    EXACT_POW,
    EXACT_REM,
    EXACT_SHL,
    EXACT_SHR,
    EXACT_STORE,
    INF_BIG,
    INF_APPROX,
    INF_CMP_DEC,
    INF_CMP_FLOAT,
    INF_DIV,
    INF_F32,
    INF_NARROW,
    INF_PACK,
    INF_POW,
    INF_REM,
    INF_SHL,
    INF_SHR,
    INF_STORE,
    INF_TO_DEC,
    INF_TRUE_DIV,
    REGEX_MATCHES,
    REGEX_FIND,
    REGEX_FIND_ALL,
    REGEX_SPLIT,
    REGEX_REPLACE,
    REGEX_REPLACE_WITH,
    REGEX_ESCAPE,
    REGEX_MATCH_TEXT,
    REGEX_MATCH_START,
    REGEX_MATCH_END,
    REGEX_MATCH_GROUP,
    REGEX_MATCH_NAMED,
    REGEX_REGEX,
    REGEX_MATCH,
    REGEX_NEW,
    REGEX_REPLACE_EACH,
    REGEX_ERROR,
    REGEX_COMPILE,
];

/// One runtime namespace: a module in Rust, a namespace export in TypeScript,
/// a name prefix in Go.
///
/// Ordered by spelling so a recorded set iterates alphabetically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Namespace {
    Exact,
    Dec,
    Inf,
    Format,
    Modulus,
    Display,
    Order,
    Tensor,
    Sparsa,
    Vector,
    Matrix,
    Intervallum,
    Instans,
    Regex,
    Json,
    Valor,
    Tuple,
    Atomic,
    Dup,
    Result,
    Variant,
    Sermo,
    Gen,
    Float,
}

impl Namespace {
    /// Every namespace, in declaration order.
    pub const ALL: [Namespace; 24] = [
        Namespace::Exact,
        Namespace::Dec,
        Namespace::Inf,
        Namespace::Format,
        Namespace::Modulus,
        Namespace::Display,
        Namespace::Order,
        Namespace::Tensor,
        Namespace::Sparsa,
        Namespace::Vector,
        Namespace::Matrix,
        Namespace::Intervallum,
        Namespace::Instans,
        Namespace::Regex,
        Namespace::Json,
        Namespace::Valor,
        Namespace::Tuple,
        Namespace::Atomic,
        Namespace::Dup,
        Namespace::Result,
        Namespace::Variant,
        Namespace::Sermo,
        Namespace::Gen,
        Namespace::Float,
    ];

    /// The `lower_snake_case` spelling of the namespace.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Namespace::Exact => "exact",
            Namespace::Dec => "dec",
            Namespace::Inf => "inf",
            Namespace::Format => "format",
            Namespace::Modulus => "modulus",
            Namespace::Display => "display",
            Namespace::Order => "order",
            Namespace::Tensor => "tensor",
            Namespace::Sparsa => "sparsa",
            Namespace::Vector => "vector",
            Namespace::Matrix => "matrix",
            Namespace::Intervallum => "intervallum",
            Namespace::Instans => "instans",
            Namespace::Regex => "regex",
            Namespace::Json => "json",
            Namespace::Valor => "valor",
            Namespace::Tuple => "tuple",
            Namespace::Atomic => "atomic",
            Namespace::Dup => "dup",
            Namespace::Result => "result",
            Namespace::Variant => "variant",
            Namespace::Sermo => "sermo",
            Namespace::Gen => "gen",
            Namespace::Float => "float",
        }
    }
}

impl PartialOrd for Namespace {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Namespace {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

/// What a table row names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuntimeKind {
    /// An ordinary helper function.
    Fn,
    /// A store-family helper (`store`, `modulus` widths): spellable only at a
    /// store site, through [`spell_store`].
    StoreFn,
    /// A runtime type.
    Type,
}

/// One row of the canonical helper table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeItem {
    pub ns: Namespace,
    /// `lower_snake_case` for a function, `UpperCamelCase` for a type.
    pub name: &'static str,
    pub kind: RuntimeKind,
    /// The targets whose runtime package carries the helper.
    pub targets: &'static [Target],
}

/// Why a row has no spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpellError {
    /// A `StoreFn` row went through [`spell`]; use [`spell_store`].
    StoreOnly,
    /// A non-`StoreFn` row went through [`spell_store`]; use [`spell`].
    NotStore,
    /// The target has no runtime-helper spelling rule.
    NoRuntime(Target),
}

/// The spelling of a non-store helper in `target`'s generated source.
///
/// # Errors
///
/// [`SpellError::StoreOnly`] for a `StoreFn` row, [`SpellError::NoRuntime`]
/// for a target outside Rust, TypeScript, Go, Swift and Python.
pub fn spell(target: Target, item: &RuntimeItem) -> Result<String, SpellError> {
    if item.kind == RuntimeKind::StoreFn {
        return Err(SpellError::StoreOnly);
    }
    spelling(target, item)
}

/// The spelling of a store-family helper. The [`HirStoreSite`] is the witness
/// that the call sits at a store site; it does not change the spelling.
///
/// # Errors
///
/// [`SpellError::NotStore`] for a row that is not a `StoreFn`,
/// [`SpellError::NoRuntime`] for a target without a spelling rule.
pub fn spell_store(
    target: Target,
    item: &RuntimeItem,
    _site: &HirStoreSite,
) -> Result<String, SpellError> {
    if item.kind != RuntimeKind::StoreFn {
        return Err(SpellError::NotStore);
    }
    spelling(target, item)
}

fn spelling(target: Target, item: &RuntimeItem) -> Result<String, SpellError> {
    let ns = item.ns.as_str();
    let is_type = item.kind == RuntimeKind::Type;
    match target {
        Target::HirRust if is_type => Ok(format!("faber::{}", upper_camel(item.name))),
        Target::HirRust => Ok(format!("faber::{ns}::{}", item.name)),
        Target::HirTypeScript | Target::HirPython if is_type => {
            Ok(format!("{ns}.{}", upper_camel(item.name)))
        }
        Target::HirTypeScript => Ok(format!("{ns}.{}", lower_camel(item.name))),
        Target::HirGo => Ok(format!("rt.{}{}", upper_camel(ns), upper_camel(item.name))),
        Target::HirSwift if is_type => {
            Ok(format!("{}.{}", upper_camel(ns), upper_camel(item.name)))
        }
        Target::HirSwift => Ok(format!("{}.{}", upper_camel(ns), lower_camel(item.name))),
        Target::HirPython => Ok(format!("{ns}.{}", item.name)),
        _ => Err(SpellError::NoRuntime(target)),
    }
}

/// The names a user binding must not take in `target`'s generated source,
/// sorted and without duplicates (a colliding binding escapes with a trailing
/// `_`).
///
/// Rust reserves the crate name `faber` and Go the package name `rt`. The
/// other targets reserve the identifier each namespace is spelled by (the
/// lower-case namespace in TypeScript and Python, the `UpperCamel` enum name in
/// Swift) for every namespace that has a row carried by `target`.
#[must_use]
pub fn reserved_names(target: Target, items: &[RuntimeItem]) -> Vec<String> {
    let namespaces = || {
        items
            .iter()
            .filter(|item| item.targets.contains(&target))
            .map(|item| item.ns.as_str())
    };
    let names: BTreeSet<String> = match target {
        Target::HirRust => BTreeSet::from(["faber".to_owned()]),
        Target::HirGo => BTreeSet::from(["rt".to_owned()]),
        Target::HirTypeScript | Target::HirPython => namespaces().map(str::to_owned).collect(),
        Target::HirSwift => namespaces().map(upper_camel).collect(),
        _ => BTreeSet::new(),
    };
    names.into_iter().collect()
}

/// The namespaces an emission referenced, recorded as the emitter spells
/// helpers so the file's imports are exactly what it used.
#[derive(Debug, Default)]
pub struct RuntimeRefs {
    used: RefCell<BTreeSet<Namespace>>,
}

impl RuntimeRefs {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one use of `ns`.
    pub fn note(&self, ns: Namespace) {
        self.used.borrow_mut().insert(ns);
    }

    /// Spell a non-store helper for `target` and record its namespace: the
    /// one way an emitter writes a helper into generated source, so the file's
    /// imports are exactly the namespaces it spelled.
    ///
    /// # Errors
    ///
    /// The errors of [`spell`]; nothing is recorded on an error.
    pub fn spell(&self, target: Target, item: &RuntimeItem) -> Result<String, SpellError> {
        let spelled = spell(target, item)?;
        self.note(item.ns);
        Ok(spelled)
    }

    /// The recorded namespaces, sorted by spelling, each once.
    #[must_use]
    pub fn used(&self) -> Vec<Namespace> {
        self.used.borrow().iter().copied().collect()
    }
}

/// The Go import the recorded references need: [`GO_RUNTIME_IMPORT`] when any
/// namespace was recorded, otherwise none.
#[must_use]
pub fn go_imports(refs: &RuntimeRefs) -> Option<&'static str> {
    if refs.used.borrow().is_empty() {
        None
    } else {
        Some(GO_RUNTIME_IMPORT)
    }
}

/// `checked_mul` to `CheckedMul`; an already `UpperCamel` name is unchanged.
fn upper_camel(name: &str) -> String {
    name.split('_').map(capitalize).collect()
}

/// `checked_mul` to `checkedMul`.
fn lower_camel(name: &str) -> String {
    let mut segments = name.split('_');
    let mut out = String::new();
    if let Some(first) = segments.next() {
        let mut chars = first.chars();
        if let Some(head) = chars.next() {
            out.extend(head.to_lowercase());
            out.push_str(chars.as_str());
        }
    }
    out.extend(segments.map(capitalize));
    out
}

fn capitalize(segment: &str) -> String {
    let mut chars = segment.chars();
    match chars.next() {
        Some(head) => head.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
#[path = "runtime_api_test.rs"]
mod tests;
