//! Annotation contract declarations and application validation.
//!
//! `@ annotatio` marks a genus as a compile-time annotation contract. User code
//! applies contracts as braced annotations (`@ Contract { field = literal }`).
//! This pass builds a registry of contract schemas and validates applications
//! before HIR lowering so consumers never reparse raw annotation tokens.
//!
//! INVARIANTS
//! ==========
//! - Only genera marked `@ annotatio` are contracts; ordinary genera stay data types.
//! - Payload values are compile-time constants (v1 scalars only).
//! - Fail closed on unknown contracts, fields, types, and attachment sites.
//! - No framework/web semantics live here — only generic schema machinery.

use crate::{Resolver, SemanticError, SemanticErrorKind, SymbolKind};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::DefId;
use radix_lexer::{Interner, Span, Symbol};
use radix_syntax::{
    Annotation, AnnotationField, AnnotationFieldValue, AnnotationKind, BracedAnnotation, ClassDecl,
    ClassMemberKind, ExprKind, FieldDecl, Literal, Program, Stmt, StmtKind, TypeExpr, TypeExprKind,
};

/// Compiler-owned seed that marks a genus as an annotation contract.
const FAMILY_ANNOTATIO: &str = "annotatio";

// Structured `issue` args for SEM018 (InvalidAnnotation). Keep stable; catalog templates key on these.
const ISSUE_MISPLACED: &str = "annotatio_misplaced";
const ISSUE_UNKNOWN_TARGET: &str = "annotatio_unknown_target";
const ISSUE_INVALID_FIELD_TYPE: &str = "annotatio_invalid_field_type";
const ISSUE_UNKNOWN_CONTRACT: &str = "annotatio_unknown_contract";
const ISSUE_NOT_A_CONTRACT: &str = "annotatio_not_a_contract";
const ISSUE_UNKNOWN_FIELD: &str = "annotatio_unknown_field";
const ISSUE_DUPLICATE_FIELD: &str = "annotatio_duplicate_field";
const ISSUE_MISSING_FIELD: &str = "annotatio_missing_field";
const ISSUE_INVALID_PAYLOAD: &str = "annotatio_invalid_payload";
const ISSUE_INVALID_ATTACHMENT: &str = "annotatio_invalid_attachment";
const ISSUE_METHOD_UNSUPPORTED: &str = "annotatio_method_unsupported";
const ISSUE_INVALID_MARKER: &str = "annotatio_invalid_marker";
const ISSUE_DUPLICATE_MARKER: &str = "annotatio_duplicate_marker";
const ISSUE_COMMUNE_ON_NON_UNION: &str = "commune_on_non_union";
const ISSUE_DUPLICATE_COMMUNE_FIELD: &str = "duplicate_commune_field";
const ISSUE_VARIANT_REDECLARES_COMMUNE: &str = "union_variant_redeclares_shared_field";
const ISSUE_CONVERSIO_MISPLACED: &str = "union_conversio_misplaced";
const FAMILY_COMMUNE: &str = "commune";

/// Where a contract may be attached in v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationAttachmentTarget {
    Functio,
    /// SPIKE (shader-annotation-stages): a genus field.
    Field,
}

impl AnnotationAttachmentTarget {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Functio => "functio",
            Self::Field => "field",
        }
    }

    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "functio" => Some(Self::Functio),
            "field" => Some(Self::Field),
            _ => None,
        }
    }
}

/// Scalar types allowed on contract fields in v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractFieldType {
    Textus,
    Numerus,
    Fractus,
    Bivalens,
}

impl ContractFieldType {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Textus => "textus",
            Self::Numerus => "numerus",
            Self::Fractus => "fractus",
            Self::Bivalens => "bivalens",
        }
    }

    #[must_use]
    pub fn from_type_name(name: &str) -> Option<Self> {
        match name {
            "textus" => Some(Self::Textus),
            "numerus" => Some(Self::Numerus),
            "fractus" => Some(Self::Fractus),
            "bivalens" => Some(Self::Bivalens),
            _ => None,
        }
    }
}

fn invalid_annotation(span: Span, issue: &'static str) -> SemanticError {
    SemanticError::structured(SemanticErrorKind::AnnotationInvalid, span).with_arg("issue", issue)
}

fn application_key(span: Span) -> (u32, u32) {
    (span.start, span.end)
}

/// One field in a contract schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotationContractField {
    pub name: Symbol,
    pub ty: ContractFieldType,
    pub optional: bool,
    pub span: Span,
}

