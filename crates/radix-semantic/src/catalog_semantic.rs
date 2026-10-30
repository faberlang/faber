//! Semantic and warning diagnostic catalog entries.
//!
//! This module owns analysis-phase code mappings and structured help
//! rendering. The parent catalog owns reader-locale fallback diagnostics and
//! shared argument helpers; diagnostic message prose lives with the phase
//! that raises it.

use super::catalog::{DiagnosticSpec, arg_value};
use crate::numeric_width::WidthLowerIssue;
use crate::{SemanticErrorKind, WarningKind};
use radix_diagnostics::DiagnosticArg;

// ANALYSIS KINDS

pub(crate) const SEMANTIC_ERROR_KINDS: &[SemanticErrorKind] = &[
    SemanticErrorKind::VariableUndefined,
    SemanticErrorKind::TypeUndefined,
    SemanticErrorKind::FunctionUndefined,
    SemanticErrorKind::MemberUndefined,
    SemanticErrorKind::DefinitionDuplicate,
    SemanticErrorKind::ImportAbsent,
    SemanticErrorKind::CircularDependency,
    SemanticErrorKind::LoweringError,
    SemanticErrorKind::CliValidation,
    SemanticErrorKind::AnnotationInvalid,
    SemanticErrorKind::ConflictingVisibility,
    SemanticErrorKind::ShadowedVariable,
    SemanticErrorKind::TypeMismatch,
    SemanticErrorKind::OperandTypesInvalid,
    SemanticErrorKind::NonCallable,
    SemanticErrorKind::ArityMismatch,
    SemanticErrorKind::TypeAnnotationAbsent,
    SemanticErrorKind::CastInvalid,
    SemanticErrorKind::ConversionInvalid,
    SemanticErrorKind::UnavailableDeclaration,
    SemanticErrorKind::AirPurityViolation,
    SemanticErrorKind::StaticInitializerInvalid,
    SemanticErrorKind::InsereInvalid,
    SemanticErrorKind::MemberInaccessible,
    SemanticErrorKind::PraefixumInvalid,
    SemanticErrorKind::ImmutableAssignment,
    SemanticErrorKind::AssignmentTargetInvalid,
    SemanticErrorKind::UseBeforeInit,
    SemanticErrorKind::BreakOutsideBreakable,
    SemanticErrorKind::ContinueOutsideLoop,
    SemanticErrorKind::ReturnOutsideFunction,
    SemanticErrorKind::ReturnAbsent,
    SemanticErrorKind::NonExhaustiveMatch,
    SemanticErrorKind::UnreachablePattern,
    SemanticErrorKind::PatternDuplicate,
    SemanticErrorKind::OmniaRequiresEnum,
    SemanticErrorKind::OmniaForbidsCatchall,
    SemanticErrorKind::UseAfterMove,
    SemanticErrorKind::BorrowOfMoved,
    SemanticErrorKind::MutableBorrowConflict,
    SemanticErrorKind::CannotMoveOut,
    SemanticErrorKind::LifetimeMismatch,
    SemanticErrorKind::AssignToImmutableBorrow,
    SemanticErrorKind::ModeMismatch,
    SemanticErrorKind::TopLevelBindingForbidden,
    SemanticErrorKind::ConstSharedIntoVar,
    SemanticErrorKind::CopyNonduplicable,
];

pub(crate) const WARNING_KINDS: &[WarningKind] = &[
    WarningKind::UnusedVariable,
    WarningKind::UnusedImport,
    WarningKind::UnusedFunction,
    WarningKind::UnreachableCode,
    WarningKind::UnnecessaryCast,
    WarningKind::TargetNoop,
    WarningKind::UnusedMutRefParam,
    WarningKind::UnusedMoveParam,
    WarningKind::ExplicitIgnotumAnnotation,
    WarningKind::WildcardAbsorbsNull,
    WarningKind::UnnecessaryVaria,
    WarningKind::FileInterfaceExportSkipped,
    WarningKind::MonomorphicHoleSolvedToUnion,
    WarningKind::UnionHoleSolvedMonomorphically,
    WarningKind::PreferStringTemplate,
    WarningKind::DiscouragedMutLocal,
    WarningKind::EmptyMarkerShadowing,
    WarningKind::UnboundedTextusIndex,
    WarningKind::TypeTestMemberAbsent,
    WarningKind::UnionNoCommonMembers,
    WarningKind::UnionMethodSignatureShadowed,
    WarningKind::ComplexityBudgetExceeded,
    WarningKind::RedundantMemberVisibility,
    WarningKind::CoverageUnchecked,
    WarningKind::LeadingUnderscore,
    WarningKind::ClosableInCycle,
    WarningKind::PositionalLaneParam,
    WarningKind::UnprovenKernelIndex,
    WarningKind::PreferGuillemetQuote,
    WarningKind::MixedTemplateHoles,
    WarningKind::TrapErrorChannelEmpty,
];

// STABLE CODES

/// Return the stable diagnostic spec for a semantic error or warning kind.
///
/// Semantic codes cover multiple compiler policies after parsing has succeeded:
/// name resolution/lowering, type checking, assignment and mutability, control
/// flow, pattern matching, borrow rules, and warning categories. The ranges are
/// descriptive rather than exhaustive; keep additions close to their policy
/// area and preserve existing numbers.
#[must_use]
pub fn semantic_spec(kind: SemanticErrorKind) -> DiagnosticSpec {
    match kind {
        SemanticErrorKind::VariableUndefined => DiagnosticSpec {
            code: "SEM001",
            help: Some("declare the variable before use"),
        },
        SemanticErrorKind::TypeUndefined => DiagnosticSpec {
            code: "SEM002",
            help: Some("declare the type or import it"),
        },
        SemanticErrorKind::FunctionUndefined => DiagnosticSpec {
            code: "SEM003",
            help: Some("declare the function or import it"),
        },
        SemanticErrorKind::MemberUndefined => DiagnosticSpec {
            code: "SEM004",
            help: Some("check the member name and type"),
        },
        SemanticErrorKind::DefinitionDuplicate => DiagnosticSpec {
            code: "SEM005",
            help: Some("rename one of the definitions"),
        },
        SemanticErrorKind::ImportAbsent => DiagnosticSpec {
            code: "SEM006",
            help: Some("check the import path"),
        },
        SemanticErrorKind::CircularDependency => DiagnosticSpec {
            code: "SEM007",
            help: Some("break the cycle between modules"),
        },
        SemanticErrorKind::LoweringError => DiagnosticSpec {
            code: "SEM008",
            help: Some("fix the AST construct for lowering"),
        },
        SemanticErrorKind::CliValidation => DiagnosticSpec {
            code: "SEM009",
            help: Some("fix the CLI annotation contract before emitting runnable CLI code"),
        },
        SemanticErrorKind::AnnotationInvalid => DiagnosticSpec {
            code: "SEM018",
            help: Some("fix the annotation contract before semantic lowering"),
        },
        SemanticErrorKind::ConflictingVisibility => DiagnosticSpec {
            code: "SEM019",
            help: Some("use a single visibility tier per declaration"),
        },
        SemanticErrorKind::ShadowedVariable => DiagnosticSpec {
            code: "SEM055",
            help: Some("rename one of the variables"),
        },
        SemanticErrorKind::TypeMismatch => DiagnosticSpec {
            code: "SEM010",
            help: Some("make the expression type match the expected type"),
        },
        SemanticErrorKind::OperandTypesInvalid => DiagnosticSpec {
            code: "SEM011",
            help: Some("use compatible operand types"),
        },
        SemanticErrorKind::NonCallable => DiagnosticSpec {
            code: "SEM012",
            help: Some("call a function or closure"),
        },
        SemanticErrorKind::ArityMismatch => DiagnosticSpec {
            code: "SEM013",
            help: Some("adjust the number of arguments"),
        },
        SemanticErrorKind::TypeAnnotationAbsent => DiagnosticSpec {
            code: "SEM014",
            help: Some("add a type annotation"),
        },
        SemanticErrorKind::CastInvalid => DiagnosticSpec {
            code: "SEM015",
            help: Some("cast to a compatible type"),
        },
        SemanticErrorKind::ConversionInvalid => DiagnosticSpec {
            code: "SEM016",
            help: Some("use a valid conversion or provide an inline default with `⊥`"),
        },
        SemanticErrorKind::UnavailableDeclaration => DiagnosticSpec {
            code: "SEM017",
            help: Some("use an available declaration or implement the marked surface"),
        },
        SemanticErrorKind::StaticInitializerInvalid => DiagnosticSpec {
            code: "SEM060",
            help: Some("move the computation into a function, or use `praefixum { … }`"),
        },
        SemanticErrorKind::InsereInvalid => DiagnosticSpec {
            code: "SEM061",
            help: Some(
                "embed a package-relative file that exists, as `textus` (UTF-8) or `octeti`",
            ),
        },
        SemanticErrorKind::MemberInaccessible => DiagnosticSpec {
            code: "SEM063",
            help: Some("reach the member through the type's own methods, or change its visibility"),
        },
        SemanticErrorKind::PraefixumInvalid => DiagnosticSpec {
            code: "SEM064",
            help: Some("keep a `praefixum` body to pure computation that yields a literal value"),
        },
        SemanticErrorKind::AirPurityViolation => DiagnosticSpec {
            code: "SEM059",
            help: Some("keep `@ radix lane \"air\"` functions in the accepted pure subset"),
        },
        SemanticErrorKind::ImmutableAssignment => DiagnosticSpec {
            code: "SEM020",
            help: Some(
                "use a `varia` binding to allow reassignment, or remove the extra assignment",
            ),
        },
        SemanticErrorKind::AssignmentTargetInvalid => DiagnosticSpec {
            code: "SEM021",
            help: Some("assign to a valid lvalue"),
        },
        SemanticErrorKind::UseBeforeInit => DiagnosticSpec {
            code: "SEM058",
            help: Some("assign the binding before reading it, or declare the slot `sponte`"),
        },
        SemanticErrorKind::BreakOutsideBreakable => DiagnosticSpec {
            code: "SEM030",
            help: Some("use 'rumpe' inside dum, itera, fac, meus, or tuus"),
        },
        SemanticErrorKind::ContinueOutsideLoop => DiagnosticSpec {
            code: "SEM031",
            help: Some("use 'perge' inside a loop"),
        },
        SemanticErrorKind::ReturnOutsideFunction => DiagnosticSpec {
            code: "SEM032",
            help: Some("use 'redde' inside a function"),
        },
        SemanticErrorKind::ReturnAbsent => DiagnosticSpec {
            code: "SEM033",
            help: Some("add a return statement or make the return type 'vacuum'"),
        },
        SemanticErrorKind::NonExhaustiveMatch => DiagnosticSpec {
            code: "SEM040",
            help: Some("add missing cases or a 'ceterum' arm"),
        },
        SemanticErrorKind::UnreachablePattern => DiagnosticSpec {
            code: "SEM041",
            help: Some("remove or reorder the unreachable pattern"),
        },
        SemanticErrorKind::PatternDuplicate => DiagnosticSpec {
            code: "SEM042",
            help: Some("remove the duplicate pattern"),
        },
        SemanticErrorKind::OmniaRequiresEnum => DiagnosticSpec {
            code: "SEM043",
            help: Some("use `omnia` only with a single enum scrutinee"),
        },
        SemanticErrorKind::OmniaForbidsCatchall => DiagnosticSpec {
            code: "SEM044",
            help: Some("list every variant explicitly instead of using `casu _` or `ceterum`"),
        },
        SemanticErrorKind::UseAfterMove => DiagnosticSpec {
            code: "SEM050",
            help: Some("avoid using a value after it has been moved"),
        },
        SemanticErrorKind::BorrowOfMoved => DiagnosticSpec {
            code: "SEM051",
            help: Some("borrow only from values that have not been moved"),
        },
        SemanticErrorKind::MutableBorrowConflict => DiagnosticSpec {
            code: "SEM052",
            help: Some("avoid overlapping mutable borrows"),
        },
        SemanticErrorKind::CannotMoveOut => DiagnosticSpec {
            code: "SEM053",
            help: Some("use a reference or clone instead of moving"),
        },
        SemanticErrorKind::LifetimeMismatch => DiagnosticSpec {
            code: "SEM054",
            help: Some("ensure borrowed values live long enough"),
        },
        SemanticErrorKind::AssignToImmutableBorrow => DiagnosticSpec {
            code: "SEM056",
            help: Some("declare the parameter as 'in' if mutation is intended"),
        },
        SemanticErrorKind::ModeMismatch => DiagnosticSpec {
            code: "SEM057",
            help: Some("align parameter modes between caller and callee"),
        },
        SemanticErrorKind::TopLevelBindingForbidden => DiagnosticSpec {
            code: "SEM062",
            help: Some(
                "a module-level value is a compile-time constant: `fixum T X = …`; a runtime value belongs in a function",
            ),
        },
        SemanticErrorKind::ConstSharedIntoVar => DiagnosticSpec {
            code: "SEM065",
            help: Some("write `← copy x` for an independent value, or `← own x` to take it over"),
        },
        SemanticErrorKind::CopyNonduplicable => DiagnosticSpec {
            code: "SEM066",
            help: Some(
                "share the value without `copy`, or take the resource over with `own` where the consumer may own it",
            ),
        },
        SemanticErrorKind::Warning(kind) => warning_spec(kind),
    }
}

