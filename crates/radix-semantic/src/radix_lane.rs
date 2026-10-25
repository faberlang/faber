//! Compiler-owned `@ radix lane ...` metadata collection.
//!
//! This pass interprets the token-backed `@ radix` annotation namespace after
//! declaration collection has assigned stable `DefId`s, but before HIR/MIR/AIR
//! routing decisions. The output is a semantic side table keyed by function
//! identity so downstream phases never re-parse raw annotation tokens.

use crate::passes::companion_predeclare::CompanionPredeclarations;
use crate::{Resolver, SemanticError, SemanticErrorKind};
use handroll::FxHashMap;
use radix_hir::DefId;
use radix_lexer::{Interner, Span, TokenKind};
use radix_syntax::{
    Annotation, AnnotationKind, AnnotationView, ClassMemberKind, InvalidAnnotationView, Program,
    StmtKind, braced_radix_lane_view_strict, braced_radix_typus_view_strict,
};

/// Compiler lane requested by `@ radix lane "..."`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RadixLane {
    Air,
    Mir,
    HirDirect,
}

impl RadixLane {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Air => "air",
            Self::Mir => "mir",
            Self::HirDirect => "hir-direct",
        }
    }
}

/// Backward companion annotation from `@ radix backward "name"`.
///
/// Records the named companion function that the generated-gradient pipeline
/// will produce for the annotated primal. Valid only when paired with
/// [`RadixLane::Air`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadixBackward {
    /// User-supplied symbol name for the generated backward function.
    pub companion_name: radix_lexer::Symbol,
    /// Allocated definition identity for the companion function.
    pub companion_def_id: DefId,
    /// Explicit trainable inputs named by the annotation; `None` selects every
    /// tensor input.
    pub selected_params: Option<Vec<radix_lexer::Symbol>>,
}

/// Semantic lane and backward facts keyed to function definitions.
#[derive(Debug, Clone, Default)]
pub struct RadixLaneMetadata {
    lanes: FxHashMap<DefId, RadixLane>,
    backward: FxHashMap<DefId, RadixBackward>,
    /// Monotonic source for companion `DefId` allocation.
    next_companion_id: u32,
}

impl RadixLaneMetadata {
    pub fn insert(&mut self, def_id: DefId, lane: RadixLane) {
        self.lanes.insert(def_id, lane);
    }

    #[must_use]
    pub fn lane_for(&self, def_id: DefId) -> Option<RadixLane> {
        self.lanes.get(&def_id).copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = (DefId, RadixLane)> + '_ {
        self.lanes.iter().map(|(&def_id, &lane)| (def_id, lane))
    }

    /// Deterministic lane snapshot for analyzed-HIR artifacts. Backward rows
    /// are joined here because a rehydrated AIR unit needs both the lane and
    /// its companion identity before it can run the concrete AIR pipeline.
    #[must_use]
    pub fn snapshot(&self) -> Vec<(DefId, RadixLane, Option<RadixBackward>)> {
        let mut entries = self
            .lanes
            .iter()
            .map(|(&def_id, &lane)| (def_id, lane, self.backward.get(&def_id).cloned()))
            .collect::<Vec<_>>();
        entries.sort_by_key(|(def_id, _, _)| def_id.0);
        entries
    }

    /// Rebuild lane metadata from the portable snapshot representation.
    pub fn from_snapshot(
        entries: impl IntoIterator<Item = (DefId, RadixLane, Option<RadixBackward>)>,
    ) -> Self {
        let mut metadata = Self::default();
        for (def_id, lane, backward) in entries {
            metadata.insert(def_id, lane);
            if let Some(backward) = backward {
                metadata.insert_backward(def_id, backward);
            }
        }
        metadata
    }

    /// Record a backward annotation for a function definition.
    pub fn insert_backward(&mut self, def_id: DefId, mut backward: RadixBackward) {
        // Allocate a fresh DefId for the companion if one hasn't been set.
        if backward.companion_def_id == DefId::INVALID {
            let companion_id = self.allocate_companion_def_id();
            backward.companion_def_id = companion_id;
        }
        self.backward.insert(def_id, backward);
    }

    /// Allocate a fresh `DefId` for a companion function.
    fn allocate_companion_def_id(&mut self) -> DefId {
        let id = DefId(self.next_companion_id | 0x8000_0000);
        self.next_companion_id = self.next_companion_id.wrapping_add(1);
        id
    }

    /// Seed the companion-id counter so it continues past a pre-resolve
    /// allocation batch without collision.
    pub fn set_next_companion_id(&mut self, id: u32) {
        self.next_companion_id = id;
    }