/// Compile-time annotation contract declared by `@ annotatio` on a genus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnotationContract {
    pub def_id: DefId,
    pub name: Symbol,
    pub target: AnnotationAttachmentTarget,
    pub fields: Vec<AnnotationContractField>,
    pub span: Span,
}

/// Registry of local and imported annotation contracts for one compilation unit.
#[derive(Debug, Clone, Default)]
pub struct AnnotationContractRegistry {
    by_def_id: FxHashMap<DefId, AnnotationContract>,
    by_name: FxHashMap<Symbol, DefId>,
}

impl AnnotationContractRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, contract: AnnotationContract) {
        self.by_name.insert(contract.name, contract.def_id);
        self.by_def_id.insert(contract.def_id, contract);
    }

    #[must_use]
    pub fn get(&self, def_id: DefId) -> Option<&AnnotationContract> {
        self.by_def_id.get(&def_id)
    }

    #[must_use]
    pub fn lookup_name(&self, name: Symbol) -> Option<&AnnotationContract> {
        let def_id = *self.by_name.get(&name)?;
        self.by_def_id.get(&def_id)
    }

    #[must_use]
    pub fn contains_name(&self, name: Symbol) -> bool {
        self.by_name.contains_key(&name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &AnnotationContract> {
        self.by_def_id.values()
    }
}

/// Constant payload value after validation (feeds HIR lowering).
#[derive(Debug, Clone, PartialEq)]
pub enum ContractPayloadValue {
    String(Symbol),
    Int(i64),
    Float(f64),
    Bool(bool),
    Nihil,
}

/// One validated field on a resolved application.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedContractField {
    pub name: Symbol,
    pub value: ContractPayloadValue,
    pub span: Span,
}

/// Validated application of a contract to a declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedContractApplication {
    pub contract_id: DefId,
    pub family: Symbol,
    pub fields: Vec<ResolvedContractField>,
    pub span: Span,
}

/// Full annotation-contract analysis result for one unit.
#[derive(Debug, Clone, Default)]
pub struct AnnotationContractMetadata {
    pub registry: AnnotationContractRegistry,
    /// Applications keyed by annotation span (start, end).
    pub applications: FxHashMap<(u32, u32), ResolvedContractApplication>,
}

impl AnnotationContractMetadata {
    #[must_use]
    pub fn application_at(&self, span: Span) -> Option<&ResolvedContractApplication> {
        self.applications.get(&(span.start, span.end))
    }

    pub fn applications(&self) -> impl Iterator<Item = &ResolvedContractApplication> {
        self.applications.values()
    }
}

/// Collect local contracts and reject misplaced `@ annotatio` markers.
///
/// # Errors
///
/// Returns [`SemanticError`]s for misplaced `@ annotatio` markers and invalid
/// contract field definitions.
pub fn collect_local_contracts(
    program: &Program,
    resolver: &Resolver,
    interner: &Interner,
) -> Result<AnnotationContractRegistry, Vec<SemanticError>> {
    let mut errors = Vec::new();
    let mut registry = AnnotationContractRegistry::new();

    for stmt in &program.statements {
        collect_declaration_contract(stmt, resolver, interner, &mut registry, &mut errors);
        reject_misplaced_annotatio(stmt, interner, &mut errors);
        reject_misplaced_commune(stmt, interner, &mut errors);
        reject_misplaced_conversio(stmt, &mut errors);
        reject_commune_field_uniqueness(stmt, &mut errors);
    }

    if errors.is_empty() {
        Ok(registry)
    } else {
        Err(errors)
    }
}

/// Validate contract applications against a fully populated registry (local + imports).
///
/// # Errors
///
/// Returns [`SemanticError`]s for unknown contracts, unknown fields, duplicate
/// fields, missing required fields, and invalid payload values.
pub fn validate_applications(
    program: &Program,
    registry: &AnnotationContractRegistry,
    resolver: &Resolver,
    interner: &Interner,
) -> Result<FxHashMap<(u32, u32), ResolvedContractApplication>, Vec<SemanticError>> {
    let mut errors = Vec::new();
    let mut applications = FxHashMap::default();
    errors.extend(crate::evm_contract::validate_syntax(program, interner));
    for stmt in &program.statements {
        validate_stmt_applications(
            stmt,
            registry,
            resolver,
            interner,
            &mut applications,
            &mut errors,
        );
    }
    if errors.is_empty() {
        Ok(applications)
    } else {
        Err(errors)
    }
}