// ISSUE RENDERING

#[must_use]
pub fn semantic_help(kind: SemanticErrorKind, args: &[DiagnosticArg]) -> Option<String> {
    if kind == SemanticErrorKind::TypeMismatch
        && arg_value(args, "issue") == Some("filum_independence_unproven")
    {
        return Some("use an ordinary loop, or prove own-coordinate writes and same-coordinate reads of written storage without loop-carried locals or hidden effects".to_owned());
    }
    if kind == SemanticErrorKind::TypeMismatch
        && arg_value(args, "issue") == Some("break_in_thread_body")
    {
        return Some("`break` cannot stop a `thread` loop: lanes run independently. To find the first match use `reduce via argmin …`; to stop at a cutoff, compute it first and loop to it.".to_owned());
    }
    if kind == SemanticErrorKind::TypeMismatch
        && arg_value(args, "issue") == Some("nominal_ascription_not_provable")
    {
        return Some(
            "make the source type match the target, declare the contract on the genus, or convert explicitly with `↦`"
                .to_owned(),
        );
    }
    match kind {
        SemanticErrorKind::AirPurityViolation => semantic_air_purity_help(args),
        SemanticErrorKind::ConflictingVisibility => semantic_conflicting_visibility_help(args),
        SemanticErrorKind::ConversionInvalid => semantic_invalid_conversion_help(args),
        SemanticErrorKind::TypeUndefined | SemanticErrorKind::LoweringError => {
            semantic_removed_width_help(args)
        }
        SemanticErrorKind::AnnotationInvalid => semantic_invalid_annotation_help(args),
        SemanticErrorKind::OperandTypesInvalid => semantic_invalid_operand_types_help(args),
        SemanticErrorKind::TypeAnnotationAbsent => semantic_missing_type_annotation_help(args),
        SemanticErrorKind::NonCallable => semantic_not_callable_help(args),
        SemanticErrorKind::DefinitionDuplicate => semantic_duplicate_definition_help(args),
        SemanticErrorKind::UnavailableDeclaration => semantic_unavailable_declaration_help(args),
        SemanticErrorKind::MemberUndefined => semantic_undefined_member_help(args),
        SemanticErrorKind::ImportAbsent => semantic_import_not_found_help(args),
        SemanticErrorKind::VariableUndefined => semantic_undefined_variable_help(args),
        SemanticErrorKind::ArityMismatch => semantic_wrong_arity_help(args),
        SemanticErrorKind::ConstSharedIntoVar => Some(semantic_const_shared_into_var_help(args)),
        SemanticErrorKind::CopyNonduplicable => semantic_copy_nonduplicable_help(args),
        SemanticErrorKind::Warning(WarningKind::MonomorphicHoleSolvedToUnion) => {
            semantic_monomorphic_hole_solved_to_union_help(args)
        }
        SemanticErrorKind::Warning(WarningKind::UnionHoleSolvedMonomorphically) => {
            semantic_union_hole_solved_monomorphically_help(args)
        }
        SemanticErrorKind::Warning(WarningKind::EmptyMarkerShadowing) => {
            semantic_keyword_shadowing_help(args)
        }
        SemanticErrorKind::Warning(WarningKind::UnboundedTextusIndex) => {
            Some("convert to textus<size N> for O(1) indexing or iterate scalars".to_owned())
        }
        SemanticErrorKind::Warning(WarningKind::TypeTestMemberAbsent) => {
            Some("narrow with a member the union declares, or fix the tested type".to_owned())
        }
        SemanticErrorKind::Warning(
            WarningKind::UnionNoCommonMembers | WarningKind::UnionMethodSignatureShadowed,
        ) => semantic_union_guardrail_help(args),
        _ => None,
    }
}

// CLI CONTRACT

// MODULE GRAPH

// CONTROL FLOW

// TYPE ANNOTATIONS AND CALLS

fn semantic_missing_type_annotation_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "cannot_infer_tensor_shape" => Some("provide a concrete tensor or sparsa shape".to_owned()),
        "bounded_capacity_unsolved" => {
            Some("write a concrete N (ascii<size 8>) or give the hole a bounded witness".to_owned())
        }
        "marker_width_unsolved" => {
            Some("write a concrete marker (i32) or give the hole a same-family witness".to_owned())
        }
        "cannot_infer_alternate_exit_type"
        | "cannot_infer_constant_type"
        | "cannot_infer_expression_type"
        | "cannot_infer_field_type"
        | "cannot_infer_return_type"
        | "cannot_infer_variable_type" => Some("add an explicit type annotation".to_owned()),
        "directional_view_type_not_registered"
        | "scrinium_type_not_registered"
        | "sermo_type_not_registered" => {
            Some("ensure built-in frame types are registered before typechecking".to_owned())
        }
        "vacua_requires_declared_type" => {
            Some("add a declared collection type for vacua".to_owned())
        }
        _ => None,
    }
}

fn semantic_wrong_arity_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "callee_type_args_unsupported" => Some("remove the explicit type argument list".to_owned()),
        "gpu_builtin_expects_no_arguments" | "method_accepts_no_arguments" => {
            Some("remove the call arguments".to_owned())
        }
        "variant_pattern_arity_mismatch" => {
            Some("match the pattern field count to the variant".to_owned())
        }
        "vector_constructor_width_arity" => {
            Some("provide one constructor element per vector lane".to_owned())
        }
        "wrong_number_of_arguments" => Some("adjust the number of arguments".to_owned()),
        "wrong_number_of_type_arguments" => Some("adjust the number of type arguments".to_owned()),
        _ => None,
    }
}

