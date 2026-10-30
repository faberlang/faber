//! Resolved trivia lookups for presentation-aware canonical Faber emission.
//!
//! The presentation sidecar ([`HirPresentation`]) carries typed owners and
//! ordered attachments; this index flattens them into anchor-keyed and
//! semantic-ref-keyed lookups so render slots can place trivia in one hop.
//! The index owns its data (no borrows into the sidecar) so [`super::FaberCodegen`]
//! can carry it across `&self` render methods without lifetime parameters.

use handroll::FxHashMap;
use radix_codegen_shared::CodeWriter;
use radix_hir::{
    DefId, HirBlock, HirId, HirOwnerRef, HirPresentation, HirSourceAnchorId, HirTrivia,
};
use radix_lexer::Interner;

/// Anchor/owner attachment lookups for one presentation sidecar.
#[derive(Debug)]
pub struct TriviaIndex {
    /// Anchor → non-empty leading trivia, for structural-end and entry lookups.
    attachments: FxHashMap<HirSourceAnchorId, Vec<HirTrivia>>,
    /// Semantic `HirId` (item or statement) → owning anchor.
    by_hir: FxHashMap<HirId, HirSourceAnchorId>,
    /// Semantic `DefId` (genus/implendum member) → owning anchor.
    by_def: FxHashMap<DefId, HirSourceAnchorId>,
    /// `(union DefId, shared-field ordinal)` → owning anchor.
    by_union_shared_field: FxHashMap<(DefId, u32), HirSourceAnchorId>,
    /// Anchor for the explicit entry (`incipit`) leading trivia, when present.
    entry_start_anchor: Option<HirSourceAnchorId>,
    /// Anchor of the program's structural end.
    program_end_anchor: HirSourceAnchorId,
}

impl Default for TriviaIndex {
    fn default() -> Self {
        Self {
            attachments: FxHashMap::default(),
            by_hir: FxHashMap::default(),
            by_def: FxHashMap::default(),
            by_union_shared_field: FxHashMap::default(),
            entry_start_anchor: None,
            program_end_anchor: HirSourceAnchorId(0),
        }
    }
}

impl TriviaIndex {
    /// Build the flattened lookups from one presentation sidecar.
    ///
    /// Only comment-bearing attachments are indexed: a lookup returning `Some`
    /// then guarantees renderable trivia (compact-joint gates and separator
    /// suppression rely on that guarantee). Newline-only boundaries carry no
    /// presentational payload — the emitter's own statement/construct newlines
    /// reproduce them, and canonical output does not preserve blank-line-only
    /// layout.
    #[must_use]
    pub fn from_presentation(presentation: &HirPresentation) -> Self {
        let mut attachments = FxHashMap::default();
        for attachment in &presentation.attachments {
            if has_comment(&attachment.leading) {
                attachments.insert(attachment.anchor.clone(), attachment.leading.clone());
            }
        }
        let mut by_hir = FxHashMap::default();
        let mut by_def = FxHashMap::default();
        let mut by_union_shared_field = FxHashMap::default();
        for owner in &presentation.owners {
            match &owner.kind {
                radix_hir::HirTriviaOwnerKind::UnionSharedField { ordinal } => {
                    if let Some(HirOwnerRef::Def(def_id)) = &owner.semantic {
                        by_union_shared_field.insert((*def_id, *ordinal), owner.id.clone());
                    }
                }
                _ => match &owner.semantic {
                    Some(HirOwnerRef::Hir(id)) => {
                        by_hir.insert(*id, owner.id.clone());
                    }
                    Some(HirOwnerRef::Def(def_id)) => {
                        by_def.insert(*def_id, owner.id.clone());
                    }
                    None => {}
                },
            }
        }
        Self {
            attachments,
            by_hir,
            by_def,
            by_union_shared_field,
            entry_start_anchor: presentation.entry_start_anchor.clone(),
            program_end_anchor: presentation.program_end.id.clone(),
        }
    }

    /// Trivia attached to one anchor, when non-empty.
    pub fn for_anchor(&self, anchor: &HirSourceAnchorId) -> Option<&[HirTrivia]> {
        self.attachments.get(anchor).map(Vec::as_slice)
    }

    /// Trivia attached to a semantic HIR node (item or statement).
    #[must_use]
    pub fn for_hir(&self, id: HirId) -> Option<&[HirTrivia]> {
        self.by_hir
            .get(&id)
            .and_then(|anchor| self.for_anchor(anchor))
    }

    /// Trivia attached to a semantic definition (genus/implendum member).
    #[must_use]
    pub fn for_def(&self, def_id: DefId) -> Option<&[HirTrivia]> {
        self.by_def
            .get(&def_id)
            .and_then(|anchor| self.for_anchor(anchor))
    }