/// Collect contracts and validate applications on the AST (local only).
///
/// # Errors
///
/// Returns [`SemanticError`]s from both collection and validation phases.
pub fn analyze(
    program: &Program,
    resolver: &Resolver,
    interner: &Interner,
) -> Result<AnnotationContractMetadata, Vec<SemanticError>> {
    let mut errors = Vec::new();
    let registry = match collect_local_contracts(program, resolver, interner) {
        Ok(registry) => registry,
        Err(e) => {
            errors.extend(e);
            AnnotationContractRegistry::new()
        }
    };

    let applications = match validate_applications(program, &registry, resolver, interner) {
        Ok(applications) => applications,
        Err(e) => {
            errors.extend(e);
            FxHashMap::default()
        }
    };

    if errors.is_empty() {
        Ok(AnnotationContractMetadata {
            registry,
            applications,
        })
    } else {
        Err(errors)
    }
}

/// Install an imported contract under a local binding name.
// public API: consumes Vec into registry
pub fn register_imported_contract(
    registry: &mut AnnotationContractRegistry,
    def_id: DefId,
    name: Symbol,
    target: AnnotationAttachmentTarget,
    fields: Vec<AnnotationContractField>,
    span: Span,
) {
    registry.insert(AnnotationContract {
        def_id,
        name,
        target,
        fields,
        span,
    });
}

fn collect_declaration_contract(
    stmt: &Stmt,
    resolver: &Resolver,
    interner: &Interner,
    registry: &mut AnnotationContractRegistry,
    errors: &mut Vec<SemanticError>,
) {
    let StmtKind::Class(class) = &stmt.kind else {
        return;
    };

    let Some(marker) = find_annotatio_marker(&stmt.annotations, interner, errors) else {
        return;
    };

    let Some(def_id) = resolver.lookup(class.name.name) else {
        return;
    };

    let fields = contract_fields_from_class(class, interner, errors);
    registry.insert(AnnotationContract {
        def_id,
        name: class.name.name,
        target: marker.target,
        fields,
        span: stmt.span,
    });
}

struct AnnotatioMarker {
    target: AnnotationAttachmentTarget,
}

fn find_annotatio_marker(
    annotations: &[Annotation],
    interner: &Interner,
    errors: &mut Vec<SemanticError>,
) -> Option<AnnotatioMarker> {
    let mut found = None;
    for annotation in annotations {
        if !is_annotatio_family(annotation, interner) {
            continue;
        }
        if found.is_some() {
            errors.push(invalid_annotation(annotation.span, ISSUE_DUPLICATE_MARKER));
            continue;
        }
        let Some(target) = parse_annotatio_target(annotation, interner, errors) else {
            continue;
        };
        found = Some(AnnotatioMarker { target });
    }
    found
}

fn is_annotatio_family(annotation: &Annotation, interner: &Interner) -> bool {
    match &annotation.kind {
        AnnotationKind::Braced(braced) => interner.resolve(braced.name.name) == FAMILY_ANNOTATIO,
        AnnotationKind::Statement(stmt) => interner.resolve(stmt.name.name) == FAMILY_ANNOTATIO,
        _ => false,
    }
}

fn parse_annotatio_target(
    annotation: &Annotation,
    interner: &Interner,
    errors: &mut Vec<SemanticError>,
) -> Option<AnnotationAttachmentTarget> {
    match &annotation.kind {
        AnnotationKind::Statement(stmt) if stmt.args.is_empty() => {
            Some(AnnotationAttachmentTarget::Functio)
        }
        AnnotationKind::Braced(braced) => {
            Some(parse_braced_annotatio_target(braced, interner, errors))
        }
        AnnotationKind::Statement(_) => {
            errors.push(
                invalid_annotation(annotation.span, ISSUE_INVALID_MARKER).with_arg(
                    "detail",
                    "bare @ annotatio must not take positional arguments",
                ),
            );
            None
        }
        _ => None,
    }
}