fn semantic_not_callable_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "callee_not_callable" => Some("call a function, closure, or callable value".to_owned()),
        _ => None,
    }
}

// CONVERSION AND OPERAND TYPES

/// Help for the removed `d32` decimal width (D11.7). Other type-lowering
/// issues keep the per-code default help.
fn semantic_removed_width_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        WidthLowerIssue::REMOVED_DECIMAL_WIDTH => {
            Some("d32 was removed; use d64, the only decimal width".to_owned())
        }
        _ => None,
    }
}

fn semantic_invalid_conversion_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "intervallum_clamp_recovery_unsupported" => Some("remove the recovery arm from this range clamp".to_owned()),
        "intervallum_target_not_range" => Some("convert to a range or intervallum value".to_owned()),
        "intervallum_clamp_bound_mismatch"
        | "intervallum_materialize_lista_element_mismatch"
        | "intervallum_materialize_tensor_element_mismatch" => {
            Some("make the source and target bound element types match".to_owned())
        }
        "sermo_materialize_target_unsupported" => Some("choose a supported sermo materialization target".to_owned()),
        "valor_to_regex_unsupported" => Some("convert valor to textus before converting to regex".to_owned()),
        "valor_to_cursor_boundary" | "cursor_to_valor_boundary" => {
            Some("materialize the cursor through lista before crossing the boundary".to_owned())
        }
        "valor_to_promissum_boundary"
        | "promissum_to_valor_boundary"
        | "promissum_target_boundary"
        | "promissum_source_boundary" => Some("resolve or wrap the async value explicitly".to_owned()),
        "numeric_to_octeti_unspecified"
        | "octeti_to_numerus_unspecified"
        | "octeti_to_fractus_unspecified"
        | "octeti_to_bivalens_unsupported" => Some("use chorda or explicit byte conversion helpers".to_owned()),
        "lista_to_textus_unsupported" => Some("convert to list<b8>, list<b16> or list<b32> first, then ↦ string; or show the list with the format string \"§\"(xs)".to_owned()),
        "textus_to_lista_unsupported" => Some("encode with ↦ list<b8>, list<b16> or list<b32> and work from the octets, or build the list of characters explicitly".to_owned()),
        "octeti_endian_width_mismatch" => {
            Some("use a window of 2, 4, or 8 bytes matching i16/u16, i32/u32, or i64/u64".to_owned())
        }
        "list_b8_to_tensor_unspecified" => Some("write bytes ↦ tensor<f32, [rows, cols]> via Le (or via Be); no other hint applies, and the hint is required for every element width".to_owned()),
        "list_b8_to_tensor_element_unsupported" | "tensor_to_list_b8_element_unsupported" => {
            Some("use one of u8, i8, i16, u16, i32, u32, i64, u64, f32, or f64 as the tensor element type".to_owned())
        }
        "list_b8_to_tensor_record_big_endian" => Some("write bytes ↦ tensor<Record, [n]> via Le; a record element has one byte order, fixed by its layout".to_owned()),
        "tensor_to_list_b8_unspecified" => Some("write tensor ↦ octeti via Le (or via Be); no other hint applies, and the hint is required for every element width".to_owned()),
        "list_b8_to_tensor_length_mismatch" => {
            Some("size the byte window to product(shape) times the element width, or change the tensor shape".to_owned())
        }
        "float_to_integer_requires_rounding" => {
            Some("write `x ↦ int via Floor`, `via Ceil`, `via HalfEven` (ties to the even neighbour) or `via Trunc`; a `↤` site takes the same clause, `a ↤ x via Trunc`; or round first with ⌊x⌋, ⌈x⌉ or ⌊x⌉ and convert the result".to_owned())
        }
        "conversio_via_bits_retired" => {
            Some("write `x ↦ b32` to read a float as its bit pattern (b16, b32 or b64 by width) and `b32 ↦ f32` to build one; a pattern held in a number goes through its marker first, as in `u16 ↦ b16 ↦ f16`".to_owned())
        }
        "conversio_map_key_not_textus" => {
            Some("write the map with `string` keys; a conversion never parses keys, and `@ json` on a class defines the wire mapping".to_owned())
        }
        "code_hint_pair_mismatch" => {
            Some("Code reads a code point as littera ↦ u32 via Code or builds one as numerus ↦ littera via Code".to_owned())
        }
        "conversio_via_unknown_hint" => {
            Some("use one of Hex, Bin, Oct, Be, Le, or Code after via; Floor, Ceil, HalfEven and Trunc belong only to a float or decimal converted to an integer".to_owned())
        }
        "conversio_via_target_takes_no_hint" => {
            Some("drop the via clause; only numeric, ascii, littera, vector, and octeti<N> targets take a hint".to_owned())
        }
        "ascii_format_hint_not_capacity" => {
            Some("write ascii<size N> via Hex, ascii<size N> via Bin, or ascii<size N> via Oct with N as digit width".to_owned())
        }
        "ascii_pack_width_overflow" => {
            Some("increase N or convert the source to a width whose digit count fits".to_owned())
        }
        "ascii_pack_negative_source" => {
            Some("convert to an unsigned numerus width before packing".to_owned())
        }
        "sparsa_collection_unsupported" => Some("densify or use an explicit sparse helper".to_owned()),
        "sparsa_tensor_element_mismatch" | "tensor_element_mismatch" => {
            Some("convert element widths explicitly before changing representation".to_owned())
        }
        "sparsa_tensor_shape_mismatch" | "tensor_shape_mismatch" => {
            Some("reshape explicitly before converting".to_owned())
        }
        "tensor_to_scalar_unsupported" => Some("index or reduce the tensor before converting".to_owned()),
        "scalar_to_tensor_unsupported" => Some("construct or fill the tensor explicitly".to_owned()),
        "conversio_failure_channel_textus_required" => {
            Some("declare the alternate-exit channel as textus or provide inline recovery".to_owned())
        }
        "ordo_conversio_string_member" => Some(
            "only an ordo whose members are numbers converts to and from numerus; convert to textus for the member name".to_owned(),
        ),
        "ordo_conversio_recovery_unsupported" => {
            Some("remove the `⊥` default; this ordo conversion cannot fail".to_owned())
        }
        "conversio_assign_route_unsupported" => {
            Some("write `target ← source ↦ T` instead of `↤` for this conversion".to_owned())
        }
        "bivalens_recovery_unsupported" => {
            Some("remove the recovery arm; use the truthiness result directly".to_owned())
        }
        "word_conversio_recovery_unsupported" => Some(
            "remove the `⊥` default; a conversion into wrapping or saturating cannot fail".to_owned(),
        ),
        "regex_operand_unsupported" => Some("convert textus or ascii values to regex".to_owned()),
        "record_conversion_requires_reconstruction" => {
            Some("reconstruct by writing `Type { field = value, … }` instead of converting with `↦`".to_owned())
        }
        "regex_recovery_unsupported" => Some("remove the recovery arm from this regex conversion".to_owned()),
        "matrix_materialize_unsupported" => {
            Some("keep matrix values as typed parameters until matrix construction lowering is implemented".to_owned())
        }
        "inferred_constructor_entries_need_map" => Some(
            "entries `\"key\": value` build a map; for a genus or class write fields `name = value`".to_owned(),
        ),
        "inferred_constructor_fields_for_map" => Some(
            "a map takes entries `\"key\": value`; fields `name = value` build a genus or class".to_owned(),
        ),
        "inferred_constructor_ambiguous_union" => {
            Some("write the explicit `Type { … }` for the member you mean".to_owned())
        }
        "inferred_constructor_no_expected_type" => {
            Some("name the type: write `Type { … }`, or give the binding, parameter or return an explicit type".to_owned())
        }
        "valor_verte_unsupported" => Some("use runtime `↦ T` conversion with a `⊥` default instead".to_owned()),
        "lista_to_tabula_unsupported" => Some("construct the tabula with explicit key/value selection".to_owned()),
        "array_ascription_to_map" => Some(
            "an array is not a map: `∷` asserts a type the expression already has, so even an \
             index-keyed array-to-map is a conversion, not an ascription; build the map with \
             `empty`, `vacua`, or a keyed literal"
                .to_owned(),
        ),
        "array_ascription_to_tensor" => Some(
            "an array is not a tensor: `∷` asserts a type the expression already has, and shape \
             agreement is a coincidence, not structural compatibility; convert explicitly with `↦`"
                .to_owned(),
        ),
        "cross_collection_unsupported" => Some("use an explicit stdlib conversion verb".to_owned()),
        "lista_to_tensor_nested" => Some("flatten the nested lista before converting to tensor".to_owned()),
        "lista_to_tensor_element_mismatch" => {
            Some("convert lista elements to the tensor element type first".to_owned())
        }
        "rectangular_lista_literal_target" => {
            Some("convert nested lista literals to a rank-2 tensor target".to_owned())
        }
        "rectangular_lista_literal_ragged" | "rectangular_lista_literal_row_length" => {
            Some("make every nested row the same length".to_owned())
        }
        "rectangular_lista_literal_shape" => {
            Some("match the nested literal dimensions to the target Figura".to_owned())
        }
        "rectangular_lista_literal_spread" => Some("expand spread elements before tensor conversion".to_owned()),
        "rectangular_lista_literal_leaf" => Some("use literal scalar leaves in the nested lista".to_owned()),
        _ => None,
    }
}

