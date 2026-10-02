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
//! carries, so each namespace unit adds its rows with the runtime it moves.
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

/// The canonical helper table: a row for every helper some runtime package
/// carries. Each namespace unit adds its rows with the runtime it moves.
pub const TABLE: &[RuntimeItem] = &[DISPLAY_LIST, DISPLAY_VALOR, DISPLAY_VERUM];

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