fn parse_braced_annotatio_target(
    braced: &BracedAnnotation,
    interner: &Interner,
    errors: &mut Vec<SemanticError>,
) -> AnnotationAttachmentTarget {
    if braced.fields.is_empty() {
        return AnnotationAttachmentTarget::Functio;
    }

    let mut target = AnnotationAttachmentTarget::Functio;
    let mut saw_target = false;
    for field in &braced.fields {
        let name = interner.resolve(field.name.name);
        if name != "target" {
            errors.push(
                invalid_annotation(field.span, ISSUE_UNKNOWN_FIELD)
                    .with_arg("field", name)
                    .with_arg("contract", FAMILY_ANNOTATIO),
            );
            continue;
        }
        if saw_target {
            errors.push(
                invalid_annotation(field.span, ISSUE_DUPLICATE_FIELD).with_arg("field", "target"),
            );
            continue;
        }
        saw_target = true;
        match target_from_field_value(&field.value, interner) {
            Some(parsed) => target = parsed,
            None => {
                errors.push(
                    invalid_annotation(field.span, ISSUE_UNKNOWN_TARGET)
                        .with_arg("target", field_value_display(&field.value, interner)),
                );
            }
        }
    }
    target
}

fn target_from_field_value(
    value: &AnnotationFieldValue,
    interner: &Interner,
) -> Option<AnnotationAttachmentTarget> {
    match value {
        AnnotationFieldValue::Expr(expr) => match &expr.kind {
            ExprKind::Ident(ident) => {
                AnnotationAttachmentTarget::parse(interner.resolve(ident.name))
            }
            ExprKind::Literal(Literal::String(sym)) => {
                AnnotationAttachmentTarget::parse(interner.resolve(*sym))
            }
            _ => None,
        },
        AnnotationFieldValue::Type(ty) => match &ty.kind {
            TypeExprKind::Named(name, params) if params.is_empty() => {
                AnnotationAttachmentTarget::parse(interner.resolve(name.name))
            }
            _ => None,
        },
    }
}

fn field_value_display(value: &AnnotationFieldValue, interner: &Interner) -> String {
    match value {
        AnnotationFieldValue::Expr(expr) => match &expr.kind {
            ExprKind::Ident(ident) => interner.resolve(ident.name).to_owned(),
            ExprKind::Literal(Literal::String(sym)) => interner.resolve(*sym).to_owned(),
            _ => "expression".to_owned(),
        },
        AnnotationFieldValue::Type(_) => "type".to_owned(),
    }
}

fn contract_fields_from_class(
    class: &ClassDecl,
    interner: &Interner,
    errors: &mut Vec<SemanticError>,
) -> Vec<AnnotationContractField> {
    let mut fields = Vec::new();
    for member in &class.members {
        match &member.kind {
            ClassMemberKind::Field(field) => {
                if let Some(contract_field) = contract_field_from_decl(field, interner, errors) {
                    fields.push(contract_field);
                }
            }
            ClassMemberKind::Method(method) => {
                errors.push(
                    invalid_annotation(method.name.span, ISSUE_METHOD_UNSUPPORTED)
                        .with_arg("method", interner.resolve(method.name.name)),
                );
            }
        }
    }
    fields
}

fn contract_field_from_decl(
    field: &FieldDecl,
    interner: &Interner,
    errors: &mut Vec<SemanticError>,
) -> Option<AnnotationContractField> {
    let (ty, optional_from_union) = parse_contract_field_type(&field.ty, interner);
    let Some(ty) = ty else {
        errors.push(
            invalid_annotation(field.ty.span, ISSUE_INVALID_FIELD_TYPE)
                .with_arg("field", interner.resolve(field.name.name)),
        );
        return None;
    };
    Some(AnnotationContractField {
        name: field.name.name,
        ty,
        optional: field.sponte || optional_from_union || field.init.is_some(),
        span: field.name.span,
    })
}

fn parse_contract_field_type(
    ty: &TypeExpr,
    interner: &Interner,
) -> (Option<ContractFieldType>, bool) {
    match &ty.kind {
        TypeExprKind::Named(name, params) if params.is_empty() => (
            ContractFieldType::from_type_name(interner.resolve(name.name)),
            false,
        ),
        TypeExprKind::Union(members) => {
            let mut core = None;
            let mut saw_nihil = false;
            for member in members {
                match &member.kind {
                    TypeExprKind::Named(name, params) if params.is_empty() => {
                        let text = interner.resolve(name.name);
                        if text == "nihil" {
                            saw_nihil = true;
                        } else if let Some(parsed) = ContractFieldType::from_type_name(text) {
                            if core.is_some() {
                                return (None, false);
                            }
                            core = Some(parsed);
                        } else {
                            return (None, false);
                        }
                    }
                    _ => return (None, false),
                }
            }
            (core, saw_nihil)
        }
        _ => (None, false),
    }
}