fn semantic_invalid_operand_types_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "explicit_union_type_arg_unsupported" => {
            Some("write _ for a monomorphic hole, or a concrete type".to_owned())
        }
        "range_step_must_be_numeric" => Some("use a numeric range step".to_owned()),
        "intervallum_bound_required" => Some("use numerus, fractus, instans, or sized scalar bounds".to_owned()),
        "intervallum_longitudo_bound" => Some("use numerus intervallum bounds for longitudo today".to_owned()),
        "intervallum_helper_bound_unsupported" => {
            Some("use bare numerus intervallum bounds for expansion helpers today".to_owned())
        }
        "numeric_lista_operator_law" => Some(
            "apply +, - or ÷ elementwise between two lists, ⊙ or ⊘ for products and quotients, or any of + - * / % ÷ between a list and a scalar; use ↦ to align numeric widths"
                .to_owned(),
        ),
        "numeric_operands_required" => Some("use numeric operands for this operator".to_owned()),
        "decimal_literal_out_of_range" => Some(
            "a d64 literal has at most 8 fraction digits and lies within ±92233720368.54775807; a written literal is never rounded"
                .to_owned(),
        ),
        WidthLowerIssue::MODULAR_WORD_WIDTH_REQUIRED => {
            Some("write the modular word type as modulus<u8>, modulus<u16>, modulus<u32>, or modulus<u64>".to_owned())
        }
        WidthLowerIssue::MODULAR_WORD_WIDTH_SHAPE => Some("use a bare unsigned width marker such as u32".to_owned()),
        WidthLowerIssue::UNSUPPORTED_MODULAR_WORD_WIDTH => Some(
            "use an unsigned modular word width: modulus<u8>, modulus<u16>, modulus<u32>, or modulus<u64>".to_owned(),
        ),
        WidthLowerIssue::MODULAR_WORD_UNSIGNED_WIDTH_REQUIRED => Some("use an unsigned integer width marker such as u32".to_owned()),
        "numeric_arithmetic_operands_identical" => {
            Some("convert operands explicitly before applying this numeric operator".to_owned())
        }
        "u64_signed_arithmetic_requires_conversion" => Some(
            "convert one side with `↦` so both operands are u64 or both are signed (`a ↦ i64`), or join them exactly with `a ↦ inf`"
                .to_owned(),
        ),
        "numeric_family_crossing_requires_conversio" => Some(
            "convert one operand explicitly with `↦`; a written constant may stand beside any number"
                .to_owned(),
        ),
        WidthLowerIssue::SATURATING_WORD_WIDTH_REQUIRED => Some(
            "write the saturating type with one integer width, such as saturatus<u8> or saturatus<i16>".to_owned(),
        ),
        WidthLowerIssue::SATURATING_WORD_WIDTH_SHAPE => Some("use a bare integer width marker such as u8 or i16".to_owned()),
        WidthLowerIssue::UNSUPPORTED_SATURATING_WORD_WIDTH => Some(
            "use an integer width for saturatus: i8, i16, i32, i64, u8, u16, u32, or u64".to_owned(),
        ),
        WidthLowerIssue::TRAPPING_WORD_WIDTH_REQUIRED => Some(
            "write the trapping type with one width, such as exactus<u8> or exactus<i64>".to_owned(),
        ),
        WidthLowerIssue::TRAPPING_WORD_WIDTH_SHAPE => Some("use a bare width marker such as u8 or i64".to_owned()),
        WidthLowerIssue::UNSUPPORTED_TRAPPING_WORD_WIDTH => Some(
            "use an integer width or d64 for exactus: i8, i16, i32, i64, u8, u16, u32, u64, or d64".to_owned(),
        ),
        WidthLowerIssue::RANGED_POLICY_WORD_REQUIRED => Some(
            "write the policy word first, such as saturating<u32, 0‥8>, trapping<i32, -2‥3> or wrapping<u32, 0‥8>".to_owned(),
        ),
        WidthLowerIssue::RANGED_BOUND_NOT_CONSTANT => Some(
            "write both ends as integer literals, such as 0‥8; a named or size end is not built yet".to_owned(),
        ),
        WidthLowerIssue::RANGED_ARITY => {
            Some("write policy<W, lo‥hi>, such as saturating<u32, 0‥8>".to_owned())
        }
        WidthLowerIssue::RANGED_EMPTY | WidthLowerIssue::RANGED_LIST_EMPTY => Some(
            "the low end must be below the high end of lo‥hi, or not above it for lo…hi".to_owned(),
        ),
        WidthLowerIssue::RANGED_OUTSIDE_WIDTH => Some(
            "keep both ends inside the width; ∞ is only a high end and -∞ only a low end".to_owned(),
        ),
        WidthLowerIssue::RANGED_UNSIGNED_NEGATIVE => {
            Some("start the range at 0, or use a signed width such as i32".to_owned())
        }
        WidthLowerIssue::RANGED_LIST_BOUND_NOT_CONSTANT => Some(
            "write both ends as integer literals, such as list<int, size 1‥8>; a named or size end is not built yet".to_owned(),
        ),
        WidthLowerIssue::RANGED_LIST_BOUND_NEGATIVE => {
            Some("a length is a count: start the range at 0 or above".to_owned())
        }
        WidthLowerIssue::RANGED_LIST_POLICY_WORD => Some(
            "write the range on the list, such as list<int, size 1…8>; it always traps, and saturating, trapping and wrapping take a number width".to_owned(),
        ),
        WidthLowerIssue::RANGED_WIDTH_UNSUPPORTED => Some(
            "use an integer width such as u8 or i32; float, decimal and inf widths take no range yet".to_owned(),
        ),
        WidthLowerIssue::LIST_COUNT_NEEDS_SIZE => Some(
            "write list<int, size 4> for at most 4 elements, or list<int, size 1…8> for a range; a bare range bounds a value, not a count".to_owned(),
        ),
        WidthLowerIssue::CAPACITY_NEEDS_SIZE => Some(
            "write the capacity after size: queue<int, size 4>, stack<int, size 4>, string<size 8> or ascii<size 8>".to_owned(),
        ),
        WidthLowerIssue::VECTOR_WIDTH_NEEDS_SIZE => Some(
            "write vector<f32, size 4> for four lanes; the shorthand vf32[4] is unchanged".to_owned(),
        ),
        "ranged_list_length_out_of_range" => Some(
            "give the literal a length inside the range, or build the list with `↦` and recover with `⊥`".to_owned(),
        ),
        "policy_float_family_mismatch" => Some(
            "convert the other operand to the same float cell with `↦`; a trapping or saturating float never mixes with a bare float, another cell, or another width implicitly".to_owned(),
        ),
        "policy_float_math_not_built" => Some(
            "convert the cell to the bare float width with `↦`, call the method on that, and convert back".to_owned(),
        ),
        "saturating_word_family_mismatch" => Some(
            "convert the other operand to the same saturatus width with `↦`; saturating, wrapping, and trapping integers never mix implicitly".to_owned(),
        ),
        "modular_word_family_mismatch" => Some(
            "convert both operands to the same modulus width, or keep both operands in the numerus family".to_owned(),
        ),
        "ascii_literal_invalid_escape" => Some("use supported ASCII literal escape sequences".to_owned()),
        "ascii_literal_non_ascii_scalar" => {
            Some("use textus for Unicode text or keep ascii payloads ASCII-only".to_owned())
        }
        "ascii_literal_unescaped_control" => Some("escape control characters in ascii literals".to_owned()),
        "ascii_literal_too_long" => Some(
            "shorten the ascii<size N> literal to fit its declared capacity, or use unbounded ascii"
                .to_owned(),
        ),
        "textus_literal_too_long" => Some(
            "shorten the textus<size N> literal to fit its declared capacity, or use unbounded textus"
                .to_owned(),
        ),
        "boolean_operands_required" => Some("use bivalens operands for this operator".to_owned()),
        "atomic_method_requires_atomic_receiver" => {
            Some("call load/store/exchange/compare_exchange on atomic<T>".to_owned())
        }
        "unsupported_atomic_method" => Some("use load, store, exchange, or compare_exchange".to_owned()),
        "bitwise_method_requires_numerus_receiver" => Some("call bitwise methods on numerus receivers".to_owned()),
        "deref_requires_reference" => Some("dereference a reference value".to_owned()),
        "densata_requires_sparsa_receiver" => Some("call densata on a sparsa value".to_owned()),
        "ascii_literal_template_application" => Some("use a textus formatting literal for template calls".to_owned()),
        "directional_view_method_type_args"
        | "dynamic_callee_type_args"
        | "dynamic_receiver_method_type_args"
        | "gpu_builtin_type_args"
        | "inferred_callee_type_args"
        | "intrinsic_method_type_args"
        | "method_call_type_args"
        | "sermo_view_payload_type_arg_count"
        | "template_application_type_args"
        | "variant_constructor_type_args" => Some("remove the explicit type argument list".to_owned()),
        "failable_call_requires_handler" => Some(
            "handle the call with fac/cape, capture it with capta, or declare a function ⇥"
                .to_owned(),
        ),
        "failable_conversio_requires_handler" => Some(
            "add a '⊥' default, wrap it in fac/cape or capta, or declare a function ⇥; a conversion that cannot fail needs none"
                .to_owned(),
        ),
        "call_default_type_mismatch" => {
            Some("give a default of the type the call returns on success".to_owned())
        }
        "call_default_on_infallible_call" => Some(
            "remove the '⊥' default; only a call whose signature has '⇥' can fail".to_owned(),
        ),
        "coalesce_on_failable_call" => Some(
            "use '⊥' for a failable call: 'vel' only defaults an absent (optional) value".to_owned(),
        ),
        "trap_member_type_mismatch" => {
            Some("check the member type flowing into the capta trap".to_owned())
        }
        "trap_union_collapsed" => Some(
            "make the success type and the error channel distinguishable, or handle the error with fac/cape instead"
                .to_owned(),
        ),
        "generic_type_arg_domain" => {
            Some("choose one of the allowed type arguments for this generic parameter".to_owned())
        }
        "size_slot_bare_value_name" => Some("write `size ` followed by the value name to use it as a size witness".to_owned()),
        "annotation_extent_bare_value_name" => Some(
            "write `size ` followed by the value name at this store, as in `tf32[2, size n]`"
                .to_owned(),
        ),
        "signature_extent_bare_value_name" => Some(
            "declare a `<size N>` generic parameter and use it in the figura, or spell the lift `size n` where a runtime lift is intended"
                .to_owned(),
        ),
        "size_lift_negative" => Some(
            "size arguments are lift stores checked at the call; pass a non-negative value"
                .to_owned(),
        ),
        "constructor_requires_extent_type" => Some(
            "construction applies to extent-bearing tensor types; spell the shape, as in tf32[2, 3]"
                .to_owned(),
        ),
        "constructor_extent_requires_term" => Some(
            "a constructor extent must name a size or a literal; the `_` hole belongs in the annotation"
                .to_owned(),
        ),
        "constructor_register_extent_requires_literal" => Some(
            "a vector width or matrix extent in a constructor must be a literal, as in vector<f32, size 4>"
                .to_owned(),
        ),
        "constructor_affine_extent_unsupported" => Some(
            "constructor extents admit literals, size names, and spelled size lifts; parenthesize affine terms once supported"
                .to_owned(),
        ),
        "gpu_builtin_device_function_required" => {
            Some("call GPU builtins only inside @ nucleum or @ nucleum fragment functions".to_owned())
        }
        "nucleum_fragment_context" => Some(nucleum_fragment_context_help(args)),
        "nucleum_fragment_effect" => nucleum_fragment_effect_help(args),
        "nucleum_fragment_type" => nucleum_fragment_type_help(args),
        "nucleum_fragment_throw_caller" => Some(
            "declare `⇥ text` on the calling kernel; the callee's constant throws become throw sites of the caller"
                .to_owned(),
        ),
        "nucleum_fragment_cycle" => Some(nucleum_fragment_cycle_help(args)),
        "nucleum_fragment_expansion" => nucleum_fragment_expansion_help(args),
        "integer_operands_required" => Some("use integer operands for this operator".to_owned()),
        "instans_ordering_operands_required" => Some("compare instans values with instans values".to_owned()),
        "json_duplicate_key" => Some("remove or rename the duplicate JSON object key".to_owned()),
        "extend_requires_plain_list" => Some(
            "extend into an unbounded list, or append one element at a time with append".to_owned(),
        ),
        "numeric_lista_elements_required" => Some("use numeric elements for this lista intrinsic".to_owned()),
        "loop_collection_shape_change" => Some(
            "write a bracket store to update an element, or use a queue with while for a worklist".to_owned(),
        ),
        "octeti_literal_invalid_character" => Some("use only hexadecimal digits in octeti literals".to_owned()),
        "octeti_literal_odd_digit_count" => Some("provide complete byte pairs in octeti literals".to_owned()),
        "octeti_literal_template_application" => {
            Some("use textus formatting instead of octeti literal calls".to_owned())
        }
        "bits_arithmetic_refused" => Some(
            "bits have no arithmetic; use the bit operations, or convert to a number type with ↦ first".to_owned(),
        ),
        "bits_operand_mismatch" => Some(
            "use two values of the same bN width; a bN does not mix with numbers or other widths".to_owned(),
        ),
        "bits_conversion_width_mismatch" => Some(
            "convert a bN only to an integer, float or bits type of the same width, such as b8 to u8 or b32 to f32".to_owned(),
        ),
        "bits_via_clause_not_allowed" => Some(
            "remove the via clause; a same-width bN conversion keeps the bit pattern and takes no hint".to_owned(),
        ),
        "bits_via_clause_required" => Some(
            "write the byte order with the conversion, such as lista<b8> ↦ lista<u16> via Le; a same-width conversion takes no clause".to_owned(),
        ),
        "bits_literal_out_of_range" => {
            Some("write a literal that fits in the bits width, such as 0xff for a b8".to_owned())
        }
        "same_width_integer_operands_required" => {
            Some("convert integer operands to the same width before applying this operator".to_owned())
        }
        "postfix_inc_dec_numerus_operand" => Some("apply postfix increment/decrement to numerus values".to_owned()),
        "membership_requires_collection" => Some(
            "use a lista, copia or tabula as the right operand; narrow a nullable value first".to_owned(),
        ),
        "membership_over_text" => Some(
            "membership does not apply to text; test substring containment with the text's contains method".to_owned(),
        ),
        "membership_operand_unresolved" => Some(
            "give the right operand a known type, for example annotate the list or bind it with a typed declaration".to_owned(),
        ),
        "array_spread_requires_lista" => Some("spread a lista value inside lista construction".to_owned()),
        "struct_computed_key_forbidden" => Some("use declared struct field names in struct construction".to_owned()),
        "vector_constructor_forbids_spread" => {
            Some("provide explicit vector elements instead of spread arguments".to_owned())
        }
        "object_spread_requires_tabula" => Some("spread a tabula value inside tabula construction".to_owned()),
        "interval_bound_single_scalar" => {
            Some("pass textus or ascii values that contain exactly one Unicode scalar".to_owned())
        }
        "numeric_operand_required" => Some("use a numeric operand for this operator".to_owned()),
        "numeric_value_equality_operands_required" => Some(
            "use numeric operands with ≈ for fuzzy equality; use ≅ for promoted exact equality or .approx for explicit tolerance".to_owned(),
        ),
        "boolean_operand_required" => Some("use a bivalens operand for this operator".to_owned()),
        "condition_must_be_bivalens" => Some("use a bivalens expression as the condition".to_owned()),
        "aggregate_equality_scalar_operand" => Some(
            "use ≅ for a per-element mask (a ≅ 2.0), or compare a reduction".to_owned(),
        ),
        "aggregate_equality_shape_mismatch" => {
            Some("≡ requires identical types; compare aggregates of one shape".to_owned())
        }
        "aggregate_mask_not_truth_value" => Some(
            "reduce the mask first with reduce via all or reduce via any".to_owned(),
        ),
        "integer_operand_required" => Some("use an integer operand for this operator".to_owned()),
        "range_operand_must_be_numeric" => Some("compare numeric values with intervallum ranges".to_owned()),
        "scalar_text_operands_required" => Some("compare single-scalar textus or ascii operands".to_owned()),
        "string_operands_required" => Some("use textus operands for string addition".to_owned()),
        "object_field_requires_value" => Some("provide an explicit value for this object field".to_owned()),
        "vacua_requires_collection_type" => {
            Some("give vacua a lista, queue, stack, tabula, copia, tensor, or sparsa context".to_owned())
        }
        "vacua_requires_resolved_extents" => {
            Some("give the tensor, vector, or matrix literal extents, or write the typed zero with a resolved shape".to_owned())
        }
        "textus_index_must_be_numerus_or_range" => Some("index textus with numerus or an integer range".to_owned()),
        "format_spec_malformed" => {
            Some("write the spec as [fill][align][sign][0][width][.precision][kind], e.g. \"08.2\"".to_owned())
        }
        "format_spec_part_invalid" => Some("drop the spec part this type does not accept".to_owned()),
        "format_spec_preset_required" => Some("format instans with \"iso\", \"date\", or \"time\"".to_owned()),
        "format_spec_preset_not_allowed" => Some("the named presets format instans only".to_owned()),
        "format_type_unsupported" => {
            Some("¶ formats numbers, textus, and instans; write a function that returns textus".to_owned())
        }
        "octeti_index_must_be_numerus_or_range" => Some("index octeti with numerus or an integer range".to_owned()),
        "window_requires_list" => Some("take a window [lo‥hi] only of a lista, textus or octeti".to_owned()),
        "array_index_must_be_numerus" => Some("index lista values with numerus".to_owned()),
        "indexing_requires_array_or_map" => {
            Some("index only lista, tabula, tensor, textus, or ignotum values".to_owned())
        }
        "tensor_index_vector_integer_elements" => Some("use lista of supported integer index elements".to_owned()),
        "tensor_scalar_index_rank" => Some(
            "use a scalar index only on rank-1 tensors; index a rank-2 value by a coordinate list such as m[[r, c]]"
                .to_owned(),
        ),
        "tensor_bracket_index_integer_or_lista" => Some("use an integer scalar or lista of integer indices".to_owned()),
        "tensor_bracket_slot_integer_or_range" => {
            Some("make each bracket slot an integer or an integer range such as 0 ‥ 2".to_owned())
        }
        "tensor_arithmetic_numeric_element_required" => Some("use numeric tensor or sparsa element types".to_owned()),
        "tensor_math_float_element_required" => Some(
            "convert the integer elements first with `↦` to a float tensor; only `absolutum` takes integer elements"
                .to_owned(),
        ),
        "tensor_function_not_lifted" => Some(
            "user functions are not lifted over tensors; apply the function to each element in a `filum` loop or with map, or use a built-in math method such as `abs`, `sqrt`, or `exp`"
                .to_owned(),
        ),
        "matrix_arithmetic_numeric_element_required" => Some("use numeric matrix element types".to_owned()),
        "tensor_receiver_structa_renamed" => Some("write data ↦ tensor<T, figura>".to_owned()),
        "tensor_receiver_creata_type_args" => {
            Some("call tensor.creata<CompleteTensorType>(fill) for static construction".to_owned())
        }
        "tensor_strue_retired" => Some("write flat ↦ tensor<T, figura>".to_owned()),
        "tensor_crea_retired" => {
            Some("bind the constructor first: let tf32[2, 2] a ← tf32[2, 2] {}".to_owned())
        }
        "tensor_structa_forma_removed" => Some("use tensor.structa<CompleteTensorType>(data)".to_owned()),
        "tensor_creata_complete_type_arg_required" | "tensor_formata_complete_type_arg_required" => {
            Some("provide a complete tensor type argument".to_owned())
        }
        "tensor_formata_forbids_value_args" => Some("use grid.forma(shape) for runtime reshape".to_owned()),
        "tensor_type_arg_incomplete" => {
            Some("include both element type and shape in the tensor type argument".to_owned())
        }
        "glyph_operands_unsupported" => {
            Some("use the pinned built-in families: · on vectors/matrices, × on 3-vectors, ⊗ on vectors, ⊙ on identical vector/matrix/tensor shapes".to_owned())
        }
        "glyph_static_shape_required" => {
            Some("use literal vector widths or matrix/tensor shapes; shape-generic overload is deferred to Phase 2".to_owned())
        }
        "glyph_op_numeric_element_required" => Some("use numeric element types (numerus, fractus, or sized numeric)".to_owned()),
        "glyph_element_type_mismatch" => {
            Some("convert one operand with ↦ so both operands share the same element type".to_owned())
        }
        "glyph_inner_dim_mismatch" => {
            Some("contract the inner dimension: vector widths must match and matrix columns must equal the peer rows".to_owned())
        }
        "glyph_cross_width_mismatch" => Some("cross both operands at the same width".to_owned()),
        "glyph_cross_width_three_required" => {
            Some("× is pinned to 3-vectors in Phase 1; the 7-vector convention is deferred".to_owned())
        }
        "glyph_hadamard_shapes_mismatch" => {
            let op = arg_value(args, "op").unwrap_or("⊙");
            Some(match op {
                "⊘" => "⊘ requires identical operand shapes; use ↦ to reshape before the divide",
                _ => "⊙ requires identical operand shapes; use ↦ to reshape before the product",
            }
            .to_owned())
        }
        "glyph_transpose_operands_unsupported" => {
            Some("apply ᵀ to a tensor value".to_owned())
        }
        "glyph_transpose_rank1_no_row_column_distinction" => {
            Some("rank-1 transpose is permanently declined because it has no row/column distinction".to_owned())
        }
        "glyph_transpose_static_rank_required" => {
            Some("give ᵀ a statically known rank-2 tensor; shape-generic transpose is not admitted in v1".to_owned())
        }
        "glyph_transpose_suffix_takes_no_arguments" => {
            Some("write ᵀ directly after the tensor; the suffix takes no arguments".to_owned())
        }
        "gradient_expression_unsupported" => {
            Some("gradient selection (`∇`) is parse-only in Phase 2A; the eligibility/selector rules land with tensor-glyphs 2B".to_owned())
        }
        _ => None,
    }
}

