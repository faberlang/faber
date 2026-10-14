//! Meaning fingerprint for the formatter's preservation proof (D8.9).
//!
//! `faber format` and `faber convert` must never change a program's meaning:
//! they prove the emitted program is unchanged or refuse to write. The proof
//! compares the analyzed HIR of the input with the analyzed HIR of the
//! emitted output. HIR is already trivia-free and locale-neutral (keywords
//! lower to one node shape in every reader locale), so what remains is to
//! erase the analysis-local identities that differ between two analyses:
//!
//! - `Span` byte ranges are dropped;
//! - `Symbol` handles resolve to their interned text;
//! - `TypeId` / `IndexId` handles render as the type or index expression
//!   they name (recursively);
//! - `DefId`, `HirId`, and the other arena ids are renumbered by first
//!   occurrence, so equal structure yields equal numbering.
//!
//! The fingerprint is built from the `Debug` form of the HIR so that every
//! field of every node participates without a hand-maintained walker that
//! could silently skip a new field.

use crate::driver::AnalyzedModule;
use radix_hir::visit::{HirVisitorMut, walk_expr_mut};
use radix_hir::{HirExpression, HirExpressionKind, HirLiteral};
use radix_types::{Primitive, Type, TypeId, TypeTable};
use rustc_hash::FxHashMap;
use std::fmt::Write as _;

/// Renumbered id families. Each family gets its own first-occurrence counter.
const RENUMBERED_IDS: &[&str] = &["DefId", "HirId", "InferVar", "IndexVar"];

/// Presentation anchors carried inside HIR for trivia placement. They record
/// where authored text sat, not what the program does, so they are erased.
const ERASED_IDS: &[&str] = &["HirSourceAnchorId"];

/// Declared spelling aliases the formatter canonicalizes on purpose. Each row
/// maps an alias's HIR rendering to its canonical one; both lower to the same
/// runtime behavior, so the proof treats them as equal. Keep this list short
/// and justified: every row is a formatter policy, not a tolerance.
const ALIAS_NORMALIZATIONS: &[(&str, &str)] = &[
    // `scribe` is an alias of `nota` (same output channel in every backend).
    ("Scribe(Scribe, ", "Scribe(Nota, "),
    // Structural block-end anchors are trivia hooks (erased ids above); a
    // block that lost or gained its anchor still does the same thing.
    ("Some(HirBlockEnd { id: _, span: _ })", "None"),
];

/// One labeled, canonical segment of an analyzed program.
#[derive(Debug, Clone)]
pub(crate) struct MeaningSegment {
    /// Human label for diagnostics (`item 3 (Color)`, `entry`).
    pub label: String,
    /// Canonical, span-free, id-free rendering of the segment.
    pub text: String,
}

/// First place where two meaning fingerprints disagree.
#[derive(Debug, Clone)]
pub(crate) struct MeaningDifference {
    /// Segment label where the difference starts.
    pub segment: String,
    /// Excerpt of the input's canonical form at the first difference.
    pub expected: String,
    /// Excerpt of the output's canonical form at the first difference.
    pub actual: String,
}

/// Canonical meaning fingerprint of one analyzed module.
pub(crate) fn meaning_fingerprint(analysis: &AnalyzedModule) -> Vec<MeaningSegment> {
    let mut hir = analysis.hir.clone();
    HandledBodyFlattener.visit_program_mut(&mut hir);
    BinaryFloatLiteralSpelling { types: &analysis.types }.visit_program_mut(&mut hir);
    let mut canon = Canonicalizer::new(analysis);
    let mut segments = Vec::with_capacity(hir.items.len() + 2);
    for (index, item) in hir.items.iter().enumerate() {
        let text =
            canon.canonicalize(&format!("{:?}", item.kind)) + &canon.canonicalize(&format!("{:?}", item.annotations));
        segments.push(MeaningSegment { label: format!("item {index} ({})", item_name(&text)), text });
    }
    segments.push(MeaningSegment {
        label: "entry annotations".to_owned(),
        text: canon.canonicalize(&format!("{:?}", hir.entry_annotations)),
    });
    let entry = format!("{:?} args={:?} async={}", hir.entry, hir.entry_args_name, hir.entry_is_async);
    segments.push(MeaningSegment { label: "entry".to_owned(), text: canon.canonicalize(&entry) });
    segments
}

/// Structural no-ops that lowering produces from equivalent source forms.
///
/// A `fac { … } cape` body lowers to a handled block whose only content is a
/// tail block; attached handlers (`cura … cape`) lower the same body without
/// the wrapper. The wrapper adds no statement, binding, or value, so both
/// shapes are the same program.
struct HandledBodyFlattener;