fn annotation_is_commune(annotation: &Annotation, interner: &Interner) -> bool {
    matches!(
        &annotation.kind,
        AnnotationKind::Statement(stmt)
            if stmt.args.is_empty() && interner.resolve(stmt.name.name) == FAMILY_COMMUNE
    )
}

fn reject_commune_on_list(
    annotations: &[Annotation],
    interner: &Interner,
    errors: &mut Vec<SemanticError>,
) {
    for annotation in annotations {
        if annotation_is_commune(annotation, interner) {
            errors.push(
                invalid_annotation(annotation.span, ISSUE_COMMUNE_ON_NON_UNION)
                    .with_arg("annotation", "@ commune"),
            );
        }
    }
}

fn reject_misplaced_commune(stmt: &Stmt, interner: &Interner, errors: &mut Vec<SemanticError>) {
    reject_commune_on_list(&stmt.annotations, interner, errors);
    match &stmt.kind {
        StmtKind::Func(func) => reject_commune_on_list(&func.annotations, interner, errors),
        StmtKind::Class(class) => {
            for member in &class.members {
                reject_commune_on_list(&member.annotations, interner, errors);
                if let ClassMemberKind::Method(method) = &member.kind {
                    reject_commune_on_list(&method.annotations, interner, errors);
                }
            }
        }
        StmtKind::Interface(iface) => {
            for method in &iface.methods {
                reject_commune_on_list(&method.annotations, interner, errors);
            }
        }
        _ => {}
    }
}

fn annotation_is_conversio(annotation: &Annotation) -> bool {
    matches!(&annotation.kind, AnnotationKind::Conversio)
}

/// `@ conversio` is legal only on a standalone function declaration prefix
/// (the parser enforces the prefix shape); every other placement fails closed.
fn reject_conversio_on_list(
    annotations: &[Annotation],
    function_site: bool,
    errors: &mut Vec<SemanticError>,
) {
    for annotation in annotations {
        if annotation_is_conversio(annotation) && !function_site {
            errors.push(
                invalid_annotation(annotation.span, ISSUE_CONVERSIO_MISPLACED)
                    .with_arg("annotation", "@ conversio"),
            );
        }
    }
}

fn reject_misplaced_conversio(stmt: &Stmt, errors: &mut Vec<SemanticError>) {
    let on_function = matches!(stmt.kind, StmtKind::Func(_));
    reject_conversio_on_list(&stmt.annotations, on_function, errors);
    match &stmt.kind {
        StmtKind::Func(func) => reject_conversio_on_list(&func.annotations, true, errors),
        StmtKind::Class(class) => {
            for member in &class.members {
                reject_conversio_on_list(&member.annotations, false, errors);
                if let ClassMemberKind::Method(method) = &member.kind {
                    reject_conversio_on_list(&method.annotations, false, errors);
                }
            }
        }
        StmtKind::Interface(iface) => {
            for method in &iface.methods {
                reject_conversio_on_list(&method.annotations, false, errors);
            }
        }
        _ => {}
    }
}

fn reject_commune_field_uniqueness(stmt: &Stmt, errors: &mut Vec<SemanticError>) {
    let StmtKind::Union(union) = &stmt.kind else {
        return;
    };
    let mut seen = FxHashSet::default();
    for field in &union.shared_fields {
        if !seen.insert(field.field.name.name) {
            errors.push(
                SemanticError::structured(
                    SemanticErrorKind::DefinitionDuplicate,
                    field.field.name.span,
                )
                .with_arg("issue", ISSUE_DUPLICATE_COMMUNE_FIELD),
            );
        }
    }
    for variant in &union.variants {
        for field in &variant.fields {
            if seen.contains(&field.name.name) {
                errors.push(
                    SemanticError::structured(
                        SemanticErrorKind::DefinitionDuplicate,
                        field.name.span,
                    )
                    .with_arg("issue", ISSUE_VARIANT_REDECLARES_COMMUNE),
                );
            }
        }
    }
}

fn reject_misplaced_annotatio(stmt: &Stmt, interner: &Interner, errors: &mut Vec<SemanticError>) {
    if matches!(stmt.kind, StmtKind::Class(_)) {
        // Allowed on the genus itself; still reject on members below.
        reject_member_annotatio(stmt, interner, errors);
        return;
    }

    reject_annotatio_on_list(
        &stmt.annotations,
        stmt_placement_name(&stmt.kind),
        interner,
        errors,
    );

    if let StmtKind::Func(func) = &stmt.kind {
        reject_annotatio_on_list(&func.annotations, "functio", interner, errors);
    }
}