fn nucleum_fragment_context_help(args: &[DiagnosticArg]) -> String {
    let caller = arg_value(args, "caller");
    match (caller, arg_value(args, "reason")) {
            (_, Some("entry_callee")) => {
                "compose another @ nucleum fragment, or keep the dispatch on the calling @ nucleum entry"
            }
            (_, Some("nested_dispatch")) => {
                "compose an @ nucleum fragment into the entry instead of dispatching another entry"
            }
            (Some("entry"), Some("graphics_stage")) => {
                "keep graphics @ fragment / @ vertex stages out of nucleus entries"
            }
            (_, Some("graphics_stage")) => "keep graphics @ fragment / @ vertex stages out of nucleus fragment bodies",
            (Some("host"), Some("host_callable")) => {
                "call an @ nucleum fragment only from an @ nucleum entry or another fragment"
            }
            (Some("entry"), Some("host_callable")) => {
                "call an @ nucleum fragment or an approved device intrinsic from this entry"
            }
            (_, Some("host_callable")) => {
                "call another @ nucleum fragment or an approved device intrinsic from this body"
            }
        _ => "call an @ nucleum fragment only from an @ nucleum entry or another fragment",
    }
    .to_owned()
}

fn nucleum_fragment_type_help(args: &[DiagnosticArg]) -> Option<String> {
    Some(
        match arg_value(args, "reason")? {
            "bindings" => {
                "supply concrete type and magnitudo arguments so the fragment can be specialized"
            }
            "shape" => {
                "pass a value whose concrete shape matches the specialized fragment parameter"
            }
            "layout" => {
                "pass a value whose concrete layout matches the specialized fragment parameter"
            }
            "argument" => "pass an argument whose specialized type matches the fragment parameter",
            "mode" => "pass the argument with the parameter's de/in/ex/owned mode",
            "target" => "restrict the specialized types to the device-safe target subset",
            "return" => {
                "assign the fragment result to a destination that matches the specialized return"
            }
            "effect" => {
                "keep the specialized fragment infallible and inside the device-safe effect subset"
            }
            _ => return None,
        }
        .to_owned(),
    )
}