impl HirVisitorMut for HandledBodyFlattener {
    fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
        walk_expr_mut(self, expr);
        if let HirExpressionKind::Handled { body, .. } = &mut expr.kind
            && body.statements.is_empty()
            && let Some(tail) = body.expr.take()
        {
            match tail.kind {
                HirExpressionKind::Block(inner) => *body = inner,
                kind => body.expr = Some(Box::new(HirExpression { kind, ..*tail })),
            }
        }
    }
}

/// Binary-float literal spelling is not meaning.
///
/// Typecheck retypes an integer constant in a `fractus` slot to
/// `Float(value)`; the emitter spells it `2.0`, which re-parses as
/// `FloatText { value, text: "2.0" }`. For a binary float the `text` carries
/// nothing the `value` does not (only decimal widths scale from the digits),
/// so both shapes render as `Float(value)`. The value still compares, so a
/// different number is still a different program; decimal-typed literals keep
/// their text.
struct BinaryFloatLiteralSpelling<'a> {
    types: &'a TypeTable,
}

impl BinaryFloatLiteralSpelling<'_> {
    fn is_binary_float(&self, mut ty: TypeId) -> bool {
        loop {
            match self.types.get(ty) {
                Type::Alias(_, inner) => ty = *inner,
                Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => return true,
                _ => return false,
            }
        }
    }
}

impl HirVisitorMut for BinaryFloatLiteralSpelling<'_> {
    fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
        walk_expr_mut(self, expr);
        if let HirExpressionKind::Literal(HirLiteral::FloatText { value, .. }) = &expr.kind
            && expr.ty.is_some_and(|ty| self.is_binary_float(ty))
        {
            expr.kind = HirExpressionKind::Literal(HirLiteral::Float(*value));
        }
    }
}

/// Compare two fingerprints; `None` means the meaning is unchanged.
pub(crate) fn first_meaning_difference(
    expected: &[MeaningSegment],
    actual: &[MeaningSegment],
) -> Option<MeaningDifference> {
    for (index, want) in expected.iter().enumerate() {
        let Some(got) = actual.get(index) else {
            return Some(MeaningDifference {
                segment: want.label.clone(),
                expected: excerpt(&want.text, 0),
                actual: "<missing>".to_owned(),
            });
        };
        if want.text != got.text {
            let at = first_divergent_byte(&want.text, &got.text);
            return Some(MeaningDifference {
                segment: want.label.clone(),
                expected: excerpt(&want.text, at),
                actual: excerpt(&got.text, at),
            });
        }
    }
    actual.get(expected.len()).map(|extra| MeaningDifference {
        segment: extra.label.clone(),
        expected: "<missing>".to_owned(),
        actual: excerpt(&extra.text, 0),
    })
}