fn reject_member_annotatio(stmt: &Stmt, interner: &Interner, errors: &mut Vec<SemanticError>) {
    let StmtKind::Class(class) = &stmt.kind else {
        return;
    };
    for member in &class.members {
        reject_annotatio_on_list(&member.annotations, "genus member", interner, errors);
        if let ClassMemberKind::Method(method) = &member.kind {
            reject_annotatio_on_list(&method.annotations, "genus method", interner, errors);
        }
    }
}

fn reject_annotatio_on_list(
    annotations: &[Annotation],
    placement: &str,
    interner: &Interner,
    errors: &mut Vec<SemanticError>,
) {
    for annotation in annotations {
        if is_annotatio_family(annotation, interner) {
            errors.push(
                invalid_annotation(annotation.span, ISSUE_MISPLACED)
                    .with_arg("placement", placement),
            );
        }
    }
}

fn stmt_placement_name(kind: &StmtKind) -> &'static str {
    match kind {
        StmtKind::Func(_) => "functio",
        StmtKind::Var(_) => "variable",
        StmtKind::Interface(_) => "implendum",
        StmtKind::TypeAlias(_) => "typus",
        StmtKind::Enum(_) => "ordo",
        StmtKind::Union(_) => "discretio",
        StmtKind::Import(_) => "importa",
        StmtKind::Class(_) => "genus",
        _ => "declaration",
    }
}

fn validate_stmt_applications(
    stmt: &Stmt,
    registry: &AnnotationContractRegistry,
    resolver: &Resolver,
    interner: &Interner,
    applications: &mut FxHashMap<(u32, u32), ResolvedContractApplication>,
    errors: &mut Vec<SemanticError>,
) {
    match &stmt.kind {
        StmtKind::Func(func) => {
            let annotations = stmt.annotations.iter().chain(func.annotations.iter());
            let mut context = AttachmentValidationContext {
                site: AnnotationAttachmentTarget::Functio,
                site_allowed: true,
                registry,
                resolver,
                interner,
                applications,
                errors,
            };
            validate_attachment_site(annotations, &mut context);
        }
        StmtKind::Class(class) => {
            // Skip @ annotatio marker; other applications are invalid on genus in v1.
            for annotation in &stmt.annotations {
                if is_annotatio_family(annotation, interner) {
                    continue;
                }
                let mut context = AttachmentValidationContext {
                    site: AnnotationAttachmentTarget::Functio,
                    site_allowed: false,
                    registry,
                    resolver,
                    interner,
                    applications,
                    errors,
                };
                try_validate_application(annotation, &mut context);
            }
            for member in &class.members {
                if let ClassMemberKind::Method(method) = &member.kind {
                    let annotations = member.annotations.iter().chain(method.annotations.iter());
                    let mut context = AttachmentValidationContext {
                        site: AnnotationAttachmentTarget::Functio,
                        site_allowed: true,
                        registry,
                        resolver,
                        interner,
                        applications,
                        errors,
                    };
                    validate_attachment_site(annotations, &mut context);
                } else {
                    for annotation in &member.annotations {
                        let mut context = AttachmentValidationContext {
                            site: AnnotationAttachmentTarget::Field,
                            site_allowed: true,
                            registry,
                            resolver,
                            interner,
                            applications,
                            errors,
                        };
                        try_validate_application(annotation, &mut context);
                    }
                }
            }
        }
        _ => {
            for annotation in &stmt.annotations {
                let mut context = AttachmentValidationContext {
                    site: AnnotationAttachmentTarget::Functio,
                    site_allowed: false,
                    registry,
                    resolver,
                    interner,
                    applications,
                    errors,
                };
                try_validate_application(annotation, &mut context);
            }
        }
    }
}

struct AttachmentValidationContext<'a> {
    site: AnnotationAttachmentTarget,
    site_allowed: bool,
    registry: &'a AnnotationContractRegistry,
    resolver: &'a Resolver,
    interner: &'a Interner,
    applications: &'a mut FxHashMap<(u32, u32), ResolvedContractApplication>,
    errors: &'a mut Vec<SemanticError>,
}

fn validate_attachment_site<'a>(
    annotations: impl Iterator<Item = &'a Annotation>,
    context: &mut AttachmentValidationContext<'_>,
) {
    for annotation in annotations {
        try_validate_application(annotation, context);
    }
}