fn nucleum_fragment_cycle_help(args: &[DiagnosticArg]) -> String {
    let cycle = arg_value(args, "cycle")
        .or_else(|| arg_value(args, "chain"))
        .unwrap_or("");
    if cycle.is_empty() {
        "remove the recursive fragment call; fragment composition requires an acyclic call graph"
            .to_owned()
    } else {
        format!("break the cycle {cycle}; fragment composition requires an acyclic call graph")
    }
}

fn nucleum_fragment_expansion_help(args: &[DiagnosticArg]) -> Option<String> {
    Some(
        match arg_value(args, "kind").or_else(|| arg_value(args, "reason"))? {
            "depth" => "shorten the fragment call chain or split the entry",
            "nodes" => "reduce the composed body size under this @ nucleum entry",
            "callsite_nodes" => {
                "one fragment call site expands to too many nodes; split the callee or call it from fewer sites"
            }
            "instances" => {
                "reduce the number of concrete fragment instances composed into this entry"
            }
            "callsite" => "a fragment call site can be composed only once",
            "survived" => {
                "composition must clone the fragment body; a surviving call is a compiler defect"
            }
            "unresolved" => "the fragment body must be present in the current MIR program",
            "failable" => "fragments are infallible; do not compose a TryCall",
            _ => "keep the composed fragment expansion inside the documented budget",
        }
        .to_owned(),
    )
}

fn nucleum_fragment_effect_help(args: &[DiagnosticArg]) -> Option<String> {
    Some(
        match arg_value(args, "reason")? {
            "alternate_exit" => "remove the ⇥ channel, iace, and fac/cape recovery from the fragment",
            "async_posture" | "generator_posture" => "write the fragment as a synchronous finite function",
            "host_io" => "keep scribe, lege, and other host I/O out of the fragment body",
            "host_allocation" => "use only local scalars, tensors, and caller-provided device storage",
            "host_runtime" => "replace the host helper with a fragment or an approved device intrinsic",
            "captured_mut_host" => "pass device state as an explicit argument instead of capturing it",
            "dynamic_function" => "call a named fragment or approved device intrinsic directly",
            "address_taking" => "read and write device values directly; do not form addresses",
            "hidden_storage" => "make every persistent write an explicit argument write",
            "missing_body" => "give the fragment a device-safe body",
            "unapproved_effect" => {
                "keep the body to scalar/tensor ops, structured control flow, locals, and device-safe argument I/O"
            }
            _ => return None,
        }
        .to_owned(),
    )
}

// NAME RESOLUTION

fn semantic_unavailable_declaration_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "nondum_method_for_target" | "nondum_function_for_target" => {
            Some("use an available declaration or change the target".to_owned())
        }
        "import_target_bound" => {
            let target = arg_value(args, "target")?;
            Some(format!(
                "build this program for `{target}`, or remove the target import"
            ))
        }
        _ => None,
    }
}

fn semantic_duplicate_definition_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "inferred_import_name_collision" => Some(
            "add an explicit import alias with `ut` so the imported binding has a unique name"
                .to_owned(),
        ),
        "default_import_name_collision" => {
            Some("alias one of the imports so each binding has a unique name".to_owned())
        }
        "duplicate_commune_field" => {
            Some("declare each commune field once in the union region".to_owned())
        }
        "union_variant_redeclares_shared_field" => {
            Some("omit the commune field from the variant payload".to_owned())
        }
        _ => None,
    }
}