    /// Look up the backward annotation for a function, if any.
    #[must_use]
    pub fn backward_for(&self, def_id: DefId) -> Option<&RadixBackward> {
        self.backward.get(&def_id)
    }

    /// Iterate over all backward-annotated function definitions.
    pub fn iter_backward(&self) -> impl Iterator<Item = (DefId, &RadixBackward)> + '_ {
        self.backward
            .iter()
            .map(|(&def_id, backward)| (def_id, backward))
    }

    /// Whether any function in this unit has a backward annotation.
    #[must_use]
    pub fn has_backward(&self) -> bool {
        !self.backward.is_empty()
    }
}

/// Collect `@ radix lane` and `@ radix backward` annotations from the parsed AST.
///
/// Pre-allocated companion identities from
/// [`CompanionPredeclarations`] are consumed idempotently: when the
/// companion name matches a pre-allocated entry the pre-existing `DefId`
/// is reused so MIR/driver identity stays stable.
///
/// # Errors
///
/// Returns [`SemanticError`]s for conflicting lanes, malformed annotations,
/// unsupported placements, and backward annotations on non-AIR functions.
pub fn collect(
    program: &Program,
    resolver: &Resolver,
    interner: &Interner,
    companion_predeclarations: &CompanionPredeclarations,
) -> Result<RadixLaneMetadata, Vec<SemanticError>> {
    let mut metadata = RadixLaneMetadata::default();
    metadata.set_next_companion_id(
        companion_predeclarations
            .max_companion_index
            .wrapping_add(1),
    );
    let mut errors = Vec::new();

    for stmt in &program.statements {
        match &stmt.kind {
            StmtKind::Func(decl) => {
                let annotations = stmt
                    .annotations
                    .iter()
                    .chain(decl.annotations.iter())
                    .collect::<Vec<_>>();
                let lane = collect_lane_from_annotations(&annotations, interner, &mut errors);
                let backward =
                    collect_backward_from_annotations(&annotations, interner, &mut errors);

                // Catch-all: validate that every @ radix annotation uses a
                // recognized directive (lane, backward, or braced typus).
                validate_radix_directives_for_annotations(&annotations, interner, &mut errors);
                crate::radix_contract::validate_declaration(
                    &stmt.annotations,
                    false,
                    interner,
                    &mut errors,
                );

                // Backward annotation is valid only when paired with lane "air".
                if backward.is_some() && lane != Some(RadixLane::Air) {
                    errors.push(radix_backward_error(
                        stmt.span,
                        "conflicting_backward_without_air",
                    ));
                }

                if let Some(def_id) = resolver.lookup(decl.name.name) {
                    if let Some(l) = lane {
                        metadata.insert(def_id, l);
                    }
                    if let Some(b) = backward
                        && lane == Some(RadixLane::Air)
                    {
                        // Consume pre-allocated DefId from the pre-resolve
                        // pass so MIR/driver identity stays stable.
                        let bw = if let Some(&pre_def_id) =
                            companion_predeclarations.by_name.get(&b.companion_name)
                        {
                            RadixBackward {
                                companion_name: b.companion_name,
                                companion_def_id: pre_def_id,
                                selected_params: b.selected_params.clone(),
                            }
                        } else {
                            b
                        };
                        metadata.insert_backward(def_id, bw);
                    }
                }
            }
            StmtKind::Class(decl) => {
                collect_class_annotations(stmt, decl, interner, &mut errors);
            }
            _ => {
                crate::radix_contract::validate_declaration(
                    &stmt.annotations,
                    matches!(stmt.kind, StmtKind::Interface(_)),
                    interner,
                    &mut errors,
                );
                reject_unsupported_radix_annotations(
                    &stmt.annotations,
                    interner,
                    "declaration",
                    &mut errors,
                );
                reject_unsupported_radix_backward_annotations(
                    &stmt.annotations,
                    interner,
                    "declaration",
                    &mut errors,
                );
            }
        }
    }

    if errors.is_empty() {
        Ok(metadata)
    } else {
        Err(errors)
    }
}