fn try_validate_application(
    annotation: &Annotation,
    context: &mut AttachmentValidationContext<'_>,
) {
    if crate::evm_contract::is_contract_annotation(annotation, context.interner) {
        // The compiler owns this family. Its class attachment and environment
        // payload are validated separately from user-defined schemas.
        return;
    }
    let AnnotationKind::Braced(braced) = &annotation.kind else {
        // Bare `@ Contract` without braces: allow only when registered and all
        // fields optional. Unknown bare names fail closed (never a silent pass)
        // unless they name a compiler-owned family.
        if let AnnotationKind::Statement(stmt) = &annotation.kind {
            if let Some(contract) = context.registry.lookup_name(stmt.name.name) {
                if !context.site_allowed || contract.target != context.site {
                    context
                        .errors
                        .push(invalid_attachment_error(annotation.span, contract));
                    return;
                }
                if let Some(resolved) = validate_payload(
                    contract,
                    &[],
                    annotation.span,
                    context.interner,
                    context.errors,
                ) {
                    context
                        .applications
                        .insert(application_key(annotation.span), resolved);
                }
            } else {
                let family_name = context.interner.resolve(stmt.name.name);
                if !is_compiler_owned_family(family_name) {
                    let issue = if genus_is_non_contract(
                        context.resolver,
                        context.registry,
                        stmt.name.name,
                    ) {
                        ISSUE_NOT_A_CONTRACT
                    } else {
                        ISSUE_UNKNOWN_CONTRACT
                    };
                    context.errors.push(
                        invalid_annotation(annotation.span, issue).with_arg("name", family_name),
                    );
                }
            }
        }
        // Structured compiler-owned kinds (Cli, Optio, Futura, …) are their own
        // `AnnotationKind` variants and pass through unvalidated.
        return;
    };

    let family_name = context.interner.resolve(braced.name.name);
    if is_compiler_owned_family(family_name) {
        return;
    }

    let Some(contract) = context.registry.lookup_name(braced.name.name) else {
        let issue = if genus_is_non_contract(context.resolver, context.registry, braced.name.name) {
            ISSUE_NOT_A_CONTRACT
        } else {
            ISSUE_UNKNOWN_CONTRACT
        };
        context
            .errors
            .push(invalid_annotation(annotation.span, issue).with_arg("name", family_name));
        return;
    };

    if !context.site_allowed || contract.target != context.site {
        context
            .errors
            .push(invalid_attachment_error(annotation.span, contract));
        return;
    }

    if let Some(resolved) = validate_payload(
        contract,
        &braced.fields,
        annotation.span,
        context.interner,
        context.errors,
    ) {
        context
            .applications
            .insert(application_key(annotation.span), resolved);
    }
}

fn invalid_attachment_error(span: Span, contract: &AnnotationContract) -> SemanticError {
    invalid_annotation(span, ISSUE_INVALID_ATTACHMENT)
        .with_arg("expected", contract.target.as_str())
}

fn is_compiler_owned_family(name: &str) -> bool {
    matches!(
        name,
        FAMILY_ANNOTATIO
            | "cli"
            | "imperium"
            | "optio"
            | "operandus"
            | "futura"
            | "cursor"
            | "tag"
            | "solum"
            | "omitte"
            | "metior"
            | "publica"
            | "protecta"
            | "privata"
            | "interna"
            | "radix"
            | "nondum"
            | "verte"
            | "nucleum"
            | FAMILY_COMMUNE
            // CLI/JSON metadata families consumed by lowering (corpus + exempla).
            | "alias"
            | "descriptio"
            | "imperia"
            | "json"
            | "versio"
            // Shader families. External-binding spellings are absent from
            // this allowlist, so active references are rejected.
            | "vertex"
            | "fragment"
            | "shader_contract"
            | "pure"
            // Declaration/fixture metadata families (grammatica-nova fixture).
            | "nomen"
            | "tabulae"
            | "latitudo"
            | "indentum"
            | "ordinatio"
            | "separaGroups"
            | "bracchiae"
            | "methodiSeparatio"
            | "meta"
    )
}