fn semantic_undefined_variable_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "scribe_callee" => {
            let spelling = arg_value(args, "spelling")?;
            Some(format!(
                "`{spelling}` is the output statement, not a function: import the console function (`import from \"norma:console\" const print`, then write `print(...)`), or bind the value first (`let _ x ← …`, then `{spelling} x`)"
            ))
        }
        "unknown_identifier" => {
            let suggestion = arg_value(args, "suggestion")?;
            Some(format!(
                "declare the variable before use; did you mean {suggestion}?"
            ))
        }
        "variant_ambiguous" => {
            Some("qualify the variant with its union name (e.g. A.X)".to_owned())
        }
        _ => None,
    }
}

fn semantic_undefined_member_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "optional_receiver_requires_chain" => {
            Some("use ?. for optional chaining or !. for non-null access".to_owned())
        }
        "duplicate_variant_field" => {
            Some("provide each named variant field at most once".to_owned())
        }
        "missing_required_struct_field" => {
            Some("initialize every required struct field".to_owned())
        }
        "ratio_field_missing" => Some("initialize every required ratio field".to_owned()),
        "missing_variant_field_name" => {
            Some("name every variant argument when using named variant construction".to_owned())
        }
        "namespace_missing_export" => {
            Some("check the imported file's `@ publica` exports".to_owned())
        }
        "struct_field_requires_value" => Some("provide a value for the struct field".to_owned()),
        "unknown_field" | "unknown_struct_field" => Some("check the struct field name".to_owned()),
        "static_field_read_through_instance" => Some(
            "read a `const` member through the type name (`Type.field`), not an instance".to_owned(),
        ),
        "instance_field_read_through_type" => Some(
            "read an instance field through an instance; only a `const` member reads through the type name"
                .to_owned(),
        ),
        "unknown_method" => Some("check the receiver type and method name".to_owned()),
        "unknown_tuple_element_label" => {
            Some("use a label declared on the iuncta annotation, or index by position".to_owned())
        }
        "unknown_variant_field" => Some("check the variant field name".to_owned()),
        "length_is_method" => Some("call it as a method: `.length()`".to_owned()),
        "type_has_no_field" => Some(
            "built-in types expose methods, not fields; check the receiver type and member name"
                .to_owned(),
        ),
        "unknown_union_field" => Some("check the union's `@ commune` field name".to_owned()),
        "union_member_not_common" => Some(
            "narrow the receiver (`est M` branch, `discerne`) to recover per-member access"
                .to_owned(),
        ),
        _ => None,
    }
}

// HIR LOWERING

// ASSIGNMENT AND OWNERSHIP

/// Help for SEM065 `const_shared_into_var`.
///
/// `source` is `parameter` when the read-only root is a `ref`/unmarked
/// parameter, and `constant` otherwise (a `const` local or module constant).
/// `← own x` of a read-only parameter is itself SEM057
/// `own_argument_unmarked_parameter`, so the parameter help must name only
/// `← copy x`.
fn semantic_const_shared_into_var_help(args: &[DiagnosticArg]) -> String {
    if arg_value(args, "source") == Some("parameter") {
        "write `← copy x` for an independent value".to_owned()
    } else {
        "write `← copy x` for an independent value, or `← own x` to take it over".to_owned()
    }
}

/// Help for SEM066 `copy_nonduplicable_member`.
fn semantic_copy_nonduplicable_help(args: &[DiagnosticArg]) -> Option<String> {
    Some(match arg_value(args, "head") {
        Some(head) => format!(
            "{head} is a resource with identity: share the value without `copy`, or take the resource over with `own` where the consumer may own it"
        ),
        None =>
            "share the value without `copy`, or take the resource over with `own` where the consumer may own it"
                .to_owned(),
    })
}

// TYPE MISMATCH

// AIR PURITY

const AIR_PURITY_HELP: &str = "keep the v1 AIR subset pure: immutable locals, pure expressions, AIR-lane calls, and tensor arithmetic intrinsics only";

/// Return the v1-AIR-subset rejection reason for a recognized air-purity issue.
///
/// Shared by the message and help renderers so the recognized-issue set has
/// one source: a `Some(_)` reason enables both message formatting and the
/// shared baseline help, while `None` leaves both at their fallthrough.
fn air_purity_reason(args: &[DiagnosticArg]) -> Option<String> {
    Some(match arg_value(args, "issue")? {
        "air_unbounded_list_size" => {
            "a list with no upper size has no unrolled AIR form".to_owned()
        }
        "air_unbounded_integer" => {
            "the unbounded integer `inf` has no AIR representation".to_owned()
        }
        "async_function" => "async functions are outside the v1 AIR subset".to_owned(),
        "assignment_mutation" => "assignment is mutation outside the v1 AIR subset".to_owned(),
        "breakable_block" => "breakable blocks are outside the v1 AIR subset".to_owned(),
        "cede" => "`cede` is outside the v1 AIR subset".to_owned(),
        "cli_mounted_function" => "CLI-mounted functions are outside the v1 AIR subset".to_owned(),
        "closure" => "closures are outside the v1 AIR subset".to_owned(),
        "defaulted_parameter" => "defaulted parameters are outside the v1 AIR subset".to_owned(),
        "deferred_local_init" => {
            "deferred local initialization is outside the v1 AIR subset".to_owned()
        }
        "diagnostic_output" => {
            "diagnostic output is a host effect outside the v1 AIR subset".to_owned()
        }
        "dum_loop" => "`dum` loops are outside the v1 AIR subset".to_owned(),
        "expression_ad" => "expression-form ad is outside the v1 AIR subset".to_owned(),
        "expression_statement" => "expression statements are outside the v1 AIR subset".to_owned(),
        "generator_function" => "generator functions are outside the v1 AIR subset".to_owned(),
        "iace" => "`iace` is outside the v1 AIR subset".to_owned(),
        "inc_dec_mutation" => {
            "increment/decrement mutation is outside the v1 AIR subset".to_owned()
        }
        "input_read" => "input reads are host effects outside the v1 AIR subset".to_owned(),
        "itera_loop" => match arg_value(args, "causa") {
            Some(causa) => format!("`itera` loops are outside the v1 AIR subset ({causa})"),
            None => "`itera` loops are outside the v1 AIR subset".to_owned(),
        },
        "kernel_device_function" => {
            "kernel/device functions are outside the v1 AIR subset".to_owned()
        }
        "loop" => "loops are outside the v1 AIR subset".to_owned(),
        "loop_control" => "loop control is outside the v1 AIR subset".to_owned(),
        "method_call" => {
            let method = arg_value(args, "method")?;
            format!("method call `{method}` is outside the v1 AIR subset")
        }
        "missing_body" => "AIR-lane functions must have a body".to_owned(),
        "mori" => "`mori` is outside the v1 AIR subset".to_owned(),
        "mutable_varia" => "mutable `varia` bindings are outside the v1 AIR subset".to_owned(),
        "named_call_arg" => "named call arguments are outside the v1 AIR subset".to_owned(),
        "non_air_call" => {
            "calls from AIR-lane functions must target another AIR-lane function".to_owned()
        }
        "optional_null_chain" => {
            "optional/null assertion chains are outside the v1 AIR subset".to_owned()
        }
        "reducta_expression_unsupported" => {
            "reduction expressions are outside the v1 AIR subset".to_owned()
        }
        "reducta_first_unbounded_source" => {
            "first reductions need a source with a statically known extent in the v1 AIR subset"
                .to_owned()
        }
        "recoverable_handler" => "recoverable handlers are outside the v1 AIR subset".to_owned(),
        "recovery_type_mismatch" => {
            "a recovery arm must exit with the handled region's value type in the v1 AIR subset"
                .to_owned()
        }
        "reference_place_op" => {
            "reference/place operations are outside the v1 AIR subset".to_owned()
        }
        "rethrow_in_recovery" => {
            "`iace` inside a recovery arm is outside the v1 AIR subset".to_owned()
        }
        "runtime_assertion" => "runtime assertions are outside the v1 AIR subset".to_owned(),
        "runtime_conversion" => "runtime `↦` conversion is outside the v1 AIR subset".to_owned(),
        "runtime_provided_binding" => {
            "runtime-provided bindings are outside the v1 AIR subset".to_owned()
        }
        "runtime_variant_check" => {
            "runtime `est` variant check is outside the v1 AIR subset".to_owned()
        }
        "sort_expression_unsupported" => {
            "sort expressions are outside the v1 AIR subset".to_owned()
        }
        "spread_call_arg" => "spread call arguments are outside the v1 AIR subset".to_owned(),
        "tacet" => "`tacet` is outside the v1 AIR subset".to_owned(),
        "tensor_method_receiver" => {
            let method = arg_value(args, "method")?;
            format!(
                "tensor method `{method}` on a non-tensor receiver is outside the v1 AIR subset"
            )
        }
        "unapproved_effect" => "scan expressions are outside the v1 AIR subset".to_owned(),
        _ => return None,
    })
}

fn semantic_air_purity_help(args: &[DiagnosticArg]) -> Option<String> {
    air_purity_reason(args).map(|_| AIR_PURITY_HELP.to_owned())
}

// IMPORT PATHS

fn semantic_import_not_found_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "import_interna_cross_package" => {
            Some("import `@ interna` names only from a file in the same package".to_owned())
        }
        "import_module_private" => Some(
            "mark the declaration `@ publica` to export it, or `@ interna` for same-package use"
                .to_owned(),
        ),
        _ => None,
    }
}

// ANNOTATION CONTRACTS