    /// Trivia attached to one shared field of a union declaration.
    #[must_use]
    pub fn for_union_shared_field(&self, def_id: DefId, ordinal: u32) -> Option<&[HirTrivia]> {
        self.by_union_shared_field
            .get(&(def_id, ordinal))
            .and_then(|anchor| self.for_anchor(anchor))
    }

    /// Trivia attached to the explicit entry start, when present.
    #[must_use]
    pub fn entry_trivia(&self) -> Option<&[HirTrivia]> {
        self.entry_start_anchor
            .as_ref()
            .and_then(|anchor| self.for_anchor(anchor))
    }

    /// Trivia attached to the program's structural end.
    #[must_use]
    pub fn program_end_trivia(&self) -> Option<&[HirTrivia]> {
        self.for_anchor(&self.program_end_anchor)
    }

    /// Whether a candidate compact block carries any attached trivia on its
    /// statements, lifted tail expression, or structural end.
    ///
    /// Compact `ergo redde` / `sic … secus …` output has no slot for attached
    /// trivia, so comment-bearing blocks must stay braced (locked invariant).
    #[must_use]
    pub fn block_has_trivia(&self, block: &HirBlock) -> bool {
        if let Some(end) = &block.end
            && self.for_anchor(&end.id).is_some()
        {
            return true;
        }
        block
            .statements
            .iter()
            .any(|stmt| self.for_hir(stmt.id).is_some())
            || block
                .expr
                .as_ref()
                .is_some_and(|expr| self.for_hir(expr.id).is_some())
    }
}

/// Whether a trivia run carries at least one comment line.
fn has_comment(trivia: &[HirTrivia]) -> bool {
    trivia
        .iter()
        .any(|item| matches!(item, HirTrivia::CommentLine { .. }))
}

/// Strip the boundary newlines that begin the file.
///
/// [`write_leading_trivia`] drops exactly one leading boundary newline because
/// every render slot is preceded by the emitter's own statement/construct
/// newline. At the very start of the file no construct newline precedes the
/// slot, so every leading newline is boundary: canonical output never begins
/// with blank lines. Without this, a source whose body starts with two or more
/// blank lines (e.g. after `+++` frontmatter) emits a leading blank on the
/// first round that the second round drops — non-idempotent.
#[must_use]
pub fn at_file_start(trivia: &[HirTrivia]) -> &[HirTrivia] {
    let mut count = 0;
    for item in trivia {
        match item {
            HirTrivia::Newline { .. } => count += 1,
            HirTrivia::CommentLine { .. } => break,
        }
    }
    &trivia[count..]
}

/// Strip one trailing boundary newline when a re-emitted annotation sequence
/// follows the trivia slot.
///
/// The parser attaches the annotation line's own terminating newline to the
/// following construct's leading trivia (annotation-path trivia combining);
/// the annotation re-emitter writes that line break itself. Stripping keeps
/// the boundary single so rendering stays idempotent.
#[must_use]
pub fn without_following_annotation(trivia: &[HirTrivia]) -> &[HirTrivia] {
    match trivia.last() {
        Some(HirTrivia::Newline { .. }) => &trivia[..trivia.len() - 1],
        _ => trivia,
    }
}

/// Emit one attachment's trivia in source order, with boundary-newline
/// normalization.
///
/// Every render slot is preceded by the emitter's own statement/construct
/// newline, so the first stored boundary `Newline` would double the line break
/// and is dropped. Remaining newlines are intentional blank lines. A comment's
/// own terminating newline is written with the comment and absorbed, so a
/// following stored `Newline` does not add a second line break.
pub fn write_leading_trivia(trivia: &[HirTrivia], interner: &Interner, writer: &mut CodeWriter) {
    let trivia = match trivia.first() {
        Some(HirTrivia::Newline { .. }) => &trivia[1..],
        _ => trivia,
    };
    let mut pending_blank_lines = 0usize;
    let mut after_comment = false;
    for item in trivia {
        match item {
            HirTrivia::CommentLine { text, .. } => {
                for _ in 0..pending_blank_lines {
                    writer.newline();
                }
                pending_blank_lines = 0;
                writer.write("#");
                writer.write(interner.resolve(*text));
                writer.newline();
                after_comment = true;
            }
            HirTrivia::Newline { .. } => {
                if after_comment {
                    // The comment's own line terminator; already written.
                    after_comment = false;
                    continue;
                }
                pending_blank_lines += 1;
            }
        }
    }
    for _ in 0..pending_blank_lines {
        writer.newline();
    }
}