fn validate_payload(
    contract: &AnnotationContract,
    fields: &[AnnotationField],
    span: Span,
    interner: &Interner,
    errors: &mut Vec<SemanticError>,
) -> Option<ResolvedContractApplication> {
    let mut resolved_fields = Vec::new();
    let mut seen = FxHashMap::default();
    let mut ok = true;

    for field in fields {
        let field_name = field.name.name;
        if seen.insert(field_name, field.span).is_some() {
            errors.push(
                invalid_annotation(field.span, ISSUE_DUPLICATE_FIELD)
                    .with_arg("field", interner.resolve(field_name)),
            );
            ok = false;
            continue;
        }

        let Some(schema_field) = contract.fields.iter().find(|f| f.name == field_name) else {
            errors.push(
                invalid_annotation(field.span, ISSUE_UNKNOWN_FIELD)
                    .with_arg("field", interner.resolve(field_name))
                    .with_arg("contract", interner.resolve(contract.name)),
            );
            ok = false;
            continue;
        };

        match payload_value(&field.value, schema_field, interner) {
            Ok(value) => resolved_fields.push(ResolvedContractField {
                name: field_name,
                value,
                span: field.span,
            }),
            Err(issue) => {
                errors.push(
                    invalid_annotation(field.span, issue)
                        .with_arg("field", interner.resolve(field_name))
                        .with_arg("expected", schema_field.ty.as_str()),
                );
                ok = false;
            }
        }
    }

    for schema_field in &contract.fields {
        if schema_field.optional || seen.contains_key(&schema_field.name) {
            continue;
        }
        errors.push(
            invalid_annotation(span, ISSUE_MISSING_FIELD)
                .with_arg("field", interner.resolve(schema_field.name))
                .with_arg("contract", interner.resolve(contract.name)),
        );
        ok = false;
    }

    if !ok {
        return None;
    }

    Some(ResolvedContractApplication {
        contract_id: contract.def_id,
        family: contract.name,
        fields: resolved_fields,
        span,
    })
}

fn payload_value(
    value: &AnnotationFieldValue,
    schema: &AnnotationContractField,
    interner: &Interner,
) -> Result<ContractPayloadValue, &'static str> {
    match value {
        AnnotationFieldValue::Type(_) => Err(ISSUE_INVALID_PAYLOAD),
        AnnotationFieldValue::Expr(expr) => match &expr.kind {
            ExprKind::Literal(Literal::String(sym)) if schema.ty == ContractFieldType::Textus => {
                Ok(ContractPayloadValue::String(*sym))
            }
            ExprKind::Literal(Literal::Int(n)) if schema.ty == ContractFieldType::Numerus => {
                // Annotation contract integer payloads keep a checked signed
                // contract; out-of-range u64 magnitudes are invalid payloads,
                // never wrapped.
                match i64::try_from(*n) {
                    Ok(n) => Ok(ContractPayloadValue::Int(n)),
                    Err(_) => Err(ISSUE_INVALID_PAYLOAD),
                }
            }
            ExprKind::Literal(Literal::Float(n) | Literal::FloatText { value: n, .. })
                if schema.ty == ContractFieldType::Fractus =>
            {
                Ok(ContractPayloadValue::Float(*n))
            }
            ExprKind::Literal(Literal::Bool(b)) if schema.ty == ContractFieldType::Bivalens => {
                Ok(ContractPayloadValue::Bool(*b))
            }
            ExprKind::Literal(Literal::Nil) if schema.optional => Ok(ContractPayloadValue::Nihil),
            ExprKind::KeywordLiteral(keyword) => match &keyword.literal {
                Literal::Bool(b) if schema.ty == ContractFieldType::Bivalens => {
                    Ok(ContractPayloadValue::Bool(*b))
                }
                Literal::Float(n) if schema.ty == ContractFieldType::Fractus => {
                    Ok(ContractPayloadValue::Float(*n))
                }
                Literal::Nil if schema.optional => Ok(ContractPayloadValue::Nihil),
                _ => Err(ISSUE_INVALID_PAYLOAD),
            },
            ExprKind::Ident(ident) if schema.ty == ContractFieldType::Bivalens => {
                match interner.resolve(ident.name) {
                    "verum" => Ok(ContractPayloadValue::Bool(true)),
                    "falsum" => Ok(ContractPayloadValue::Bool(false)),
                    _ => Err(ISSUE_INVALID_PAYLOAD),
                }
            }
            _ => Err(ISSUE_INVALID_PAYLOAD),
        },
    }
}

/// Whether a resolved name is a genus that is not an annotation contract.
pub fn genus_is_non_contract(
    resolver: &Resolver,
    registry: &AnnotationContractRegistry,
    name: Symbol,
) -> bool {
    if registry.contains_name(name) {
        return false;
    }
    resolver
        .lookup(name)
        .and_then(|def_id| resolver.get_symbol(def_id))
        .is_some_and(|sym| sym.kind == SymbolKind::Struct)
}