fn semantic_invalid_annotation_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "contract_annotation_invalid" => Some("write `@ contract { target = \"evm\" }` on one class".to_owned()),
        "evm_contract_unsupported" => Some("the experimental EVM profile admits one field-free class with scalar methods and explicit public entries".to_owned()),
        "protecta_reserved" => {
            let publica = arg_value(args, "publica")?;
            let privata = arg_value(args, "privata")?;
            Some(format!(
                "use `{publica}` for explicit export intent or `{privata}` for file-private intent"
            ))
        }
        "empty_radix_typus_domain" | "missing_type_param" | "non_identifier_type_param" | "non_type_allowed" => {
            Some("use `@ radix typus T in textus octeti` or braced `@ radix { param = T, allowed = textus }` with at least one allowed type".to_owned())
        }
        "conflicting_lane"
        | "missing_directive"
        | "missing_lane_value"
        | "non_string_lane_value"
        | "unknown_directive"
        | "unknown_lane"
        | "unsupported_placement" => Some(
            "use `@ radix lane \"air\"`, `@ radix lane \"mir\"`, or `@ radix lane \"hir-direct\"` on a top-level functio"
                .to_owned(),
        ),
        "annotatio_misplaced" | "annotatio_invalid_marker" | "annotatio_duplicate_marker" => {
            Some("write `@ annotatio { target = functio }` immediately before a genus declaration".to_owned())
        }
        "annotatio_unknown_target" => Some("use `target = functio` in v1".to_owned()),
        "annotatio_invalid_field_type" | "annotatio_method_unsupported" => {
            Some("declare scalar fields only: `textus name`, `numerus n`, `fractus f`, or `bivalens flag`".to_owned())
        }
        "annotatio_unknown_contract" | "annotatio_not_a_contract" => {
            Some("import or declare the contract genus with `@ annotatio` before applying it".to_owned())
        }
        "annotatio_unknown_field" | "annotatio_duplicate_field" | "annotatio_missing_field" => {
            Some("match the braced payload fields to the contract genus field schema".to_owned())
        }
        "annotatio_invalid_payload" => {
            Some("use a string, number, boolean, or `nihil` literal matching the field type".to_owned())
        }
        "annotatio_invalid_attachment" => Some("apply this contract only to `functio` declarations".to_owned()),
        "commune_on_non_union" => {
            Some("declare shared fields in a union `@ commune` region".to_owned())
        }
        _ => None,
    }
}

fn semantic_conflicting_visibility_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "conflicting_visibility" => Some("use a single visibility tier per declaration".to_owned()),
        _ => None,
    }
}

// PATTERN COVERAGE

// WARNING SPECS

fn warning_spec(kind: WarningKind) -> DiagnosticSpec {
    match kind {
        WarningKind::UnusedVariable => DiagnosticSpec {
            code: "WARN001",
            help: Some("remove the variable or prefix it with '_'"),
        },
        WarningKind::UnusedImport => DiagnosticSpec {
            code: "WARN002",
            help: Some("remove the unused import"),
        },
        WarningKind::UnusedFunction => DiagnosticSpec {
            code: "WARN003",
            help: Some("remove the function or mark it as used"),
        },
        WarningKind::UnreachableCode => DiagnosticSpec {
            code: "WARN004",
            help: Some("remove unreachable code"),
        },
        WarningKind::UnnecessaryCast => DiagnosticSpec {
            code: "WARN005",
            help: Some("remove the redundant cast"),
        },
        WarningKind::TargetNoop => DiagnosticSpec {
            code: "WARN007",
            help: Some("remove the construct or gate it by target"),
        },
        WarningKind::UnusedMutRefParam => DiagnosticSpec {
            code: "WARN008",
            help: Some("change `mut` to `ref` if mutation is unnecessary"),
        },
        WarningKind::UnusedMoveParam => DiagnosticSpec {
            code: "WARN009",
            help: Some("change `own` to `ref` if ownership transfer is unnecessary"),
        },
        WarningKind::ExplicitIgnotumAnnotation => DiagnosticSpec {
            code: "WARN010",
            help: Some("prefer a concrete type annotation to keep type-checking precise"),
        },
        WarningKind::WildcardAbsorbsNull => DiagnosticSpec {
            code: "WARN011",
            help: Some("add an explicit `casu nihil` arm so the null case is handled deliberately"),
        },
        WarningKind::UnnecessaryVaria => DiagnosticSpec {
            code: "WARN013",
            help: Some("use `fixum` when the binding is never reassigned"),
        },
        WarningKind::FileInterfaceExportSkipped => DiagnosticSpec {
            code: "WARN014",
            help: Some(
                "use a supported export from the module, or wait for fuller file-interface coverage of this signature",
            ),
        },
        WarningKind::MonomorphicHoleSolvedToUnion => DiagnosticSpec {
            code: "WARN016",
            help: Some("use an explicit union type or a `∪` hole if a sum was intended"),
        },
        WarningKind::UnionHoleSolvedMonomorphically => DiagnosticSpec {
            code: "WARN017",
            help: Some("use `_` if only one type is intended"),
        },
        WarningKind::PreferStringTemplate => DiagnosticSpec {
            code: "WARN018",
            help: Some("use a string template (`\"…§…\"(args)`) for long concatenation chains"),
        },
        WarningKind::DiscouragedMutLocal => DiagnosticSpec {
            code: "WARN019",
            help: Some("declare the exclusive borrow as a `mut` parameter instead of a local"),
        },
        WarningKind::EmptyMarkerShadowing => DiagnosticSpec {
            code: "WARN021",
            help: Some(
                "choose a different binding name when the empty-collection marker is needed",
            ),
        },
        WarningKind::UnboundedTextusIndex => DiagnosticSpec {
            code: "WARN022",
            help: Some("convert to textus<size N> for O(1) indexing or iterate scalars"),
        },
        WarningKind::TypeTestMemberAbsent => DiagnosticSpec {
            code: "WARN023",
            help: Some("narrow with a member the union declares, or fix the tested type"),
        },
        WarningKind::UnionNoCommonMembers => DiagnosticSpec {
            code: "WARN025",
            help: Some(
                "narrow with `est`/`discerne` before any member access; the union itself is store-only",
            ),
        },
        WarningKind::UnionMethodSignatureShadowed => DiagnosticSpec {
            code: "WARN026",
            help: Some("align the constituent signatures or rename one method to keep it common"),
        },
        WarningKind::ComplexityBudgetExceeded => DiagnosticSpec {
            code: "WARN027",
            help: Some("split the function or raise the complexity budget for this check"),
        },
        WarningKind::RedundantMemberVisibility => DiagnosticSpec {
            code: "WARN028",
            help: Some("remove the annotation; members are public by default"),
        },
        WarningKind::CoverageUnchecked => DiagnosticSpec {
            code: "WARN029",
            help: Some("add a 'ceterum' arm, or split the match so each pattern kind is checkable"),
        },
        WarningKind::LeadingUnderscore => DiagnosticSpec {
            code: "WARN030",
            help: Some("drop the leading `_`; unmarked top-level declarations are already private"),
        },
        WarningKind::ClosableInCycle => DiagnosticSpec {
            code: "WARN031",
            help: Some("call `close()` explicitly, or break the reference cycle"),
        },
        WarningKind::PositionalLaneParam => DiagnosticSpec {
            code: "WARN032",
            help: Some(
                "bind the iteration coordinate with `apud` on a `filum` loop: `itera ex t apud [i, j] filum f sit v`",
            ),
        },
        WarningKind::UnprovenKernelIndex => DiagnosticSpec {
            code: "WARN033",
            help: Some("convert the index with `x ↦ 0‥N` to carry its range, or read with `get`"),
        },
        WarningKind::PreferGuillemetQuote => DiagnosticSpec {
            code: "WARN034",
            help: Some("write the string with «…»; guillemets need no escape for `\\\"` or `'`"),
        },
        WarningKind::MixedTemplateHoles => DiagnosticSpec {
            code: "WARN035",
            help: Some("number every hole (`§0`, `§1`, …) so each reads the argument you mean"),
        },
        WarningKind::TrapErrorChannelEmpty => DiagnosticSpec {
            code: "WARN036",
            help: Some("drop the boundary, or call something failable inside it"),
        },
    }
}

fn semantic_monomorphic_hole_solved_to_union_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "monomorphic_hole_solved_to_union" => {
            Some("use an explicit union type or a `∪` hole if a sum was intended".to_owned())
        }
        _ => None,
    }
}

fn semantic_keyword_shadowing_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue") {
        Some("value_literal_shadowing") => Some(format!(
            "choose a different binding name when the `{}` literal is needed",
            arg_value(args, "keyword")?
        )),
        _ => Some(
            "choose a different binding name when the empty-collection marker is needed".to_owned(),
        ),
    }
}

fn semantic_union_hole_solved_monomorphically_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "union_hole_solved_monomorphically" => {
            Some("use `_` if only one type is intended".to_owned())
        }
        _ => None,
    }
}

fn semantic_union_guardrail_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "union_no_common_members" => Some(
            "narrow with `est`/`discerne` before any member access; the union itself is store-only"
                .to_owned(),
        ),
        "union_method_signature_shadowed" => Some(
            "align the constituent signatures or rename one method to keep it common".to_owned(),
        ),
        _ => None,
    }
}

#[cfg(test)]
#[path = "catalog_semantic_issue_rows_test.rs"]
mod issue_rows_tests;