fn collect_class_annotations(
    stmt: &radix_syntax::Stmt,
    decl: &radix_syntax::ClassDecl,
    interner: &Interner,
    errors: &mut Vec<SemanticError>,
) {
    crate::radix_contract::validate_declaration(&stmt.annotations, false, interner, errors);
    reject_unsupported_radix_annotations(&stmt.annotations, interner, "genus declaration", errors);
    reject_unsupported_radix_backward_annotations(
        &stmt.annotations,
        interner,
        "genus declaration",
        errors,
    );
    for member in &decl.members {
        reject_unsupported_radix_annotations(&member.annotations, interner, "genus member", errors);
        reject_unsupported_radix_backward_annotations(
            &member.annotations,
            interner,
            "genus member",
            errors,
        );
        if let ClassMemberKind::Method(method) = &member.kind {
            reject_unsupported_radix_annotations(
                &method.annotations,
                interner,
                "genus method",
                errors,
            );
            reject_unsupported_radix_backward_annotations(
                &method.annotations,
                interner,
                "genus method",
                errors,
            );
        }
    }
}

pub(crate) fn collect_lane_from_annotations(
    annotations: &[&Annotation],
    interner: &Interner,
    errors: &mut Vec<SemanticError>,
) -> Option<RadixLane> {
    let mut lane: Option<RadixLane> = None;
    let mut invalid = false;

    for annotation in annotations {
        if let Some(error) = invalid_radix_typus_annotation(annotation, interner) {
            invalid = true;
            errors.push(error);
            continue;
        }
        match lane_from_annotation(annotation, interner) {
            LaneParse::Parsed(parsed) => {
                if let Some(existing) = lane {
                    errors.push(
                        radix_lane_error(annotation.span, "conflicting_lane")
                            .with_arg("existing", existing.as_str())
                            .with_arg("parsed", parsed.as_str()),
                    );
                } else {
                    lane = Some(parsed);
                }
            }
            LaneParse::Error(error) => {
                invalid = true;
                errors.push(error);
            }
            LaneParse::Skip => {}
        }
    }

    if invalid {
        // Keep collecting every malformed annotation, but do not expose partial
        // metadata from a declaration whose directive set is invalid.
        None
    } else {
        lane
    }
}

fn reject_unsupported_radix_annotations(
    annotations: &[Annotation],
    interner: &Interner,
    placement: &str,
    errors: &mut Vec<SemanticError>,
) {
    for annotation in annotations {
        if is_radix_lane_annotation(annotation, interner) {
            errors.push(
                radix_lane_error(annotation.span, "unsupported_placement")
                    .with_arg("placement", placement),
            );
        }
    }
}

enum LaneParse {
    Parsed(RadixLane),
    Error(SemanticError),
    Skip,
}

fn lane_from_annotation(annotation: &Annotation, interner: &Interner) -> LaneParse {
    match &annotation.kind {
        AnnotationKind::Statement(stmt) => {
            match radix_lane_statement(annotation.span, stmt, interner) {
                StmtLaneParse::Parsed(view) => {
                    parse_radix_lane_value(view.lane, view.span, interner).into()
                }
                StmtLaneParse::Error(error) => LaneParse::Error(error),
                StmtLaneParse::Skip => LaneParse::Skip,
            }
        }
        AnnotationKind::Braced(braced) => match braced_radix_lane_view_strict(braced, interner) {
            AnnotationView::Valid(view) => {
                parse_radix_lane_value(view.lane, view.span, interner).into()
            }
            AnnotationView::Malformed(error) => LaneParse::Error(invalid_annotation_error(error)),
            AnnotationView::Absent => LaneParse::Skip,
        },
        AnnotationKind::RadixLane(lane) => {
            parse_radix_lane_value(lane.lane, annotation.span, interner).into()
        }
        _ => LaneParse::Skip,
    }
}

fn invalid_radix_typus_annotation(
    annotation: &Annotation,
    interner: &Interner,
) -> Option<SemanticError> {
    let AnnotationKind::Braced(braced) = &annotation.kind else {
        return None;
    };
    match braced_radix_typus_view_strict(braced, interner) {
        AnnotationView::Valid(_) | AnnotationView::Absent => None,
        AnnotationView::Malformed(error) => Some(invalid_annotation_error(error)),
    }
}

enum StmtLaneParse {
    Parsed(radix_syntax::RadixLaneView),
    Error(SemanticError),
    Skip,
}