fn item_name(text: &str) -> &str {
    // Declarations render as `Kind(HirX { name: Symbol("Foo"), ... })`.
    text.split_once("name: Symbol(\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map_or("?", |(name, _)| name)
}

fn first_divergent_byte(left: &str, right: &str) -> usize {
    let mut at = left
        .bytes()
        .zip(right.bytes())
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| left.len().min(right.len()));
    while at > 0 && !left.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn excerpt(text: &str, at: usize) -> String {
    const BEFORE: usize = 60;
    const AFTER: usize = 100;
    let mut start = at.saturating_sub(BEFORE);
    while start > 0 && !text.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (at + AFTER).min(text.len());
    while end < text.len() && !text.is_char_boundary(end) {
        end += 1;
    }
    let prefix = if start > 0 { "…" } else { "" };
    let suffix = if end < text.len() { "…" } else { "" };
    format!("{prefix}{}{suffix}", &text[start..end])
}

struct Canonicalizer<'a> {
    analysis: &'a AnalyzedModule,
    renumbered: FxHashMap<(&'static str, u64), usize>,
    counters: FxHashMap<&'static str, usize>,
    arena_text: FxHashMap<(bool, u32), String>,
    in_progress: Vec<(bool, u32)>,
}

impl<'a> Canonicalizer<'a> {
    fn new(analysis: &'a AnalyzedModule) -> Self {
        Self {
            analysis,
            renumbered: FxHashMap::default(),
            counters: FxHashMap::default(),
            arena_text: FxHashMap::default(),
            in_progress: Vec::new(),
        }
    }

    fn canonicalize(&mut self, debug: &str) -> String {
        let mut out = self.canonicalize_ids(debug);
        for (alias, canonical) in ALIAS_NORMALIZATIONS {
            if out.contains(alias) {
                out = out.replace(alias, canonical);
            }
        }
        out
    }

    fn canonicalize_ids(&mut self, debug: &str) -> String {
        let mut out = String::with_capacity(debug.len());
        let mut index = 0;
        let mut in_string = false;
        let mut escaped = false;
        while let Some(ch) = debug[index..].chars().next() {
            if !in_string && let Some(consumed) = self.try_rewrite(debug, index, &mut out) {
                index += consumed;
                continue;
            }
            out.push(ch);
            index += ch.len_utf8();
            // String literal content is copied verbatim, never rewritten.
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
            } else if ch == '"' {
                in_string = true;
            }
        }
        out
    }

    /// Rewrite an id token or span starting at `index`; return bytes consumed.
    fn try_rewrite(&mut self, debug: &str, index: usize, out: &mut String) -> Option<usize> {
        let rest = &debug[index..];
        let starts_word = index == 0 || {
            let prev = debug.as_bytes()[index - 1];
            !(prev.is_ascii_alphanumeric() || prev == b'_')
        };
        if !starts_word || !rest.as_bytes()[0].is_ascii_uppercase() {
            return None;
        }
        if let Some(after) = rest.strip_prefix("Span { start: ") {
            let close = after.find('}')?;
            out.push('_');
            return Some("Span { start: ".len() + close + 1);
        }
        let open = rest.find('(')?;
        let head = &rest[..open];
        if !head.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return None;
        }
        let digits_len = rest[open + 1..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        if digits_len == 0 || rest.as_bytes().get(open + 1 + digits_len) != Some(&b')') {
            return None;
        }
        let value: u64 = rest[open + 1..open + 1 + digits_len].parse().ok()?;
        let consumed = open + 2 + digits_len;
        match head {
            "Symbol" => {
                let strings = self.analysis.interner.strings();
                match usize::try_from(value).ok().and_then(|i| strings.get(i)) {
                    Some(text) => {
                        out.push_str("Symbol(");
                        write!(out, "{text:?}").ok();
                        out.push(')');
                    }
                    None => out.push_str("Symbol(<unresolved>)"),
                }
            }
            "TypeId" | "IndexId" => {
                let id = u32::try_from(value).ok()?;
                let is_type = head == "TypeId";
                let text = self.arena_text(is_type, id);
                out.push_str(if is_type { "Type(" } else { "Index(" });
                out.push_str(&text);
                out.push(')');
            }
            _ if ERASED_IDS.contains(&head) => out.push('_'),
            _ => {
                let family = RENUMBERED_IDS.iter().find(|family| **family == head)?;
                let next = self.counters.entry(family).or_default();
                let number = *self.renumbered.entry((family, value)).or_insert_with(|| {
                    let assigned = *next;
                    *next += 1;
                    assigned
                });
                out.push_str(family);
                out.push('#');
                out.push_str(&number.to_string());
            }
        }
        Some(consumed)
    }

    /// Render a type (`is_type`) or index expression by content, memoized.
    fn arena_text(&mut self, is_type: bool, id: u32) -> String {
        let key = (is_type, id);
        if let Some(text) = self.arena_text.get(&key) {
            return text.clone();
        }
        if self.in_progress.contains(&key) {
            return "<recursive>".to_owned();
        }
        let types = &self.analysis.types;
        let index = id as usize;
        let debug = if is_type && index < types.type_count() {
            match types.get(radix_types::TypeId(id)) {
                // Record fields are a hash map keyed by analysis-local symbol
                // handles; list them in a stable order after resolving names.
                radix_types::Type::Record(fields) => {
                    let mut entries: Vec<(&str, radix_types::TypeId)> = fields
                        .iter()
                        .map(|(name, ty)| {
                            let text = self
                                .analysis
                                .interner
                                .strings()
                                .get(name.0 as usize)
                                .map_or("<unresolved>", String::as_str);
                            (text, *ty)
                        })
                        .collect();
                    entries.sort_unstable_by(|a, b| a.0.cmp(b.0));
                    let rendered = entries
                        .iter()
                        .map(|(name, ty)| format!("{name:?}: {ty:?}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("Record {{{rendered}}}")
                }
                ty => format!("{ty:?}"),
            }
        } else if !is_type && index < types.index_count() {
            format!("{:?}", types.get_index(radix_types::IndexId(id)))
        } else {
            return "<unknown>".to_owned();
        };
        self.in_progress.push(key);
        let text = self.canonicalize(&debug);
        self.in_progress.pop();
        self.arena_text.insert(key, text.clone());
        text
    }
}
