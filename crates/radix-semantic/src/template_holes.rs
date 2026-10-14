//! Position analysis for named and positional template holes.
//!
//! Template labels are a source-level alias for the existing positional hole
//! domain.  This module deliberately does not know about the parser, AST, HIR,
//! or a particular template consumer.  It scans the literal text and produces
//! the positional form that existing consumers already understand.

use std::collections::{BTreeMap, BTreeSet};

/// The source spelling of one template hole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HoleKind {
    /// `§`, which consumes the next automatic position.
    Auto,
    /// `§N`, which addresses an explicit zero-based position.
    Index(usize),
    /// `§{label}`, a source-level name for the hole's automatic position.
    Named(String),
}

/// One hole in source order, with the position it addresses after analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hole {
    pub kind: HoleKind,
    pub position: usize,
}

/// The result of scanning one template literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateHoleAnalysis {
    /// Holes in source order. Explicit indexes do not consume an automatic
    /// position, matching the existing `§`/`§N` consumers.
    pub holes: Vec<Hole>,
    /// Each duplicated label appears once, in lexical order of its spelling.
    pub duplicate_labels: Vec<String>,
    /// The first position assigned to each label. Duplicate labels remain
    /// reported in [`Self::duplicate_labels`].
    pub label_positions: BTreeMap<String, usize>,
    /// The template text with named-hole syntax erased to `§`/`§N` syntax.
    pub erased: String,
}

impl TemplateHoleAnalysis {
    /// Return the positional address of a declared label.
    #[must_use]
    pub fn label_position(&self, label: &str) -> Option<usize> {
        self.label_positions.get(label).copied()
    }

    /// Whether the template has no duplicate named-hole labels.
    #[must_use]
    pub fn labels_are_unique(&self) -> bool {
        self.duplicate_labels.is_empty()
    }

    /// The positional value slots a template application must supply.
    ///
    /// The first element is the number of required values: the largest hole
    /// position plus one, or zero when the template has no holes. The second
    /// element is the number of distinct positions some hole actually reads;
    /// when it is smaller than the first, the template leaves a value slot
    /// unread.
    #[must_use]
    pub fn value_slots(&self) -> (usize, usize) {
        let Some(max_position) = self.holes.iter().map(|hole| hole.position).max() else {
            return (0, 0);
        };
        let distinct: BTreeSet<usize> = self.holes.iter().map(|hole| hole.position).collect();
        (max_position.saturating_add(1), distinct.len())
    }

    /// The smallest required value slot that no hole addresses.
    ///
    /// A missing position exists exactly when [`Self::value_slots`] reports a
    /// distinct count smaller than its required count. This names the first
    /// gap in `0..required`, or `None` when every position in that range is
    /// read.
    #[must_use]
    pub fn first_unread_slot(&self) -> Option<usize> {
        let (required, _) = self.value_slots();
        let read: BTreeSet<usize> = self.holes.iter().map(|hole| hole.position).collect();
        (0..required).find(|position| !read.contains(position))
    }
}

/// Scan `template` and assign all holes to the existing positional domain.
///
/// Automatic holes (`§`) and named holes (`§{label}`) consume positions from
/// the same zero-based counter. Explicit holes (`§N`) address `N` without
/// advancing that counter. This preserves the established backend behavior for
/// interleaved explicit and automatic holes, including `§1 §` → positions
/// `[1, 0]`.
#[must_use]
pub fn analyze_template(template: &str) -> TemplateHoleAnalysis {
    let mut holes = Vec::new();
    let mut duplicate_labels = BTreeSet::new();
    let mut label_positions = BTreeMap::new();
    let mut erased = String::with_capacity(template.len());
    let mut next_auto_position = 0usize;
    let mut cursor = 0usize;

    while cursor < template.len() {
        let Some(character) = template[cursor..].chars().next() else {
            break;
        };
        if character != '§' {
            let next = cursor + character.len_utf8();
            erased.push_str(&template[cursor..next]);
            cursor = next;
            continue;
        }

        let marker_end = cursor + character.len_utf8();
        if let Some((label, end)) = named_hole_at(template, marker_end) {
            let position = next_auto_position;
            next_auto_position = next_auto_position.saturating_add(1);
            holes.push(Hole {
                kind: HoleKind::Named(label.to_owned()),
                position,
            });
            if label_positions.contains_key(label) {
                // Keep the first position in the public map while reporting
                // every repeated spelling separately.
                duplicate_labels.insert(label.to_owned());
            } else {
                label_positions.insert(label.to_owned(), position);
            }
            erased.push('§');
            cursor = end;
            continue;
        }

        if let Some((digits, end)) = indexed_hole_at(template, marker_end) {
            let position = digits.parse::<usize>().unwrap_or(usize::MAX);
            holes.push(Hole {
                kind: HoleKind::Index(position),
                position,
            });
            erased.push('§');
            erased.push_str(digits);
            cursor = end;
            continue;
        }

        // A bare marker is the established automatic hole. This is also the
        // fallback for an invalid would-be label such as `§{a b}`: the marker
        // remains an ordinary hole and its following text remains literal.
        let position = next_auto_position;
        next_auto_position = next_auto_position.saturating_add(1);
        holes.push(Hole {
            kind: HoleKind::Auto,
            position,
        });
        erased.push('§');
        cursor = marker_end;
    }

    TemplateHoleAnalysis {
        holes,
        duplicate_labels: duplicate_labels.into_iter().collect(),
        label_positions,
        erased,
    }
}

/// Alias for callers that describe this operation as a scan.
#[must_use]
pub fn scan_template(template: &str) -> TemplateHoleAnalysis {
    analyze_template(template)
}

fn named_hole_at(template: &str, marker_end: usize) -> Option<(&str, usize)> {
    if !template.get(marker_end..)?.starts_with('{') {
        return None;
    }
    let label_start = marker_end + '{'.len_utf8();
    let close_offset = template.get(label_start..)?.find('}')?;
    let close = label_start + close_offset;
    let label = template.get(label_start..close)?;
    is_identifier(label).then_some((label, close + '}'.len_utf8()))
}

fn indexed_hole_at(template: &str, marker_end: usize) -> Option<(&str, usize)> {
    let bytes = template.as_bytes();
    let mut end = marker_end;
    while bytes.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    (end > marker_end).then_some((&template[marker_end..end], end))
}

// Keep this predicate aligned with the language's identifier shape without
// pulling AST or lexer types into the pure template analysis module. Keyword
// spellings are intentionally accepted: the parser applies contextual keyword
// policy at the call site, while this scanner only needs the spelling boundary.
fn is_identifier(label: &str) -> bool {
    let mut characters = label.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first == '_' || first.is_alphabetic())
        && characters.all(|character| character == '_' || character.is_alphanumeric())
}