fn radix_lane_statement(
    annotation_span: Span,
    stmt: &radix_syntax::AnnotationStmt,
    interner: &Interner,
) -> StmtLaneParse {
    if interner.resolve(stmt.name.name) != "radix" {
        return StmtLaneParse::Skip;
    }
    let Some(directive) = stmt.args.first() else {
        return StmtLaneParse::Error(radix_annotation_error(
            annotation_span,
            "missing_directive",
            "@ radix",
        ));
    };
    let TokenKind::Ident(symbol) = directive.kind else {
        return StmtLaneParse::Error(radix_annotation_error(
            directive.span,
            "unknown_directive",
            "@ radix",
        ));
    };
    if interner.resolve(symbol) != "lane" {
        // Not a lane directive — silently skip so other parsers (e.g. backward)
        // can handle this annotation. Unknown directives are caught by the
        // @ radix annotation-level validation.
        return StmtLaneParse::Skip;
    }
    let Some(value) = stmt.args.get(1) else {
        return StmtLaneParse::Error(radix_lane_error(annotation_span, "missing_lane_value"));
    };
    if stmt.args.len() > 2 {
        return StmtLaneParse::Error(radix_lane_error(value.span, "non_string_lane_value"));
    }
    match value.kind {
        TokenKind::String(lane) => StmtLaneParse::Parsed(radix_syntax::RadixLaneView {
            lane,
            span: value.span,
        }),
        _ => StmtLaneParse::Error(radix_lane_error(value.span, "non_string_lane_value")),
    }
}

fn is_radix_lane_annotation(annotation: &Annotation, interner: &Interner) -> bool {
    matches!(
        lane_from_annotation(annotation, interner),
        LaneParse::Parsed(_) | LaneParse::Error(_)
    )
}

impl From<Result<RadixLane, SemanticError>> for LaneParse {
    fn from(result: Result<RadixLane, SemanticError>) -> Self {
        match result {
            Ok(parsed) => LaneParse::Parsed(parsed),
            Err(error) => LaneParse::Error(error),
        }
    }
}

fn parse_radix_lane_value(
    lane: radix_lexer::Symbol,
    span: Span,
    interner: &Interner,
) -> Result<RadixLane, SemanticError> {
    match interner.resolve(lane) {
        "air" => Ok(RadixLane::Air),
        "mir" => Ok(RadixLane::Mir),
        "hir-direct" => Ok(RadixLane::HirDirect),
        other => Err(radix_lane_error(span, "unknown_lane").with_arg("lane", other)),
    }
}

fn radix_lane_error(span: Span, issue: &'static str) -> SemanticError {
    radix_annotation_error(span, issue, "@ radix lane")
}

fn radix_backward_error(span: Span, issue: &'static str) -> SemanticError {
    radix_annotation_error(span, issue, "@ radix backward")
}

fn invalid_annotation_error(error: InvalidAnnotationView) -> SemanticError {
    radix_annotation_error(error.span, error.issue, error.annotation)
}

fn radix_annotation_error(
    span: Span,
    issue: &'static str,
    annotation: &'static str,
) -> SemanticError {
    SemanticError::structured(SemanticErrorKind::AnnotationInvalid, span)
        .with_arg("issue", issue)
        .with_arg("annotation", annotation)
        .with_arg(
            "radix_typus_braced",
            "@ radix { param = T, allowed = textus }",
        )
}

// ---------------------------------------------------------------------------
// Backward annotation parsing
// ---------------------------------------------------------------------------

/// Parse `@ radix backward "name"` annotations from a function's annotation set.
///
/// Iterates all annotations looking for statement-form `@ radix backward "name"`
/// annotations. Returns `None` when no backward annotation is present. Returns
/// `Some(RadixBackward)` when exactly one valid backward annotation is found.
/// Malformed backward annotations (non-string value, missing value) push errors
/// into `errors` and return `None`.
pub(crate) fn collect_backward_from_annotations(
    annotations: &[&Annotation],
    interner: &Interner,
    errors: &mut Vec<SemanticError>,
) -> Option<RadixBackward> {
    let mut backward: Option<RadixBackward> = None;

    for annotation in annotations {
        match backward_from_annotation(annotation, interner) {
            BackwardParse::Parsed(parsed) => {
                if backward.is_some() {
                    errors.push(radix_backward_error(
                        annotation.span,
                        "conflicting_backward",
                    ));
                } else {
                    backward = Some(parsed);
                }
            }
            BackwardParse::Error(error) => {
                errors.push(error);
            }
            BackwardParse::Skip => {}
        }
    }

    backward
}

enum BackwardParse {
    Parsed(RadixBackward),
    Error(SemanticError),
    Skip,
}

fn backward_from_annotation(annotation: &Annotation, interner: &Interner) -> BackwardParse {
    match &annotation.kind {
        AnnotationKind::Statement(stmt) => {
            match radix_backward_statement(annotation.span, stmt, interner) {
                Ok(parsed) => BackwardParse::Parsed(parsed),
                Err(error) => {
                    // Annotations not in the `@ radix` family produce
                    // `unknown_annotation_family`. `@ radix` annotations with
                    // a directive other than `backward` produce
                    // `unknown_directive`. Both are non-error skips so other
                    // annotation parsers can handle them.
                    let is_skip = error.args.iter().any(|arg| {
                        arg.name == "issue"
                            && (arg.value == "unknown_directive"
                                || arg.value == "unknown_annotation_family")
                    });
                    if is_skip {
                        BackwardParse::Skip
                    } else {
                        BackwardParse::Error(error)
                    }
                }
            }
        }
        // Braced and other annotation forms are not valid for backward.
        _ => BackwardParse::Skip,
    }
}

/// Parse a `@ radix backward "name"` statement annotation.
///
/// Expects exactly three tokens: `radix`, `backward`, `"name"`.
/// Returns `Err` with `unknown_directive` issue when the annotation is not
/// a backward annotation (the caller uses this to distinguish "not backward"
/// from "malformed backward").
fn radix_backward_statement(
    annotation_span: Span,
    stmt: &radix_syntax::AnnotationStmt,
    interner: &Interner,
) -> Result<RadixBackward, SemanticError> {
    if interner.resolve(stmt.name.name) != "radix" {
        return Err(radix_annotation_error(
            annotation_span,
            "unknown_annotation_family",
            "@ radix",
        ));
    }
    let Some(directive) = stmt.args.first() else {
        return Err(radix_backward_error(annotation_span, "missing_directive"));
    };
    let TokenKind::Ident(symbol) = directive.kind else {
        return Err(radix_backward_error(directive.span, "unknown_directive"));
    };
    if interner.resolve(symbol) != "backward" {
        return Err(radix_backward_error(directive.span, "unknown_directive"));
    }
    let Some(value) = stmt.args.get(1) else {
        return Err(radix_backward_error(
            annotation_span,
            "missing_backward_name",
        ));
    };
    if stmt.args.len() > 2 {
        return Err(radix_backward_error(value.span, "extra_backward_args"));
    }
    match value.kind {
        TokenKind::String(name) => Ok(RadixBackward {
            companion_name: name,
            companion_def_id: DefId::INVALID,
            selected_params: None,
        }),
        _ => Err(radix_backward_error(value.span, "non_string_backward_name")),
    }
}

/// Emit unsupported-placement diagnostics for `@ radix backward` annotations
/// found on non-function declarations.
fn reject_unsupported_radix_backward_annotations(
    annotations: &[Annotation],
    interner: &Interner,
    placement: &str,
    errors: &mut Vec<SemanticError>,
) {
    for annotation in annotations {
        if is_radix_backward_annotation(annotation, interner) {
            errors.push(
                radix_backward_error(annotation.span, "unsupported_placement")
                    .with_arg("placement", placement),
            );
        }
    }
}

/// Check whether an annotation is a `@ radix backward "..."` annotation.
///
/// Returns `true` when the annotation parses successfully as a backward
/// annotation, or when it parses to an error (malformed but structurally
/// identified as backward). Returns `false` for non-backward annotations.
fn is_radix_backward_annotation(annotation: &Annotation, interner: &Interner) -> bool {
    matches!(
        backward_from_annotation(annotation, interner),
        BackwardParse::Parsed(_) | BackwardParse::Error(_)
    )
}

// ---------------------------------------------------------------------------
// General radix directive validation
// ---------------------------------------------------------------------------

/// Validate that every `@ radix <directive>` annotation on a set of
/// annotations uses a recognized directive.
///
/// Recognized directives: `lane`, `backward`, and the braced form
/// (`@ radix { ... }`). Unknown directives like `@ radix target "air"`
/// produce an `unknown_directive` error.
fn validate_radix_directives_for_annotations(
    annotations: &[&Annotation],
    interner: &Interner,
    errors: &mut Vec<SemanticError>,
) {
    for annotation in annotations {
        // Only validate statement-form @ radix annotations.
        let AnnotationKind::Statement(stmt) = &annotation.kind else {
            continue;
        };
        if interner.resolve(stmt.name.name) != "radix" {
            continue;
        }
        let Some(directive) = stmt.args.first() else {
            continue;
        };
        let TokenKind::Ident(symbol) = directive.kind else {
            continue;
        };
        let directive_str = interner.resolve(symbol);
        // `contract` is validated by `radix_contract::validate_declaration`.
        if matches!(directive_str, "lane" | "backward" | "contract") {
            continue;
        }
        // Unrecognized directive on @ radix.
        errors.push(radix_annotation_error(
            directive.span,
            "unknown_directive",
            "@ radix",
        ));
    }
}
