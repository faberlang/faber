//! Semantic and warning diagnostic catalog entries.
//!
//! This module owns analysis-phase code mappings and structured message/help
//! rendering. The parent catalog owns reader-locale fallback diagnostics and
//! shared argument helpers.

#![allow(dead_code)]

use super::catalog::{arg_value, quote_arg, DiagnosticSpec};
use crate::semantic::{SemanticErrorKind, WarningKind};
use radix_diagnostics::DiagnosticArg;

// ANALYSIS KINDS

pub(crate) const SEMANTIC_ERROR_KINDS: &[SemanticErrorKind] = &[
    SemanticErrorKind::UndefinedVariable,
    SemanticErrorKind::UndefinedType,
    SemanticErrorKind::UndefinedFunction,
    SemanticErrorKind::UndefinedMember,
    SemanticErrorKind::DuplicateDefinition,
    SemanticErrorKind::ImportNotFound,
    SemanticErrorKind::CircularDependency,
    SemanticErrorKind::LoweringError,
    SemanticErrorKind::CliValidation,
    SemanticErrorKind::InvalidAnnotation,
    SemanticErrorKind::ConflictingVisibility,
    SemanticErrorKind::ShadowedVariable,
    SemanticErrorKind::TypeMismatch,
    SemanticErrorKind::InvalidOperandTypes,
    SemanticErrorKind::NotCallable,
    SemanticErrorKind::WrongArity,
    SemanticErrorKind::MissingTypeAnnotation,
    SemanticErrorKind::InvalidCast,
    SemanticErrorKind::InvalidConversion,
    SemanticErrorKind::UnavailableDeclaration,
    SemanticErrorKind::AirPurityViolation,
    SemanticErrorKind::ImmutableAssignment,
    SemanticErrorKind::InvalidAssignmentTarget,
    SemanticErrorKind::UseBeforeInit,
    SemanticErrorKind::BreakOutsideBreakable,
    SemanticErrorKind::ContinueOutsideLoop,
    SemanticErrorKind::ReturnOutsideFunction,
    SemanticErrorKind::MissingReturn,
    SemanticErrorKind::NonExhaustiveMatch,
    SemanticErrorKind::UnreachablePattern,
    SemanticErrorKind::DuplicatePattern,
    SemanticErrorKind::OmniaRequiresEnum,
    SemanticErrorKind::OmniaForbidsCatchall,
    SemanticErrorKind::UseAfterMove,
    SemanticErrorKind::BorrowOfMoved,
    SemanticErrorKind::MutableBorrowConflict,
    SemanticErrorKind::CannotMoveOut,
    SemanticErrorKind::LifetimeMismatch,
    SemanticErrorKind::AssignToImmutableBorrow,
    SemanticErrorKind::ModeMismatch,
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
    WarningKind::UnrecognizedCurataAllocator,
    WarningKind::MonomorphicHoleSolvedToUnion,
    WarningKind::UnionHoleSolvedMonomorphically,
    WarningKind::PreferStringTemplate,
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
        SemanticErrorKind::UndefinedVariable => {
            DiagnosticSpec { code: "SEM001", help: Some("declare the variable before use") }
        }
        SemanticErrorKind::UndefinedType => {
            DiagnosticSpec { code: "SEM002", help: Some("declare the type or import it") }
        }
        SemanticErrorKind::UndefinedFunction => {
            DiagnosticSpec { code: "SEM003", help: Some("declare the function or import it") }
        }
        SemanticErrorKind::UndefinedMember => {
            DiagnosticSpec { code: "SEM004", help: Some("check the member name and type") }
        }
        SemanticErrorKind::DuplicateDefinition => {
            DiagnosticSpec { code: "SEM005", help: Some("rename one of the definitions") }
        }
        SemanticErrorKind::ImportNotFound => DiagnosticSpec { code: "SEM006", help: Some("check the import path") },
        SemanticErrorKind::CircularDependency => {
            DiagnosticSpec { code: "SEM007", help: Some("break the cycle between modules") }
        }
        SemanticErrorKind::LoweringError => {
            DiagnosticSpec { code: "SEM008", help: Some("fix the AST construct for lowering") }
        }
        SemanticErrorKind::CliValidation => DiagnosticSpec {
            code: "SEM009",
            help: Some("fix the CLI annotation contract before emitting runnable CLI code"),
        },
        SemanticErrorKind::InvalidAnnotation => {
            DiagnosticSpec { code: "SEM018", help: Some("fix the annotation contract before semantic lowering") }
        }
        SemanticErrorKind::ConflictingVisibility => {
            DiagnosticSpec { code: "SEM019", help: Some("use a single visibility tier per declaration") }
        }
        SemanticErrorKind::ShadowedVariable => {
            DiagnosticSpec { code: "SEM055", help: Some("rename one of the variables") }
        }
        SemanticErrorKind::TypeMismatch => {
            DiagnosticSpec { code: "SEM010", help: Some("make the expression type match the expected type") }
        }
        SemanticErrorKind::InvalidOperandTypes => {
            DiagnosticSpec { code: "SEM011", help: Some("use compatible operand types") }
        }
        SemanticErrorKind::NotCallable => DiagnosticSpec { code: "SEM012", help: Some("call a function or closure") },
        SemanticErrorKind::WrongArity => {
            DiagnosticSpec { code: "SEM013", help: Some("adjust the number of arguments") }
        }
        SemanticErrorKind::MissingTypeAnnotation => {
            DiagnosticSpec { code: "SEM014", help: Some("add a type annotation") }
        }
        SemanticErrorKind::InvalidCast => DiagnosticSpec { code: "SEM015", help: Some("cast to a compatible type") },
        SemanticErrorKind::InvalidConversion => DiagnosticSpec {
            code: "SEM016",
            help: Some("use a valid conversion or provide inline recovery with `⇥`"),
        },
        SemanticErrorKind::UnavailableDeclaration => DiagnosticSpec {
            code: "SEM017",
            help: Some("use an available declaration or implement the marked surface"),
        },
        SemanticErrorKind::AirPurityViolation => DiagnosticSpec {
            code: "SEM059",
            help: Some("keep `@ radix lane \"air\" functions in the accepted pure subset"),
        },
        SemanticErrorKind::ImmutableAssignment => DiagnosticSpec {
            code: "SEM020",
            help: Some("use a `varia` binding to allow reassignment, or remove the extra assignment"),
        },
        SemanticErrorKind::InvalidAssignmentTarget => {
            DiagnosticSpec { code: "SEM021", help: Some("assign to a valid lvalue") }
        }
        SemanticErrorKind::UseBeforeInit => DiagnosticSpec {
            code: "SEM058",
            help: Some("assign the binding before reading it, or declare the slot `sponte`"),
        },
        SemanticErrorKind::BreakOutsideBreakable => {
            DiagnosticSpec { code: "SEM030", help: Some("use 'rumpe' inside dum, itera, fac, meus, or tuus") }
        }
        SemanticErrorKind::ContinueOutsideLoop => {
            DiagnosticSpec { code: "SEM031", help: Some("use 'perge' inside a loop") }
        }
        SemanticErrorKind::ReturnOutsideFunction => {
            DiagnosticSpec { code: "SEM032", help: Some("use 'redde' inside a function") }
        }
        SemanticErrorKind::MissingReturn => {
            DiagnosticSpec { code: "SEM033", help: Some("add a return statement or make the return type 'vacuum'") }
        }
        SemanticErrorKind::NonExhaustiveMatch => {
            DiagnosticSpec { code: "SEM040", help: Some("add missing cases or a 'ceterum' arm") }
        }
        SemanticErrorKind::UnreachablePattern => {
            DiagnosticSpec { code: "SEM041", help: Some("remove or reorder the unreachable pattern") }
        }
        SemanticErrorKind::DuplicatePattern => {
            DiagnosticSpec { code: "SEM042", help: Some("remove the duplicate pattern") }
        }
        SemanticErrorKind::OmniaRequiresEnum => {
            DiagnosticSpec { code: "SEM043", help: Some("use `omnia` only with a single enum scrutinee") }
        }
        SemanticErrorKind::OmniaForbidsCatchall => DiagnosticSpec {
            code: "SEM044",
            help: Some("list every variant explicitly instead of using `casu _` or `ceterum`"),
        },
        SemanticErrorKind::UseAfterMove => {
            DiagnosticSpec { code: "SEM050", help: Some("avoid using a value after it has been moved") }
        }
        SemanticErrorKind::BorrowOfMoved => {
            DiagnosticSpec { code: "SEM051", help: Some("borrow only from values that have not been moved") }
        }
        SemanticErrorKind::MutableBorrowConflict => {
            DiagnosticSpec { code: "SEM052", help: Some("avoid overlapping mutable borrows") }
        }
        SemanticErrorKind::CannotMoveOut => {
            DiagnosticSpec { code: "SEM053", help: Some("use a reference or clone instead of moving") }
        }
        SemanticErrorKind::LifetimeMismatch => {
            DiagnosticSpec { code: "SEM054", help: Some("ensure borrowed values live long enough") }
        }
        SemanticErrorKind::AssignToImmutableBorrow => {
            DiagnosticSpec { code: "SEM056", help: Some("declare the parameter as 'in' if mutation is intended") }
        }
        SemanticErrorKind::ModeMismatch => {
            DiagnosticSpec { code: "SEM057", help: Some("align parameter modes between caller and callee") }
        }
        SemanticErrorKind::Warning(kind) => warning_spec(kind),
    }
}

// ISSUE RENDERING

#[must_use]
pub fn semantic_message(kind: SemanticErrorKind, args: &[DiagnosticArg]) -> Option<String> {
    match kind {
        SemanticErrorKind::AirPurityViolation => semantic_air_purity_message(args),
        SemanticErrorKind::AssignToImmutableBorrow
        | SemanticErrorKind::BorrowOfMoved
        | SemanticErrorKind::CannotMoveOut
        | SemanticErrorKind::ModeMismatch
        | SemanticErrorKind::MutableBorrowConflict
        | SemanticErrorKind::UseAfterMove => semantic_borrow_message(args),
        SemanticErrorKind::BreakOutsideBreakable
        | SemanticErrorKind::ContinueOutsideLoop
        | SemanticErrorKind::ReturnOutsideFunction => semantic_outside_context_message(args),
        SemanticErrorKind::CliValidation => semantic_cli_validation_message(args),
        SemanticErrorKind::CircularDependency => semantic_circular_dependency_message(args),
        SemanticErrorKind::ConflictingVisibility => semantic_conflicting_visibility_message(args),
        SemanticErrorKind::DuplicateDefinition => semantic_duplicate_definition_message(args),
        SemanticErrorKind::UndefinedFunction => Some("undefined function".to_owned()),
        SemanticErrorKind::InvalidCast => Some("invalid cast".to_owned()),
        SemanticErrorKind::LifetimeMismatch => Some("lifetime mismatch".to_owned()),
        SemanticErrorKind::ImmutableAssignment | SemanticErrorKind::UseBeforeInit => {
            semantic_definite_assignment_message(args)
        }
        SemanticErrorKind::ImportNotFound => semantic_import_not_found_message(args),
        SemanticErrorKind::InvalidAssignmentTarget => semantic_invalid_assignment_target_message(args),
        SemanticErrorKind::InvalidAnnotation => semantic_invalid_annotation_message(args),
        SemanticErrorKind::InvalidOperandTypes => semantic_invalid_operand_types_message(args),
        SemanticErrorKind::LoweringError => semantic_lowering_error_message(args),
        SemanticErrorKind::ShadowedVariable => semantic_lint_message(args),
        SemanticErrorKind::DuplicatePattern
        | SemanticErrorKind::NonExhaustiveMatch
        | SemanticErrorKind::OmniaForbidsCatchall
        | SemanticErrorKind::OmniaRequiresEnum
        | SemanticErrorKind::UnreachablePattern => semantic_exhaustiveness_message(kind, args),
        SemanticErrorKind::MissingReturn => semantic_missing_return_message(args),
        SemanticErrorKind::MissingTypeAnnotation => semantic_missing_type_annotation_message(args),
        SemanticErrorKind::InvalidConversion => semantic_invalid_conversion_message(args),
        SemanticErrorKind::NotCallable => semantic_not_callable_message(args),
        SemanticErrorKind::TypeMismatch => semantic_type_mismatch_message(args),
        SemanticErrorKind::UnavailableDeclaration => semantic_unavailable_declaration_message(args),
        SemanticErrorKind::UndefinedMember => semantic_undefined_member_message(args),
        SemanticErrorKind::UndefinedType => semantic_undefined_type_message(args),
        SemanticErrorKind::UndefinedVariable => semantic_undefined_variable_message(args),
        SemanticErrorKind::WrongArity => semantic_wrong_arity_message(args),
        SemanticErrorKind::Warning(WarningKind::TargetNoop) => Some("target no-op".to_owned()),
        SemanticErrorKind::Warning(
            WarningKind::ExplicitIgnotumAnnotation
            | WarningKind::UnnecessaryCast
            | WarningKind::UnnecessaryVaria
            | WarningKind::UnreachableCode
            | WarningKind::UnusedFunction
            | WarningKind::UnusedImport
            | WarningKind::UnusedVariable,
        ) => semantic_lint_message(args),
        SemanticErrorKind::Warning(WarningKind::UnusedMoveParam | WarningKind::UnusedMutRefParam) => {
            semantic_borrow_message(args)
        }
        SemanticErrorKind::Warning(WarningKind::WildcardAbsorbsNull) => semantic_wildcard_absorbs_null_message(args),
        SemanticErrorKind::Warning(WarningKind::FileInterfaceExportSkipped) => {
            semantic_file_interface_export_skipped_message(args)
        }
        SemanticErrorKind::Warning(WarningKind::UnrecognizedCurataAllocator) => {
            semantic_unrecognized_curata_allocator_message(args)
        }
        SemanticErrorKind::Warning(WarningKind::MonomorphicHoleSolvedToUnion) => {
            semantic_monomorphic_hole_solved_to_union_message(args)
        }
        SemanticErrorKind::Warning(WarningKind::UnionHoleSolvedMonomorphically) => {
            semantic_union_hole_solved_monomorphically_message(args)
        }
        SemanticErrorKind::Warning(WarningKind::PreferStringTemplate) => semantic_prefer_string_template_message(args),
    }
}

pub fn semantic_help(kind: SemanticErrorKind, args: &[DiagnosticArg]) -> Option<String> {
    match kind {
        SemanticErrorKind::AirPurityViolation => semantic_air_purity_help(args),
        SemanticErrorKind::ConflictingVisibility => semantic_conflicting_visibility_help(args),
        SemanticErrorKind::InvalidConversion => semantic_invalid_conversion_help(args),
        SemanticErrorKind::InvalidAnnotation => semantic_invalid_annotation_help(args),
        SemanticErrorKind::InvalidOperandTypes => semantic_invalid_operand_types_help(args),
        SemanticErrorKind::MissingTypeAnnotation => semantic_missing_type_annotation_help(args),
        SemanticErrorKind::NotCallable => semantic_not_callable_help(args),
        SemanticErrorKind::DuplicateDefinition => semantic_duplicate_definition_help(args),
        SemanticErrorKind::UnavailableDeclaration => semantic_unavailable_declaration_help(args),
        SemanticErrorKind::UndefinedMember => semantic_undefined_member_help(args),
        SemanticErrorKind::ImportNotFound => semantic_import_not_found_help(args),
        SemanticErrorKind::UndefinedVariable => semantic_undefined_variable_help(args),
        SemanticErrorKind::WrongArity => semantic_wrong_arity_help(args),
        SemanticErrorKind::Warning(WarningKind::MonomorphicHoleSolvedToUnion) => {
            semantic_monomorphic_hole_solved_to_union_help(args)
        }
        SemanticErrorKind::Warning(WarningKind::UnionHoleSolvedMonomorphically) => {
            semantic_union_hole_solved_monomorphically_help(args)
        }
        _ => None,
    }
}

// CLI CONTRACT

fn semantic_cli_validation_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "alias_collides_with_path" => {
            let alias = arg_value(args, "alias")?;
            Some(format!("command alias '{alias}' collides with a command path"))
        }
        "cli_on_non_incipit" => Some("@ cli may only annotate an incipit entry point".to_owned()),
        "command_global_surface" => {
            Some("'ubique' options and operands must be declared on the @ cli entry point".to_owned())
        }
        "command_params_unsupported" => Some(
            "Phase 04 CLI commands must receive parsed values through 'argumenta <ident>', not ordinary parameters"
                .to_owned(),
        ),
        "duplicate_binding" => {
            let binding = arg_value(args, "binding")?;
            let label = arg_value(args, "label")?;
            Some(format!("duplicate CLI binding '{binding}' in {label} CLI surface"))
        }
        "duplicate_cli_entry" => Some("only one @ cli entry point is allowed".to_owned()),
        "duplicate_command_alias" => {
            let alias = arg_value(args, "alias")?;
            Some(format!("duplicate command alias '{alias}'"))
        }
        "duplicate_command_path" => {
            let path = arg_value(args, "path")?;
            Some(format!("duplicate command path '{path}'"))
        }
        "duplicate_long_flag" => {
            let long = arg_value(args, "long")?;
            let label = arg_value(args, "label")?;
            Some(format!("duplicate long flag '{long}' in {label} CLI surface"))
        }
        "duplicate_rest_operand" => Some("only one ceteri operand is allowed".to_owned()),
        "duplicate_short_flag" => {
            let short = arg_value(args, "short")?;
            let label = arg_value(args, "label")?;
            Some(format!("duplicate short flag '{short}' in {label} CLI surface"))
        }
        "global_operand_placement" => Some("'ubique' operands must be declared on the @ cli entry point".to_owned()),
        "global_option_placement" => Some("'ubique' options must be declared on the @ cli entry point".to_owned()),
        "imperium_on_non_func" => Some("@ imperium may only annotate a functio declaration".to_owned()),
        "invalid_command_alias" => {
            let alias = arg_value(args, "alias")?;
            Some(format!("invalid command alias '{alias}'"))
        }
        "invalid_command_path" => {
            let path = arg_value(args, "path")?;
            Some(format!("invalid command path '{path}'"))
        }
        "invalid_long_flag" => {
            let long = arg_value(args, "long")?;
            let binding = arg_value(args, "binding")?;
            Some(format!("invalid long flag '{long}' for option '{binding}'"))
        }
        "invalid_short_flag" => {
            let short = arg_value(args, "short")?;
            let binding = arg_value(args, "binding")?;
            Some(format!("invalid short flag '{short}' for option '{binding}'"))
        }
        "list_element_not_named" => Some("CLI list types must use a named element type".to_owned()),
        "local_operand_global_collision" => {
            let label = arg_value(args, "label")?;
            let binding = arg_value(args, "binding")?;
            Some(format!("{label} operand '{binding}' collides with a global CLI binding"))
        }
        "local_option_global_collision" => {
            let label = arg_value(args, "label")?;
            let binding = arg_value(args, "binding")?;
            Some(format!("{label} option '{binding}' collides with a global CLI binding"))
        }
        "missing_entry_argument" => {
            Some("@ cli programs must use an explicit 'incipit argumenta <ident>' entry point".to_owned())
        }
        "malformed_annotation" => {
            let annotation = arg_value(args, "annotation")?;
            let field = arg_value(args, "field")?;
            let expected = arg_value(args, "expected")?;
            Some(format!("{annotation} field `{field}` must be {expected}"))
        }
        "missing_option_flag" => {
            let label = arg_value(args, "label")?;
            let binding = arg_value(args, "binding")?;
            Some(format!("{label} option '{binding}' needs brevis or longum"))
        }
        "mounted_module_root_cli" => {
            Some("mounted CLI command modules must not declare their own @ cli entry point".to_owned())
        }
        "nested_module_mount" => {
            Some("@ imperia module mounts may only be declared on the root CLI entry point".to_owned())
        }
        "operand_after_rest" => {
            let binding = arg_value(args, "binding")?;
            Some(format!("operand '{binding}' appears after ceteri operand"))
        }
        "orphan_cli_surface" => {
            Some("@ optio and @ operandus must annotate @ cli incipit or @ imperium functio".to_owned())
        }
        "rest_not_final" => Some("ceteri operand must be the final operand".to_owned()),
        "subcommand_local_surface" => {
            Some("subcommand CLI programs may only declare top-level options or operands with 'ubique'".to_owned())
        }
        "subcommand_root_body" => {
            Some("subcommand CLI programs must use an empty root incipit body in Phase 04".to_owned())
        }
        "unsupported_list_element_type" => {
            let ty = arg_value(args, "type")?;
            Some(format!("unsupported CLI list element type '{ty}'"))
        }
        "unsupported_type" => {
            let ty = arg_value(args, "type")?;
            Some(format!("unsupported CLI type '{ty}'"))
        }
        "unsupported_type_shape" => Some("unsupported CLI type shape".to_owned()),
        _ => None,
    }
}

// MODULE GRAPH

fn semantic_circular_dependency_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "type_alias_cycle" => Some("type alias cycle".to_owned()),
        _ => None,
    }
}

// CONTROL FLOW

fn semantic_outside_context_message(args: &[DiagnosticArg]) -> Option<String> {
    let keyword = arg_value(args, "keyword")?;
    let context = arg_value(args, "context")?;
    Some(format!("{keyword} outside {context}"))
}

fn semantic_missing_return_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "control_flow_fallthrough" => Some("not all control-flow paths return a value".to_owned()),
        "redde_requires_return_type" => {
            let keyword = arg_value(args, "keyword")?;
            Some(format!("{keyword} requires an explicit normal return type"))
        }
        _ => None,
    }
}

// TYPE ANNOTATIONS AND CALLS

fn semantic_missing_type_annotation_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "cannot_infer_alternate_exit_type" => Some("cannot infer alternate-exit type".to_owned()),
        "cannot_infer_constant_type" => Some("cannot infer constant type".to_owned()),
        "cannot_infer_expression_type" => Some("cannot infer expression type".to_owned()),
        "cannot_infer_field_type" => Some("cannot infer field type".to_owned()),
        "cannot_infer_return_type" => Some("cannot infer return type".to_owned()),
        "cannot_infer_tensor_shape" => Some("cannot infer tensor shape".to_owned()),
        "cannot_infer_variable_type" => Some("cannot infer variable type".to_owned()),
        "directional_view_type_not_registered" => Some("directional view type not registered".to_owned()),
        "explicit_type_arg_inference_unsupported" => {
            Some("partial inference in explicit type arguments is not supported".to_owned())
        }
        "scrinium_type_not_registered" => Some("scrinium type not registered".to_owned()),
        "sermo_type_not_registered" => Some("sermo type not registered".to_owned()),
        "vacua_requires_declared_type" => Some("vacua requires an explicit declared type".to_owned()),
        _ => None,
    }
}

fn semantic_missing_type_annotation_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "cannot_infer_tensor_shape" => Some("provide a concrete tensor or sparsa shape".to_owned()),
        "cannot_infer_alternate_exit_type"
        | "cannot_infer_constant_type"
        | "cannot_infer_expression_type"
        | "cannot_infer_field_type"
        | "cannot_infer_return_type"
        | "cannot_infer_variable_type" => Some("add an explicit type annotation".to_owned()),
        "directional_view_type_not_registered" | "scrinium_type_not_registered" | "sermo_type_not_registered" => {
            Some("ensure built-in frame types are registered before typechecking".to_owned())
        }
        "explicit_type_arg_inference_unsupported" => {
            Some("replace inferred explicit type arguments with concrete types".to_owned())
        }
        "vacua_requires_declared_type" => Some("add a declared collection type for vacua".to_owned()),
        _ => None,
    }
}

fn semantic_wrong_arity_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "callee_type_args_unsupported" => Some("callee does not accept type arguments".to_owned()),
        "gpu_builtin_expects_no_arguments" => Some("GPU builtin call expects no arguments".to_owned()),
        "method_accepts_no_arguments" => {
            let method = arg_value(args, "method")?;
            Some(format!("{method} accepts no arguments"))
        }
        "variant_pattern_arity_mismatch" => Some("variant pattern arity mismatch".to_owned()),
        "vector_constructor_width_arity" => {
            Some("vector constructor argument count must match vector width".to_owned())
        }
        "wrong_number_of_arguments" => Some("wrong number of arguments".to_owned()),
        "wrong_number_of_type_arguments" => Some("wrong number of type arguments".to_owned()),
        _ => None,
    }
}

fn semantic_wrong_arity_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "callee_type_args_unsupported" => Some("remove the explicit type argument list".to_owned()),
        "gpu_builtin_expects_no_arguments" | "method_accepts_no_arguments" => {
            Some("remove the call arguments".to_owned())
        }
        "variant_pattern_arity_mismatch" => Some("match the pattern field count to the variant".to_owned()),
        "vector_constructor_width_arity" => Some("provide one constructor element per vector lane".to_owned()),
        "wrong_number_of_arguments" => Some("adjust the number of arguments".to_owned()),
        "wrong_number_of_type_arguments" => Some("adjust the number of type arguments".to_owned()),
        _ => None,
    }
}

fn semantic_not_callable_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "callee_not_callable" => Some("callee is not callable".to_owned()),
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

fn semantic_invalid_conversion_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "intervallum_clamp_recovery_unsupported" => {
            Some("intervallum clamp conversio does not support `⇥` recovery".to_owned())
        }
        "intervallum_target_not_range" => {
            Some("intervallum conversio target must be a range or intervallum value".to_owned())
        }
        "intervallum_clamp_bound_mismatch" => {
            Some("intervallum clamp requires matching bound types".to_owned())
        }
        "sermo_materialize_target_unsupported" => Some(
            "sermo materialization target is not supported; expected vacuum, valor, scalar (numerus, fractus, bivalens, textus, ascii, octeti, instans), or lista<T>".to_owned(),
        ),
        "intervallum_materialize_lista_element_mismatch" => {
            Some("intervallum materialize requires matching lista element type".to_owned())
        }
        "intervallum_materialize_tensor_element_mismatch" => {
            Some("intervallum materialize requires matching tensor element type".to_owned())
        }
        "valor_to_sized_numeric_unsupported" => {
            Some("valor extraction to sized numeric targets requires an explicit width policy".to_owned())
        }
        "lista_to_vector_literal_required" => {
            Some("vector materialization requires a lista literal source".to_owned())
        }
        "lista_to_vector_element_mismatch" => {
            Some("lista to vector conversio requires matching element types".to_owned())
        }
        "lista_to_vector_literal_width" => {
            Some("lista to vector conversio requires a literal vector width".to_owned())
        }
        "matrix_materialize_unsupported" => Some("matrix materialization is not supported yet".to_owned()),
        "valor_to_regex_unsupported" => {
            Some("valor to regex conversion has no default pattern; use `valor ↦ textus ↦ regex`".to_owned())
        }
        "valor_to_cursor_boundary" => Some(
            "valor to cursor conversion crosses the eager/lazy boundary; materialize via `valor ↦ lista` then `lista ↦ cursor`".to_owned(),
        ),
        "valor_to_promissum_boundary" => Some(
            "valor to promissum conversion crosses the async/sync boundary; valor is synchronous — wrap the producer in `@ futura`".to_owned(),
        ),
        "promissum_to_valor_boundary" => Some(
            "promissum to valor conversion crosses the async/sync boundary; resolve with `.exspecta()` before boxing".to_owned(),
        ),
        "cursor_to_valor_boundary" => Some(
            "cursor to valor conversion crosses the eager/lazy boundary; materialize via `cursor ↦ lista` then `lista ↦ valor`".to_owned(),
        ),
        "numeric_to_octeti_unspecified" => Some(
            "conversion to octeti from numerus, fractus, or bivalens is unspecified (byte width and endianness); use `chorda` or explicit byte verbs instead".to_owned(),
        ),
        "octeti_to_numerus_unspecified" => Some(
            "conversion from octeti to numerus is unspecified (byte width and endianness); use `chorda` or explicit byte verbs instead".to_owned(),
        ),
        "octeti_to_fractus_unspecified" => Some(
            "conversion from octeti to fractus is unspecified (IEEE width and endianness); use `chorda` or explicit byte verbs instead".to_owned(),
        ),
        "octeti_to_bivalens_unsupported" => {
            Some("conversion from octeti to bivalens has no default meaning; decode to textus or compare bytes explicitly".to_owned())
        }
        "promissum_target_boundary" => Some(
            "conversion to promissum crosses the async/sync boundary; use `@ futura` or `.tum()` for async wrapping".to_owned(),
        ),
        "promissum_source_boundary" => Some(
            "conversion from promissum crosses the async/sync boundary; use `.exspecta()` or `@ futura` `.tum()` for async resolution".to_owned(),
        ),
        "sparsa_collection_unsupported" => Some(
            "sparsa conversion has no default lista meaning; use densata() before lista conversion or an explicit sparse helper".to_owned(),
        ),
        "sparsa_tensor_element_mismatch" => Some(
            "tensor and sparsa conversio requires the same element type; convert element width explicitly before changing representation".to_owned(),
        ),
        "sparsa_tensor_shape_mismatch" => {
            Some("tensor and sparsa conversio requires the same Figura shape; reshape explicitly before `↦`".to_owned())
        }
        "tensor_to_scalar_unsupported" => Some(
            "tensor to scalar conversion has no default element pick; use `accipe(index)` for indexed reads".to_owned(),
        ),
        "scalar_to_tensor_unsupported" => {
            Some("scalar to tensor conversion has no default broadcast shape; use `crea(shape, fill)` or `structa`".to_owned())
        }
        "tensor_shape_mismatch" => Some("tensor shape conversion is not supported for this pair".to_owned()),
        "tensor_element_mismatch" => Some("tensor element-type conversion is not supported for this pair".to_owned()),
        "conversio_failure_channel_textus_required" => {
            Some("propagating conversio failure requires a `textus` alternate-exit channel".to_owned())
        }
        "bivalens_recovery_unsupported" => {
            Some("truthiness conversion to bivalens does not support `⇥` recovery because it cannot fail".to_owned())
        }
        "regex_operand_unsupported" => Some("regex conversion requires textus or ascii operand".to_owned()),
        "regex_pattern_too_long" => {
            // Cap constant: `REGEX_PATTERN_MAX_BYTES` (1024) in
            // semantic/passes/typecheck/convert.rs.
            let length = arg_value(args, "length")?;
            let max = arg_value(args, "max")?;
            Some(format!("regex literal pattern is {length} bytes, exceeding the {max}-byte cap"))
        }
        "regex_recovery_unsupported" => Some(
            "regex conversion does not support `⇥` recovery; regex has no literal form, so there is no target-typed value to substitute".to_owned(),
        ),
        "valor_verte_unsupported" => Some(
            "valor ∷ T is not supported — a dynamic value's shape cannot be asserted at compile time. Use `↦ T` for runtime extraction (with `⇥` for recovery).".to_owned(),
        ),
        "lista_to_tabula_unsupported" => Some(
            "lista to tabula conversion has no default key/value derivation; use a tabula constructor verb or `.in.tabula(k, v)`; pair-destructuring default deferred".to_owned(),
        ),
        "cross_collection_unsupported" => {
            Some("cross-collection conversion has no default `↦` meaning; use an explicit stdlib verb".to_owned())
        }
        "lista_to_tensor_nested" => Some(
            "lista to tensor conversio requires a flat lista; flatten nested lista values explicitly before `↦`"
                .to_owned(),
        ),
        "lista_to_tensor_element_mismatch" => {
            Some("lista to tensor conversio requires matching element types; convert elements explicitly before `↦`".to_owned())
        }
        "rectangular_lista_literal_target" => {
            Some("rectangular lista literal conversio requires a rank-2 literal tensor Figura target".to_owned())
        }
        "rectangular_lista_literal_ragged" => {
            Some("ragged nested lista literal cannot convert to tensor; use crea or an explicit flatten verb".to_owned())
        }
        "rectangular_lista_literal_row_length" => {
            Some("rectangular lista literal row lengths disagree".to_owned())
        }
        "rectangular_lista_literal_shape" => {
            Some("rectangular lista literal shape does not match conversio target".to_owned())
        }
        "rectangular_lista_literal_spread" => {
            Some("rectangular lista literal conversio does not allow spread elements".to_owned())
        }
        "rectangular_lista_literal_leaf" => {
            Some("rectangular lista literal conversio requires literal leaf elements".to_owned())
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
        "bivalens_recovery_unsupported" => {
            Some("remove the recovery arm; use the truthiness result directly".to_owned())
        }
        "regex_operand_unsupported" => Some("convert textus or ascii values to regex".to_owned()),
        "regex_pattern_too_long" => Some("shorten the literal pattern or build it dynamically".to_owned()),
        "regex_recovery_unsupported" => Some("remove the recovery arm from this regex conversion".to_owned()),
        "matrix_materialize_unsupported" => {
            Some("keep matrix values as typed parameters until matrix construction lowering is implemented".to_owned())
        }
        "valor_verte_unsupported" => Some("use runtime `↦ T` conversion with `⇥` recovery instead".to_owned()),
        "lista_to_tabula_unsupported" => Some("construct the tabula with explicit key/value selection".to_owned()),
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

fn semantic_invalid_operand_types_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "range_step_must_be_numeric" => Some("range step must be numeric".to_owned()),
        "intervallum_bound_required" => Some(crate::semantic::INTERVALLUM_BOUND_MSG.to_owned()),
        "intervallum_longitudo_bound" => {
            Some("intervallum longitudo requires numerus bounds until momentum ships".to_owned())
        }
        "intervallum_helper_bound_unsupported" => {
            Some("intervallum helper operations currently require bare numerus bounds".to_owned())
        }
        "numeric_operands_required" => Some("numeric operands required".to_owned()),
        "numeric_arithmetic_operands_identical" => {
            Some("numeric arithmetic operands must have identical types; use ↦ to convert explicitly".to_owned())
        }
        "modular_word_family_mismatch" => {
            Some("modular word arithmetic requires two modulus operands of the same width".to_owned())
        }
        "ascii_literal_invalid_escape" => Some("ascii literal contains invalid escape sequence".to_owned()),
        "ascii_literal_non_ascii_scalar" => {
            let character = arg_value(args, "character")?;
            Some(format!("ascii literal contains non-ASCII character '{character}'"))
        }
        "ascii_literal_unescaped_control" => {
            let codepoint = arg_value(args, "codepoint")?;
            Some(format!("ascii literal contains unescaped control character U+{codepoint}"))
        }
        "ascii_literal_too_long" => {
            // Mirrors ASCII_LITERAL_MAX_PAYLOAD_BYTES in typecheck/lookup.rs.
            Some("ascii literal exceeds the maximum payload of 64 bytes".to_owned())
        }
        "boolean_operands_required" => Some("boolean operands required".to_owned()),
        "atomic_method_requires_atomic_receiver" => Some("atomic operation requires an atomic receiver".to_owned()),
        "unsupported_atomic_method" => Some("unsupported atomic operation".to_owned()),
        "bitwise_method_requires_numerus_receiver" => {
            Some("bitwise operator methods require numerus receiver".to_owned())
        }
        "deref_requires_reference" => Some("deref requires reference".to_owned()),
        "densata_requires_sparsa_receiver" => Some("densata requires sparsa receiver".to_owned()),
        "ascii_literal_template_application" => Some("ascii literals do not support template application".to_owned()),
        "directional_view_method_type_args" => {
            Some("directional view method does not accept type arguments".to_owned())
        }
        "dynamic_callee_type_args" => Some("dynamic callee does not accept type arguments".to_owned()),
        "dynamic_receiver_method_type_args" => {
            Some("dynamic receiver does not accept method type arguments".to_owned())
        }
        "failable_call_requires_handler" => {
            Some("failable call requires a ⇥ declaring context or a local handler".to_owned())
        }
        "generic_type_arg_domain" => {
            let rejected = arg_value(args, "rejected")?;
            let param = arg_value(args, "param")?;
            let accepted = arg_value(args, "accepted")?;
            Some(format!(
                "type argument {rejected} is not allowed for {param}; expected one of {accepted}"
            ))
        }
        "gpu_builtin_device_function_required" => {
            Some("GPU builtin call is only valid inside device functions".to_owned())
        }
        "NUCLEUM_FRAGMENT_CONTEXT" => nucleum_fragment_context_message(args),
        "NUCLEUM_FRAGMENT_EFFECT" => nucleum_fragment_effect_message(args),
        "gpu_builtin_type_args" => Some("GPU builtin call does not accept type arguments".to_owned()),
        "inferred_callee_type_args" => Some("inferred callee does not accept type arguments".to_owned()),
        "intrinsic_method_type_args" => Some("intrinsic method does not accept type arguments".to_owned()),
        "integer_operands_required" => Some("integer operands required".to_owned()),
        "instans_ordering_operands_required" => Some("incompatible instans operands".to_owned()),
        "json_duplicate_key" => {
            let key = arg_value(args, "key")?;
            Some(format!("duplicate JSON key \"{key}\""))
        }
        "method_call_type_args" => Some("method call does not accept type arguments".to_owned()),
        "numeric_lista_elements_required" => Some("numeric lista elements required".to_owned()),
        "octeti_literal_invalid_character" => {
            let character = arg_value(args, "character")?;
            Some(format!("octeti literal contains invalid character '{character}'"))
        }
        "octeti_literal_odd_digit_count" => Some("octeti literal has odd hex digit count".to_owned()),
        "octeti_literal_template_application" => Some("octeti literals do not support template application".to_owned()),
        "sermo_view_payload_type_arg_count" => {
            let method = arg_value(args, "method")?;
            Some(format!("{method} accepts at most one payload type argument"))
        }
        "template_application_type_args" => Some("template application does not accept type arguments".to_owned()),
        "variant_constructor_type_args" => Some("variant constructor does not accept type arguments".to_owned()),
        "same_width_integer_operands_required" => Some("same-width integer operands required".to_owned()),
        "postfix_inc_dec_numerus_operand" => Some("postfix increment/decrement requires numerus operand".to_owned()),
        "array_spread_requires_lista" => Some("array spread requires lista operand".to_owned()),
        "struct_computed_key_forbidden" => Some("computed keys are not valid in struct construction".to_owned()),
        "struct_spread_forbidden" => Some("spread fields are not valid in struct construction".to_owned()),
        "vector_constructor_forbids_spread" => Some("vector constructor does not accept spread arguments".to_owned()),
        "object_spread_requires_tabula" => Some("object spread requires tabula operand".to_owned()),
        "interval_bound_single_scalar" => Some("interval bound must be a single Unicode scalar".to_owned()),
        "numeric_operand_required" => Some("numeric operand required".to_owned()),
        "numeric_value_equality_operands_required" => {
            Some("≈ requires numeric operands; use ≡ for exact type equality or widen with ↦".to_owned())
        }
        "boolean_operand_required" => Some("boolean operand required".to_owned()),
        "condition_must_be_bivalens" => Some("condition must be bivalens".to_owned()),
        "integer_operand_required" => Some("integer operand required".to_owned()),
        "range_operand_must_be_numeric" => Some("range operand must be numeric".to_owned()),
        "scalar_text_operands_required" => Some("scalar text operands required".to_owned()),
        "string_operands_required" => Some("string operands required".to_owned()),
        "object_field_requires_value" => Some("object field requires value".to_owned()),
        "vacua_requires_collection_type" => Some("vacua requires a collection type".to_owned()),
        "textus_index_must_be_numerus_or_range" => Some("textus index must be numerus or range".to_owned()),
        "array_index_must_be_numerus" => Some("array index must be numerus".to_owned()),
        "indexing_requires_array_or_map" => Some("indexing requires array or map".to_owned()),
        "tensor_index_vector_integer_elements" => Some(crate::semantic::TENSOR_INDEX_VECTOR_MSG.to_owned()),
        "tensor_scalar_index_rank" => Some(crate::semantic::tensor_bracket::TENSOR_SCALAR_INDEX_RANK_MSG.to_owned()),
        "tensor_bracket_index_integer_or_lista" => {
            Some(crate::semantic::tensor_bracket::TENSOR_BRACKET_INDEX_MSG.to_owned())
        }
        "tensor_arithmetic_numeric_element_required" => Some(crate::semantic::TENSOR_ELEMENT_NUMERIC_MSG.to_owned()),
        "matrix_arithmetic_numeric_element_required" => {
            Some("matrix arithmetic requires numeric element types".to_owned())
        }
        "tensor_receiver_structa_renamed" => {
            Some(crate::semantic::tensor_type_directed::TENSOR_STRUCTA_RENAMED_MSG.to_owned())
        }
        "tensor_receiver_creata_type_args" => {
            Some(crate::semantic::tensor_type_directed::TENSOR_RECEIVER_CREATA_TYPE_ARGS_MSG.to_owned())
        }
        "tensor_crea_argument_order" => {
            Some(crate::semantic::tensor_type_directed::TENSOR_CREA_ARG_ORDER_MSG.to_owned())
        }
        "tensor_structa_forma_removed" => {
            Some(crate::semantic::tensor_type_directed::TENSOR_STRUCTA_FORMA_OLD_FORM_MSG.to_owned())
        }
        "tensor_creata_runtime_shape_removed" => {
            Some(crate::semantic::tensor_type_directed::TENSOR_CREATA_OLD_FORM_MSG.to_owned())
        }
        "tensor_creata_complete_type_arg_required" => Some(
            "tensor.creata requires a complete tensor type argument; use tensor.creata<CompleteTensorType>(fill)"
                .to_owned(),
        ),
        "tensor_formata_complete_type_arg_required" => {
            Some("formata requires a complete tensor type argument; use grid.formata<CompleteTensorType>()".to_owned())
        }
        "tensor_formata_forbids_value_args" => {
            Some(crate::semantic::tensor_type_directed::TENSOR_FORMA_VALUE_ARGS_MSG.to_owned())
        }
        "tensor_type_arg_incomplete" => {
            Some(crate::semantic::tensor_type_directed::TENSOR_TYPE_ARG_INCOMPLETE_MSG.to_owned())
        }
        "glyph_operands_unsupported" => {
            let op = arg_value(args, "op")?;
            Some(format!("glyph product operator {op} requires built-in vector/matrix/tensor operands with the pinned rank rules"))
        }
        "glyph_static_shape_required" => {
            let op = arg_value(args, "op")?;
            Some(format!("glyph product operator {op} requires statically known shapes; shape-generic operands are deferred to Phase 2"))
        }
        "glyph_op_numeric_element_required" => {
            let op = arg_value(args, "op")?;
            Some(format!("glyph product operator {op} requires numeric element types"))
        }
        "glyph_element_type_mismatch" => {
            let op = arg_value(args, "op")?;
            Some(format!(
                "glyph product operator {op} requires strictly equal element types across both operands"
            ))
        }
        "glyph_inner_dim_mismatch" => {
            let op = arg_value(args, "op")?;
            Some(format!("glyph product operator {op} requires matching inner dimensions"))
        }
        "glyph_cross_width_mismatch" => {
            Some("cross product × requires both operands to have the same width".to_owned())
        }
        "glyph_cross_width_three_required" => {
            Some("cross product × is pinned to 3-vectors; other widths are rejected in Phase 1".to_owned())
        }
        "glyph_hadamard_shapes_mismatch" => {
            Some("Hadamard product ⊙ requires operands with identical shapes".to_owned())
        }
        "gradient_expression_unsupported" => {
            Some("postfix gradient selection (`call(...) ∇ [places]`) is not lowered until the tensor-glyphs 2B autograd surface lands; no operand, selector, or result rule is active in Phase 2A".to_owned())
        }
        _ => None,
    }
}

fn semantic_invalid_operand_types_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "range_step_must_be_numeric" => Some("use a numeric range step".to_owned()),
        "intervallum_bound_required" => Some("use numerus, fractus, instans, or sized scalar bounds".to_owned()),
        "intervallum_longitudo_bound" => Some("use numerus intervallum bounds for longitudo today".to_owned()),
        "intervallum_helper_bound_unsupported" => {
            Some("use bare numerus intervallum bounds for expansion helpers today".to_owned())
        }
        "numeric_operands_required" => Some("use numeric operands for this operator".to_owned()),
        "modular_word_width_required" => {
            Some("write the modular word type as modulus<u8>, modulus<u16>, modulus<u32>, or modulus<u64>".to_owned())
        }
        "modular_word_width_shape" => Some("use a bare unsigned width marker such as u32".to_owned()),
        "unsupported_modular_word_width" => Some(
            "use an unsigned modular word width: modulus<u8>, modulus<u16>, modulus<u32>, or modulus<u64>".to_owned(),
        ),
        "modular_word_unsigned_width_required" => Some("use an unsigned integer width marker such as u32".to_owned()),
        "numeric_arithmetic_operands_identical" => {
            Some("convert operands explicitly before applying this numeric operator".to_owned())
        }
        "modular_word_family_mismatch" => Some(
            "convert both operands to the same modulus width, or keep both operands in the numerus family".to_owned(),
        ),
        "ascii_literal_invalid_escape" => Some("use supported ASCII literal escape sequences".to_owned()),
        "ascii_literal_non_ascii_scalar" => {
            Some("use textus for Unicode text or keep ascii payloads ASCII-only".to_owned())
        }
        "ascii_literal_unescaped_control" => Some("escape control characters in ascii literals".to_owned()),
        "ascii_literal_too_long" => Some("shorten the ascii literal or use a textus literal instead".to_owned()),
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
        "failable_call_requires_handler" => Some("handle the call with fac/cape or declare a function ⇥".to_owned()),
        "generic_type_arg_domain" => {
            Some("choose one of the allowed type arguments for this generic parameter".to_owned())
        }
        "gpu_builtin_device_function_required" => {
            Some("call GPU builtins only inside @ nucleum or @ nucleum fragment functions".to_owned())
        }
        "NUCLEUM_FRAGMENT_CONTEXT" => nucleum_fragment_context_help(args),
        "NUCLEUM_FRAGMENT_EFFECT" => nucleum_fragment_effect_help(args),
        "integer_operands_required" => Some("use integer operands for this operator".to_owned()),
        "instans_ordering_operands_required" => Some("compare instans values with instans values".to_owned()),
        "json_duplicate_key" => Some("remove or rename the duplicate JSON object key".to_owned()),
        "numeric_lista_elements_required" => Some("use numeric elements for this lista intrinsic".to_owned()),
        "octeti_literal_invalid_character" => Some("use only hexadecimal digits in octeti literals".to_owned()),
        "octeti_literal_odd_digit_count" => Some("provide complete byte pairs in octeti literals".to_owned()),
        "octeti_literal_template_application" => {
            Some("use textus formatting instead of octeti literal calls".to_owned())
        }
        "same_width_integer_operands_required" => {
            Some("convert integer operands to the same width before applying this operator".to_owned())
        }
        "postfix_inc_dec_numerus_operand" => Some("apply postfix increment/decrement to numerus values".to_owned()),
        "array_spread_requires_lista" => Some("spread a lista value inside lista construction".to_owned()),
        "struct_computed_key_forbidden" => Some("use declared struct field names in struct construction".to_owned()),
        "struct_spread_forbidden" => Some("provide each required struct field explicitly".to_owned()),
        "vector_constructor_forbids_spread" => {
            Some("provide explicit vector elements instead of spread arguments".to_owned())
        }
        "object_spread_requires_tabula" => Some("spread a tabula value inside tabula construction".to_owned()),
        "interval_bound_single_scalar" => {
            Some("pass textus or ascii values that contain exactly one Unicode scalar".to_owned())
        }
        "numeric_operand_required" => Some("use a numeric operand for this operator".to_owned()),
        "numeric_value_equality_operands_required" => Some(
            "use numeric operands for approximate equality, or use exact equality for non-numeric values".to_owned(),
        ),
        "boolean_operand_required" => Some("use a bivalens operand for this operator".to_owned()),
        "condition_must_be_bivalens" => Some("use a bivalens expression as the condition".to_owned()),
        "integer_operand_required" => Some("use an integer operand for this operator".to_owned()),
        "range_operand_must_be_numeric" => Some("compare numeric values with intervallum ranges".to_owned()),
        "scalar_text_operands_required" => Some("compare single-scalar textus or ascii operands".to_owned()),
        "string_operands_required" => Some("use textus operands for string addition".to_owned()),
        "object_field_requires_value" => Some("provide an explicit value for this object field".to_owned()),
        "vacua_requires_collection_type" => {
            Some("give vacua a lista, tabula, copia, tensor, or sparsa context".to_owned())
        }
        "textus_index_must_be_numerus_or_range" => Some("index textus with numerus or an integer range".to_owned()),
        "array_index_must_be_numerus" => Some("index lista values with numerus".to_owned()),
        "indexing_requires_array_or_map" => {
            Some("index only lista, tabula, tensor, textus, or ignotum values".to_owned())
        }
        "tensor_index_vector_integer_elements" => Some("use lista of supported integer index elements".to_owned()),
        "tensor_scalar_index_rank" => Some("use a scalar index only on rank-1 tensors".to_owned()),
        "tensor_bracket_index_integer_or_lista" => Some("use an integer scalar or lista of integer indices".to_owned()),
        "tensor_arithmetic_numeric_element_required" => Some("use numeric tensor or sparsa element types".to_owned()),
        "matrix_arithmetic_numeric_element_required" => Some("use numeric matrix element types".to_owned()),
        "tensor_receiver_structa_renamed" => Some("use seed.strue(data, shape)".to_owned()),
        "tensor_receiver_creata_type_args" => {
            Some("call tensor.creata<CompleteTensorType>(fill) for static construction".to_owned())
        }
        "tensor_crea_argument_order" => Some("pass fill before shape".to_owned()),
        "tensor_structa_forma_removed" => Some("use tensor.structa<CompleteTensorType>(data)".to_owned()),
        "tensor_creata_runtime_shape_removed" => Some("use tensor.creata<CompleteTensorType>(fill)".to_owned()),
        "tensor_creata_complete_type_arg_required" => Some("provide a complete tensor type argument".to_owned()),
        "tensor_formata_complete_type_arg_required" => Some("provide a complete tensor type argument".to_owned()),
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
            Some("⊙ requires identical operand shapes; use ↦ to reshape before the product".to_owned())
        }
        "gradient_expression_unsupported" => {
            Some("gradient selection (`∇`) is parse-only in Phase 2A; the eligibility/selector rules land with tensor-glyphs 2B".to_owned())
        }
        _ => None,
    }
}

fn nucleum_fragment_context_message(args: &[DiagnosticArg]) -> Option<String> {
    Some(
        match arg_value(args, "reason") {
            Some("entry_callee") => "@ nucleum fragment cannot call an @ nucleum entry",
            Some("graphics_stage") => "@ nucleum fragment cannot call a graphics @ fragment or @ vertex",
            Some("host_callable") => "@ nucleum fragment cannot call a host function or other non-device-safe callable",
            _ => "calls to @ nucleum fragment definitions are rejected until composition lands",
        }
        .to_owned(),
    )
}

fn nucleum_fragment_context_help(args: &[DiagnosticArg]) -> Option<String> {
    Some(
        match arg_value(args, "reason") {
            Some("entry_callee") => {
                "compose another @ nucleum fragment, or keep the dispatch on the calling @ nucleum entry"
            }
            Some("graphics_stage") => "keep graphics @ fragment / @ vertex stages out of nucleus fragment bodies",
            Some("host_callable") => "call another @ nucleum fragment or an approved device intrinsic from this body",
            _ => "wait for nucleum fragment composition, or call an @ nucleum entry instead",
        }
        .to_owned(),
    )
}

fn nucleum_fragment_effect_message(args: &[DiagnosticArg]) -> Option<String> {
    Some(
        match arg_value(args, "reason")? {
            "alternate_exit" => "@ nucleum fragment must be infallible; alternate-exit (⇥) is not allowed",
            "async_posture" => "@ nucleum fragment cannot use an async (fiet) posture",
            "generator_posture" => "@ nucleum fragment cannot use a generator (fiunt/fient) posture",
            "host_io" => "@ nucleum fragment cannot perform host I/O",
            "host_allocation" => "@ nucleum fragment cannot perform host allocation",
            "host_runtime" => "@ nucleum fragment cannot invoke a host runtime helper",
            "captured_mut_host" => "@ nucleum fragment cannot capture mutable host state",
            "dynamic_function" => "@ nucleum fragment cannot use a dynamic function value",
            "address_taking" => "@ nucleum fragment cannot take or dereference an address",
            "hidden_storage" => "@ nucleum fragment cannot use hidden persistent storage",
            "missing_body" => "@ nucleum fragment requires a body",
            "unapproved_effect" => "@ nucleum fragment body is outside the device-safe effect subset",
            _ => return None,
        }
        .to_owned(),
    )
}

fn nucleum_fragment_effect_help(args: &[DiagnosticArg]) -> Option<String> {
    Some(
        match arg_value(args, "reason")? {
            "alternate_exit" => "remove the ⇥ channel, iace, and fac/cape recovery from the fragment",
            "async_posture" => "write the fragment as a synchronous finite function",
            "generator_posture" => "write the fragment as a synchronous finite function",
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

fn semantic_unavailable_declaration_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "nondum_method_for_target" => {
            let method = arg_value(args, "method")?;
            let mut message = format!("method `{method}` is marked nondum for this target");
            if let Some(reason) = arg_value(args, "reason") {
                message.push_str(": ");
                message.push_str(reason);
            }
            Some(message)
        }
        _ => None,
    }
}

fn semantic_unavailable_declaration_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "nondum_method_for_target" => Some("use an available declaration or change the target".to_owned()),
        _ => None,
    }
}

fn semantic_duplicate_definition_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "duplicate_definition" => Some("duplicate definition".to_owned()),
        "inferred_import_name_collision" => {
            Some("inferred import name conflicts with an existing definition".to_owned())
        }
        "schema_duplicate_visible" => {
            let column = arg_value(args, "column")?;
            Some(format!("schema declares duplicate visible column name '{column}'"))
        }
        "schema_duplicate_source" => {
            let source = arg_value(args, "source")?;
            Some(format!("two schema columns resolve to the same source column '{source}'"))
        }
        _ => None,
    }
}

fn semantic_duplicate_definition_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "inferred_import_name_collision" => {
            Some("add an explicit import alias with `ut` so the imported binding has a unique name".to_owned())
        }
        _ => None,
    }
}

fn semantic_undefined_variable_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "name_not_variant" => Some("name does not refer to a variant".to_owned()),
        "scribe_callee" | "unknown_identifier" => Some("unknown identifier".to_owned()),
        "unknown_record_field" => Some("unknown record field".to_owned()),
        "unknown_variant" => Some("unknown variant".to_owned()),
        "unknown_variant_fields" => Some("unknown variant fields".to_owned()),
        "variant_ambiguous" => Some("variant name is ambiguous across in-scope unions".to_owned()),
        _ => None,
    }
}

fn semantic_undefined_variable_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "scribe_callee" => {
            let spelling = arg_value(args, "spelling")?;
            Some(format!(
                "write `{spelling} <expr>` without parentheses for the output statement"
            ))
        }
        "unknown_identifier" => {
            let suggestion = arg_value(args, "suggestion")?;
            Some(format!("declare the variable before use; did you mean {suggestion}?"))
        }
        "variant_ambiguous" => Some("qualify the variant with its union name (e.g. A.X)".to_owned()),
        _ => None,
    }
}

fn semantic_undefined_member_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "duplicate_variant_field" => Some("duplicate variant field".to_owned()),
        "missing_required_struct_field" => Some("missing required struct field".to_owned()),
        "missing_variant_field_name" => Some("missing variant field name".to_owned()),
        "namespace_missing_export" => {
            let member = arg_value(args, "member")?;
            Some(format!("namespace does not export `{member}`"))
        }
        "optional_receiver_requires_chain" => {
            Some("optional receiver requires optional chaining or non-null access".to_owned())
        }
        "struct_field_requires_value" => Some("struct field requires value".to_owned()),
        "unknown_field" => Some("unknown field".to_owned()),
        "unknown_method" => Some("unknown method".to_owned()),
        "unknown_struct_field" => Some("unknown struct field".to_owned()),
        "unknown_variant_field" => Some("unknown variant field".to_owned()),
        _ => None,
    }
}

fn semantic_undefined_member_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "optional_receiver_requires_chain" => Some("use ?. for optional chaining or !. for non-null access".to_owned()),
        "duplicate_variant_field" => Some("provide each named variant field at most once".to_owned()),
        "missing_required_struct_field" => Some("initialize every required struct field".to_owned()),
        "missing_variant_field_name" => {
            Some("name every variant argument when using named variant construction".to_owned())
        }
        "namespace_missing_export" => Some("check the imported file's `@ publica` exports".to_owned()),
        "struct_field_requires_value" => Some("provide a value for the struct field".to_owned()),
        "unknown_field" | "unknown_struct_field" => Some("check the struct field name".to_owned()),
        "unknown_method" => Some("check the receiver type and method name".to_owned()),
        "unknown_variant_field" => Some("check the variant field name".to_owned()),
        _ => None,
    }
}

fn semantic_undefined_type_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "figura_unknown_index" => Some("unknown index parameter in Figura".to_owned()),
        "float_width_on_numerus" => {
            Some("f16, f32, and f64 are fractus widths; use fractus<f16>, fractus<f32>, or fractus<f64>".to_owned())
        }
        "integer_width_on_fractus" => Some("integer width markers are numerus widths; use numerus<…>".to_owned()),
        "invalid_precision_marker" => Some("instans accepts only ms, us, or ns as a precision marker".to_owned()),
        "invalid_fractus_width_marker" => Some("fractus accepts only f16, f32, or f64".to_owned()),
        "invalid_numerus_width_marker" => Some("unknown width marker for numerus".to_owned()),
        "invalid_width_marker" => Some("unknown width marker for this family".to_owned()),
        "name_not_type" => Some("name does not refer to a type".to_owned()),
        "precision_marker_params" => Some("precision marker cannot accept parameters".to_owned()),
        "precision_marker_shape" => Some("precision marker must be a bare spelling such as ms or us".to_owned()),
        "qualified_name_not_type" => Some("qualified name does not refer to a type".to_owned()),
        "qualified_type_not_exported" => Some("qualified type is not exported by namespace".to_owned()),
        "qualified_type_prefix_not_namespace" => Some("qualified type prefix must name a namespace".to_owned()),
        "unknown_namespace" => Some("unknown namespace".to_owned()),
        "unknown_qualified_type" => Some("unknown qualified type".to_owned()),
        "unknown_struct" => Some("unknown struct".to_owned()),
        "unknown_type" => Some("unknown type".to_owned()),
        "schema_unknown_type" => {
            let column = arg_value(args, "column")?;
            Some(format!(
                "schema column '{column}' names a type that is not in the type namespace"
            ))
        }
        "width_marker_params" => Some("width marker cannot accept parameters".to_owned()),
        "width_marker_shape" => Some("width marker must be a bare spelling such as i32 or u8".to_owned()),
        _ => None,
    }
}

// HIR LOWERING

fn semantic_lowering_error_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "array_destructuring_initializer" => Some("array destructuring requires initializer".to_owned()),
        "array_destructuring_not_expanded" => {
            Some("array destructuring should be expanded before statement lowering".to_owned())
        }
        "atomic_compiler_owned_syntax" => Some("atomic requires compiler-owned syntax; use atomic<T>".to_owned()),
        "lista_arity" => Some("lista requires one type parameter".to_owned()),
        "tabula_arity" => Some("tabula requires two type parameters".to_owned()),
        "copia_arity" => Some("copia requires one type parameter".to_owned()),
        "promissum_arity" => Some("promissum requires one type parameter".to_owned()),
        "cursor_arity" => Some("cursor requires one type parameter".to_owned()),
        "discerne_casu_pattern_required" => Some("discerne casu requires a pattern".to_owned()),
        "discerne_pattern_count" => Some("discerne pattern count must match subject count".to_owned()),
        "discerne_subject_required" => Some("discerne requires at least one subject".to_owned()),
        "duplicate_module_method_binding" => Some("duplicate module method binding during lowering".to_owned()),
        "ego_outside_method" => Some("ego used outside method context".to_owned()),
        "elige_case_value_shape" => Some("elige case value must be a literal or enum variant".to_owned()),
        "empty_qualified_type_path" => Some("empty qualified type path".to_owned()),
        "empty_path_pattern" => Some("empty path pattern".to_owned()),
        "empty_union_type" => Some("empty union type".to_owned()),
        "ex_destructuring_not_expanded" => {
            Some("ex destructuring should be expanded before statement lowering".to_owned())
        }
        "finge_cast_not_type" => Some("finge cast must name a type".to_owned()),
        "figura_not_index_param" => Some("name does not refer to an index parameter".to_owned()),
        "figura_type_param_in_index" => Some("type parameter cannot appear in index position".to_owned()),
        "figura_unknown_index" => Some("unknown index variable in Figura".to_owned()),
        "float_width_on_numerus" => {
            Some("f16, f32, and f64 are fractus widths; use fractus<f16>, fractus<f32>, or fractus<f64>".to_owned())
        }
        "forma_builtin_missing" => Some("builtin forma genus is not registered".to_owned()),
        "forma_literal_lowering_path" => Some("forma template literals must lower through forma capture".to_owned()),
        "forma_literal_pattern" => Some("forma template literals are not valid in patterns".to_owned()),
        "forma_params_field_missing" => Some("builtin forma genus is missing params field".to_owned()),
        "forma_template_field_missing" => Some("builtin forma genus is missing template field".to_owned()),
        "fractus_width_params" => Some("fractus accepts at most one width marker".to_owned()),
        "integer_width_on_fractus" => Some("integer width markers are numerus widths; use numerus<…>".to_owned()),
        "instans_precision_params" => Some("instans accepts at most one precision marker".to_owned()),
        "intervallum_arity" => Some("intervallum requires one type parameter".to_owned()),
        "intervallum_bound" => Some("intervallum bound type must be numerus or instans".to_owned()),
        "atomic_arity" => Some("atomic requires exactly one element type; use atomic<T>".to_owned()),
        "atomic_element" => Some("atomic element type must be numerus<i32> or numerus<u32>".to_owned()),
        "atomic_incomplete" => Some("atomic requires one element type; use atomic<T>".to_owned()),
        "invalid_fractus_width_marker" => Some("fractus accepts only f16, f32, or f64".to_owned()),
        "invalid_lowered_expression" => Some("invalid expression produced during lowering".to_owned()),
        "invalid_numerus_width_marker" => Some("unknown width marker for numerus".to_owned()),
        "invalid_precision_marker" => Some("instans accepts only ms, us, or ns as a precision marker".to_owned()),
        "invalid_width_marker" => Some("unknown width marker for this family".to_owned()),
        "json_valor_pattern" => Some("JSON valor literals are not valid in patterns".to_owned()),
        "kernel_glob_call_shape" => Some("kernel glob calls must use `faber.<module>.<verb>`".to_owned()),
        "kernel_glob_named_alias" => Some("`faber:*` must be imported with a named alias".to_owned()),
        "kernel_unknown_module" => {
            let module = arg_value(args, "module")?;
            Some(format!("unknown faber kernel module '{module}'; see faber host manifest"))
        }
        "matrix_incomplete" => Some("matrix requires element type and shape; use matrix<T, [R, C]>".to_owned()),
        "matrix_missing_figura" => Some("matrix requires a two-dimensional shape; use matrix<T, [R, C]>".to_owned()),
        "matrix_shape_two_dim" => Some("matrix shape must be exactly two dimensions".to_owned()),
        "missing_cast_symbol_info" => Some("missing cast symbol information".to_owned()),
        "missing_resolved_type_alias" => Some("type alias is missing resolved type".to_owned()),
        "missing_type_alias_symbol" => Some("missing symbol for type alias".to_owned()),
        "missing_type_symbol_info" => Some("missing type symbol information".to_owned()),
        "mutable_pattern_binding" => Some("mutable pattern bindings are not lowered yet".to_owned()),
        "nested_array_destructuring" => Some("nested array destructuring is not lowered yet".to_owned()),
        "nested_object_destructuring" => Some("nested object destructuring is not lowered yet".to_owned()),
        "numerus_width_params" => Some("numerus accepts at most one width marker".to_owned()),
        "object_destructuring_initializer" => Some("object destructuring requires initializer".to_owned()),
        "object_destructuring_not_expanded" => {
            Some("object destructuring should be expanded before statement lowering".to_owned())
        }
        "octet_params" => Some("octet is fixed-width and cannot accept parameters".to_owned()),
        "precision_marker_shape" => Some("precision marker must be a bare spelling such as ms or us".to_owned()),
        "primitive_params" => Some("primitive type cannot accept parameters".to_owned()),
        "primitive_width_params" => Some("primitive accepts at most one width marker".to_owned()),
        "qualified_type_not_exported" => Some("qualified type is not exported by namespace".to_owned()),
        "qualified_type_prefix_not_namespace" => Some("qualified type prefix must name a namespace".to_owned()),
        "radix_typus_allowed_type_not_concrete" => Some("@ radix typus allowed type must be concrete".to_owned()),
        "radix_typus_non_type_param" => Some("@ radix typus can constrain only type parameters".to_owned()),
        "radix_typus_unknown_generic" => Some("@ radix typus names an unknown generic parameter".to_owned()),
        "retired_bare_object_literal" => {
            let forms = arg_value(args, "object_literal_forms")?;
            Some(format!("bare object literal is retired; use {forms}"))
        }
        "scrinium_arity" => Some("scrinium accepts at most one payload type parameter".to_owned()),
        "scrinium_missing_builtin" => Some("builtin scrinium type is not registered".to_owned()),
        "series_params" => Some("series cannot accept parameters".to_owned()),
        "schema_method" => Some("schema declarations cannot contain method-like members".to_owned()),
        "schema_inheritance" => Some("schema declarations cannot use 'sub'/'implet' inheritance syntax".to_owned()),
        "schema_nested_column" => {
            let column = arg_value(args, "column")?;
            Some(format!("schema column '{column}' is nested or carries a schema type"))
        }
        "sparsa_element_non_numeric" => Some("sparsa element type must be numeric in v1".to_owned()),
        "sparsa_incomplete" => Some("sparsa requires element type and Figura; use sparsa<T, Figura>".to_owned()),
        "sparsa_missing_figura" => Some("sparsa requires a Figura shape; use sparsa<T, []> for rank-0".to_owned()),
        "standalone_width_internal" => Some("internal error lowering standalone width type".to_owned()),
        "tensor_incomplete" => Some("tensor requires element type and Figura; use tensor<T, Figura>".to_owned()),
        "tensor_missing_figura" => Some("tensor requires a Figura shape; use tensor<T, []> for rank-0".to_owned()),
        "template_type_args" => Some("template application does not accept type arguments".to_owned()),
        "top_level_initializer" => Some("top-level variable declaration requires initializer".to_owned()),
        "top_level_identifier_binding" => Some("top-level variable requires an identifier binding".to_owned()),
        "type_name_not_type" => Some("type name does not refer to a type".to_owned()),
        "union_only_nihil" => Some("union type cannot consist only of 'nihil'".to_owned()),
        "undefined_name" => Some("undefined name in lowering".to_owned()),
        "unhandled_statement_kind" => Some("unhandled statement kind in lowering".to_owned()),
        "unknown_base_class" => Some("unknown base class in gens declaration".to_owned()),
        "unknown_finge_cast_type" => Some("unknown cast type in finge expression".to_owned()),
        "unknown_finge_variant" => Some("unknown variant in finge expression".to_owned()),
        "unknown_interface" => Some("unknown interface in gens declaration".to_owned()),
        "unknown_lowering_error" => Some("HIR lowering failed".to_owned()),
        "unknown_namespace" => Some("unknown namespace".to_owned()),
        "unknown_type_lowering_error" => Some("type lowering failed".to_owned()),
        "unknown_type_name" => Some("unknown type name".to_owned()),
        "vector_width_single_dim" => Some("vector width must be a single dimension".to_owned()),
        "width_marker_params" => Some("width marker cannot accept parameters".to_owned()),
        "width_marker_shape" => Some("width marker must be a bare spelling such as i32 or u8".to_owned()),
        _ => None,
    }
}

// ASSIGNMENT AND OWNERSHIP

fn semantic_definite_assignment_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "assignment_to_constant" => Some("assignment to constant".to_owned()),
        "immutable_second_assignment" => Some("second assignment to immutable binding".to_owned()),
        "imperativus_operator_method_requires_varia_receiver" => {
            Some("imperativus operator method requires varia receiver".to_owned())
        }
        "possibly_uninitialized" => Some("use of possibly uninitialized binding".to_owned()),
        _ => None,
    }
}

fn semantic_invalid_assignment_target_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "invalid_assignment_target" => Some("invalid assignment target".to_owned()),
        _ => None,
    }
}

fn semantic_borrow_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "assign_to_de_parameter" => Some("cannot assign to `de` parameter".to_owned()),
        "borrow_of_moved" => Some("borrow of moved value".to_owned()),
        "cannot_move_while_borrowed" => Some("cannot move out while borrowed".to_owned()),
        "de_to_in_or_ex_position" => Some("cannot pass `de` parameter to `in` or `ex` position".to_owned()),
        "mutable_borrow_conflict" => Some("mutable borrow conflicts with existing borrow".to_owned()),
        "shared_borrow_conflict" => Some("shared borrow conflicts with mutable borrow".to_owned()),
        "unused_ex_parameter" => Some("`ex` parameter is never consumed; consider `de`".to_owned()),
        "unused_in_parameter" => Some("`in` parameter is never mutated; consider `de`".to_owned()),
        "use_after_move" => Some("use after move".to_owned()),
        "use_while_mutably_borrowed" => Some("use while mutably borrowed".to_owned()),
        "write_while_borrowed" => Some("write while borrowed".to_owned()),
        _ => None,
    }
}

fn semantic_lint_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "explicit_ignotum_annotation" => Some("explicit ignotum annotation disables precise type-checking".to_owned()),
        "shadowed_variable" => Some("shadowed variable".to_owned()),
        "unnecessary_cast" => Some("unnecessary cast: type is already known".to_owned()),
        "unnecessary_varia" => Some("unnecessary varia: binding is never reassigned or mutated".to_owned()),
        "unreachable_code" => Some("unreachable code".to_owned()),
        "unused_binding" => Some("unused binding".to_owned()),
        "unused_function" => Some("unused function".to_owned()),
        "unused_import" => Some("unused import".to_owned()),
        "unused_variable" => Some("unused variable".to_owned()),
        _ => None,
    }
}

fn semantic_prefer_string_template_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "prefer_string_template" => {
            let mut message = "long string concatenation; prefer a string template".to_owned();
            if let Some(suggestion) = arg_value(args, "suggestion") {
                message.push_str(": `");
                message.push_str(suggestion);
                message.push('`');
            }
            Some(message)
        }
        _ => None,
    }
}

// TYPE MISMATCH

fn semantic_type_mismatch_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "argument_type_mismatch" => Some("argument type mismatch".to_owned()),
        "callee_not_callable" => Some("callee is not callable".to_owned()),
        "figura_not_index_param" => Some("name does not refer to an index parameter".to_owned()),
        "assignment_type_mismatch" => Some("assignment type mismatch".to_owned()),
        "assert_condition_type_mismatch" => Some("assert condition must be boolean".to_owned()),
        "constant_annotation_mismatch" => Some("constant value does not match annotation".to_owned()),
        "closure_parameter_type_mismatch" => Some("closure parameter type mismatch".to_owned()),
        "closure_return_type_mismatch" => Some("closure return type mismatch".to_owned()),
        "coalesce_fallback_type_mismatch" => Some("coalesce fallback type mismatch".to_owned()),
        "common_type_mismatch" => Some("incompatible types".to_owned()),
        "array_element_type_mismatch" => Some("array element type mismatch".to_owned()),
        "array_spread_element_type_mismatch" => Some("array spread element type mismatch".to_owned()),
        "await_bind_requires_async_context" => Some("await binding requires an async-capable body".to_owned()),
        "await_bind_requires_promissum" => Some("await binding requires a promissum value".to_owned()),
        "await_failable_requires_alternate_exit" => {
            Some("awaiting a failable promissum requires an alternate-exit channel".to_owned())
        }
        "await_failable_alternate_type_mismatch" => Some("awaited promissum alternate channel mismatch".to_owned()),
        "conversio_failure_channel_type_mismatch" => Some("propagating conversio failure channel mismatch".to_owned()),
        "conversio_recovery_type_mismatch" => Some("conversio recovery type mismatch".to_owned()),
        "conversio_assign_inference_hole_destination" => {
            Some("conversion-directed assignment '↤' requires a resolved destination type".to_owned())
        }
        "cursor_iteration_requires_ex" => Some("cursor iteration supports itera ex only".to_owned()),
        "cede_requires_generator" => Some("cede can only yield from a generator body".to_owned()),
        "cede_requires_promissum" => Some("await operation requires a promissum value".to_owned()),
        "reddet_requires_async_function" => Some("reddet requires an async finite function body".to_owned()),
        "tacebit_requires_async_context" => Some("tacebit requires an async-capable body".to_owned()),
        "expression_type_mismatch" => Some("expression type mismatch".to_owned()),
        "modular_word_literal_out_of_range" => {
            Some("modular word literal is outside the declared modulus width range".to_owned())
        }
        "numerus_literal_out_of_range" => Some(
            "integer literal is outside the signed numerus range; use numerus<u64> for values above i64::MAX"
                .to_owned(),
        ),
        "equality_exact_types_required" => Some(
            "equality requires identical types; use `↦` to convert explicitly before comparing (ascii ↦ numerus for the scalar value, or compare against an ascii literal like 'a')"
                .to_owned(),
        ),
        "field_initializer_type_mismatch" => Some("field initializer type mismatch".to_owned()),
        "failable_call_enclosing_error_type_mismatch" => {
            Some("failable call error type does not match enclosing ⇥".to_owned())
        }
        "failable_call_handler_error_type_mismatch" => Some("handled failable call error type mismatch".to_owned()),
        "field_default_type_mismatch" => Some("field default type mismatch".to_owned()),
        "iace_requires_alternate_exit" => {
            Some("iace requires an enclosing function with a '⇥' alternate-exit type".to_owned())
        }
        "iace_value_type_mismatch" => Some("alternate exit value type mismatch".to_owned()),
        "ignored_expression_result_must_resolve" => Some("ignored expression result must resolve".to_owned()),
        "initializer_annotation_mismatch" => Some("initializer does not match annotation".to_owned()),
        "incompatible_return_types" => Some("incompatible return types".to_owned()),
        "intervallum_bound_required" => Some(crate::semantic::INTERVALLUM_BOUND_MSG.to_owned()),
        "intervallum_clamp_operand_type_mismatch" => Some("intervallum clamp operand type mismatch".to_owned()),
        "intervallum_value_step_unsupported" => Some("range step is only supported by iteration or slicing".to_owned()),
        "intra_requires_intervallum_range" => Some("intra requires an intervallum range".to_owned()),
        "incompatible_tensor_index" => Some(crate::semantic::INCOMPATIBLE_INDEX_MSG.to_owned()),
        "invalid_cast" => Some("invalid cast".to_owned()),
        "itera_ab_source_requires_range" => Some("itera ab source must be a range expression".to_owned()),
        "json_map_key_must_be_textus" => Some("JSON map key must be textus".to_owned()),
        "json_annotation_unsupported_site" => {
            Some("@ json is only supported on genus declarations and fields inside @ json genera".to_owned())
        }
        "json_field_annotation_option" => Some("field @ json supports only nomen = \"...\"".to_owned()),
        "json_field_annotation_outside_json_genus" => {
            Some("field-level @ json requires an enclosing @ json genus".to_owned())
        }
        "json_field_nomen_empty" => Some("JSON field output name cannot be empty".to_owned()),
        "json_genus_duplicate_field_name" => Some("duplicate JSON output field name".to_owned()),
        "json_genus_field_type_unsupported" => Some("@ json genus field type is not JSON-safe".to_owned()),
        "json_genus_map_key_not_textus" => Some("@ json genus map fields require textus keys".to_owned()),
        "json_genus_nested_requires_json" => {
            Some("nested genus fields in @ json genera must also be @ json".to_owned())
        }
        "json_nested_value_requires_valor" => Some("nested JSON value requires tabula value type valor".to_owned()),
        "json_value_type_mismatch" => Some("JSON value does not match tabula value type".to_owned()),
        "live_tuus_cursor_call_arg" => {
            Some("live tuus cursor values cannot be passed as function arguments yet".to_owned())
        }
        "live_tuus_cursor_return" => Some("live tuus cursor values cannot be returned from functions yet".to_owned()),
        "live_tuus_cursor_struct" => Some("live tuus cursor values cannot be stored in structures yet".to_owned()),
        "live_tuus_cursor_reassign" => Some("live tuus cursor locals cannot be reassigned yet".to_owned()),
        "live_tuus_cursor_copy" => {
            Some("live tuus cursor values cannot be copied or moved between locals yet".to_owned())
        }
        "live_tuus_cursor_conversio" => {
            Some("live tuus cursor values cannot be converted through aggregate conversio yet".to_owned())
        }
        "live_tuus_cursor_direct_initializer" => {
            Some("tuus.cursor() is only supported as a direct local initializer".to_owned())
        }
        "live_tuus_cursor_value" => Some("live tuus cursor values cannot be used as expressions yet".to_owned()),
        "map_index_type_mismatch" => Some("map index type mismatch".to_owned()),
        "map_key_type_mismatch" => Some("map key type mismatch".to_owned()),
        "map_spread_key_type_mismatch" => Some("map spread key type mismatch".to_owned()),
        "map_spread_value_type_mismatch" => Some("map spread value type mismatch".to_owned()),
        "map_value_type_mismatch" => Some("map value type mismatch".to_owned()),
        "membership_operand_type_mismatch" => Some("membership operand type mismatch".to_owned()),
        "matmul_argument_rank_must_be_two" => Some("matmul requires rank-2 tensor argument".to_owned()),
        "matmul_argument_shape_must_be_tuple" => {
            Some("matmul requires rank-2 tensor argument with tuple shape".to_owned())
        }
        "matmul_receiver_rank_must_be_two" => Some("matmul requires rank-2 tensor receiver".to_owned()),
        "matmul_receiver_shape_must_be_tuple" => {
            Some("matmul requires rank-2 tensor receiver with tuple shape".to_owned())
        }
        "numeric_arithmetic_operands_identical" => {
            Some("numeric arithmetic operands must have identical types; use ↦ to convert explicitly".to_owned())
        }
        "numeric_operand_required" => Some("numeric operand required".to_owned()),
        "numeric_operands_required" => Some("numeric operands required".to_owned()),
        "boolean_operand_required" => Some("boolean operand required".to_owned()),
        "boolean_operands_required" => Some("boolean operands required".to_owned()),
        "integer_operand_required" => Some("integer operand required".to_owned()),
        "integer_operands_required" => Some("integer operands required".to_owned()),
        "instans_ordering_operands_required" => Some("incompatible instans operands".to_owned()),
        "pattern_type_mismatch" => Some("pattern type mismatch".to_owned()),
        "parameter_default_type_mismatch" => Some("parameter default type mismatch".to_owned()),
        "range_operand_type_mismatch" => Some("range operand type mismatch".to_owned()),
        "range_step_must_be_numeric" => Some("range step must be numeric".to_owned()),
        "return_type_mismatch" => Some("return type does not match function signature".to_owned()),
        "regex_conversion_operand_type_mismatch" => Some("regex conversion requires textus or ascii".to_owned()),
        "string_operands_required" => Some("string operands required".to_owned()),
        "atomic_arity" => Some("atomic requires exactly one element type; use atomic<T>".to_owned()),
        "atomic_element" => Some("atomic element type must be numerus<i32> or numerus<u32>".to_owned()),
        "atomic_incomplete" => Some("atomic requires one element type; use atomic<T>".to_owned()),
        "matrix_incomplete" => Some("matrix requires element type and shape; use matrix<T, [R, C]>".to_owned()),
        "matrix_missing_figura" => Some("matrix requires a two-dimensional shape; use matrix<T, [R, C]>".to_owned()),
        "matrix_shape_two_dim" => Some("matrix shape must be exactly two dimensions".to_owned()),
        "sparsa_incomplete" => Some("sparsa requires element type and Figura; use sparsa<T, Figura>".to_owned()),
        "sparsa_missing_figura" => Some("sparsa requires a Figura shape; use sparsa<T, []> for rank-0".to_owned()),
        "scalar_text_operands_required" => Some("scalar text operands required".to_owned()),
        "stream_ad_opener_type_mismatch" => Some("stream ad opener must lower to valor".to_owned()),
        "tensor_formata_element_type_mismatch" => {
            Some(crate::semantic::tensor_type_directed::TENSOR_FORMA_TYPE_ARG_ELEMENT_MSG.to_owned())
        }
        "tensor_incomplete" => Some("tensor requires element type and Figura; use tensor<T, Figura>".to_owned()),
        "tensor_missing_figura" => Some("tensor requires a Figura shape; use tensor<T, []> for rank-0".to_owned()),
        "variant_pattern_scrutinee_mismatch" => Some("variant does not match scrutinee type".to_owned()),
        "vector_element_type_mismatch" => Some("vector element type mismatch".to_owned()),
        "vector_incomplete" => Some("vector requires element type and width; use vector<T, N>".to_owned()),
        "vector_missing_width" => Some("vector requires a width; use vector<T, 4> or vf32[4]".to_owned()),
        _ => None,
    }
}

// AIR PURITY

const AIR_PURITY_HELP: &str = "keep the v1 AIR subset pure: immutable locals, pure expressions, AIR-lane calls, and tensor arithmetic intrinsics only";

/// Return the v1-AIR-subset rejection reason for a recognized air-purity issue.
///
/// Shared by the message and help renderers so the recognized-issue set has
/// one source: a `Some(_)` reason enables both message formatting and the
/// shared baseline help, while `None` leaves both at their fallthrough.
fn air_purity_reason(args: &[DiagnosticArg]) -> Option<String> {
    Some(match arg_value(args, "issue")? {
        "async_function" => "async functions are outside the v1 AIR subset".to_owned(),
        "assignment_mutation" => "assignment is mutation outside the v1 AIR subset".to_owned(),
        "breakable_block" => "breakable blocks are outside the v1 AIR subset".to_owned(),
        "cede" => "`cede` is outside the v1 AIR subset".to_owned(),
        "cli_mounted_function" => "CLI-mounted functions are outside the v1 AIR subset".to_owned(),
        "closure" => "closures are outside the v1 AIR subset".to_owned(),
        "conditional_recovery" => "conditional recovery is outside the v1 AIR subset".to_owned(),
        "defaulted_parameter" => "defaulted parameters are outside the v1 AIR subset".to_owned(),
        "deferred_local_init" => "deferred local initialization is outside the v1 AIR subset".to_owned(),
        "diagnostic_output" => "diagnostic output is a host effect outside the v1 AIR subset".to_owned(),
        "dum_loop" => "`dum` loops are outside the v1 AIR subset".to_owned(),
        "expression_ad" => "expression-form ad is outside the v1 AIR subset".to_owned(),
        "expression_statement" => "expression statements are outside the v1 AIR subset".to_owned(),
        "generator_function" => "generator functions are outside the v1 AIR subset".to_owned(),
        "iace" => "`iace` is outside the v1 AIR subset".to_owned(),
        "inc_dec_mutation" => "increment/decrement mutation is outside the v1 AIR subset".to_owned(),
        "input_read" => "input reads are host effects outside the v1 AIR subset".to_owned(),
        "itera_loop" => "`itera` loops are outside the v1 AIR subset".to_owned(),
        "kernel_device_function" => "kernel/device functions are outside the v1 AIR subset".to_owned(),
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
        "non_air_call" => "calls from AIR-lane functions must target another AIR-lane function".to_owned(),
        "optional_null_chain" => "optional/null assertion chains are outside the v1 AIR subset".to_owned(),
        "recoverable_handler" => "recoverable handlers are outside the v1 AIR subset".to_owned(),
        "reference_place_op" => "reference/place operations are outside the v1 AIR subset".to_owned(),
        "runtime_assertion" => "runtime assertions are outside the v1 AIR subset".to_owned(),
        "runtime_conversion" => "runtime `↦` conversion is outside the v1 AIR subset".to_owned(),
        "runtime_provided_binding" => "runtime-provided bindings are outside the v1 AIR subset".to_owned(),
        "runtime_variant_check" => "runtime `est` variant check is outside the v1 AIR subset".to_owned(),
        "spread_call_arg" => "spread call arguments are outside the v1 AIR subset".to_owned(),
        "tacet" => "`tacet` is outside the v1 AIR subset".to_owned(),
        _ => return None,
    })
}

fn semantic_air_purity_message(args: &[DiagnosticArg]) -> Option<String> {
    let lane = arg_value(args, "lane")?;
    let reason = air_purity_reason(args)?;
    Some(format!("@ radix lane \"{lane}\" rejects this function body: {reason}"))
}

fn semantic_air_purity_help(args: &[DiagnosticArg]) -> Option<String> {
    air_purity_reason(args).map(|_| AIR_PURITY_HELP.to_owned())
}

// IMPORT PATHS

fn semantic_import_not_found_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "kernel_glob_reexport" => {
            let path = arg_value(args, "path")?;
            Some(format!("`{path}` glob imports cannot be re-exported"))
        }
        "kernel_glob_named_alias" => {
            let path = arg_value(args, "path")?;
            Some(format!("`{path}` must be imported as a named glob alias"))
        }
        "kernel_invalid_path" => Some(arg_value(args, "reason")?.to_owned()),
        "kernel_unknown_module" => {
            let module = arg_value(args, "module")?;
            Some(format!("unknown faber kernel module '{module}'; see faber host manifest"))
        }
        "kernel_norma_json_in_script" => {
            let script_path = arg_value(args, "script_path")?;
            let library_path = arg_value(args, "library_path")?;
            Some(format!(
                "script mode uses `{script_path}`; `{library_path}` is package/library only"
            ))
        }
        "import_parent_traversal" => {
            let path = arg_value(args, "path")?;
            Some(format!(
                "import path `{path}` contains a parent-directory segment (`..`); \
                 use `§template/…` or relative paths within the package"
            ))
        }
        "import_absolute_path" => {
            let path = arg_value(args, "path")?;
            Some(format!(
                "import path `{path}` is absolute; use a relative path, \
                 provider import, or `§template/…`"
            ))
        }
        "import_unknown_scheme" => {
            let path = arg_value(args, "path")?;
            Some(format!(
                "import path `{path}` has an unknown scheme; \
                 use `provider:module`, `./rel`, or `§template/…`"
            ))
        }
        "import_unknown_template" => {
            let name = arg_value(args, "name")?;
            Some(format!(
                "`{name}` is not a known path template; \
                 add it to `[paths.templates]` in faber.toml"
            ))
        }
        "import_bare_glyph" => Some(
            "bare `§/` is not allowed in imports; \
                   use `§name/…` with a template name"
                .to_owned(),
        ),
        "import_multiple_holes" => {
            let path = arg_value(args, "path")?;
            Some(format!(
                "import path `{path}` has multiple `§` holes; \
                 one template per import"
            ))
        }
        "import_positional_hole" => Some(
            "`§0`/`§1` positional holes are not allowed in imports; \
                 use `§name/…` with a named template"
                .to_owned(),
        ),
        "import_interna_cross_package" => {
            let name = arg_value(args, "name")?;
            Some(format!(
                "`{name}` is `@ interna` and cannot be imported across a package boundary"
            ))
        }
        "import_module_private" => {
            let name = arg_value(args, "name")?;
            Some(format!("`{name}` is module-private and cannot be imported"))
        }
        _ => None,
    }
}

fn semantic_import_not_found_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "import_interna_cross_package" => {
            Some("import `@ interna` names only from a file in the same package".to_owned())
        }
        "import_module_private" => {
            Some("mark the declaration `@ publica` to export it, or `@ interna` for same-package use".to_owned())
        }
        _ => None,
    }
}

// ANNOTATION CONTRACTS

fn semantic_invalid_annotation_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "protecta_reserved" => {
            let annotation = arg_value(args, "annotation")?;
            Some(format!("`{annotation}` is reserved and has no active visibility meaning"))
        }
        "conflicting_lane" => {
            let existing = arg_value(args, "existing")?;
            let parsed = arg_value(args, "parsed")?;
            Some(format!("conflicting @ radix lane annotations: '{existing}' and '{parsed}'"))
        }
        "missing_directive" => Some("@ radix requires a directive such as `lane`".to_owned()),
        "missing_lane_value" => Some("@ radix lane requires a string value".to_owned()),
        "non_string_lane_value" => Some("@ radix lane value must be a string literal".to_owned()),
        "empty_radix_typus_domain" => Some("@ radix typus domain requires at least one allowed type".to_owned()),
        "missing_type_param" => Some("@ radix typus requires a generic type parameter".to_owned()),
        "non_identifier_type_param" => Some("@ radix typus parameter must be an identifier".to_owned()),
        "non_type_allowed" => Some("@ radix typus allowed value must be a type".to_owned()),
        "unexpected_token" => Some("unexpected token after @ radix lane value".to_owned()),
        "unknown_directive" => Some("unknown @ radix directive; expected `lane`".to_owned()),
        "unknown_lane" => {
            let lane = arg_value(args, "lane")?;
            Some(format!("unknown @ radix lane '{lane}'; expected 'air', 'mir', or 'hir-direct'"))
        }
        "unsupported_placement" => {
            let placement = arg_value(args, "placement")?;
            Some(format!(
                "@ radix lane is only supported on top-level functio declarations, not on {placement}"
            ))
        }
        "annotatio_misplaced" => {
            let placement = arg_value(args, "placement")?;
            Some(format!(
                "`@ annotatio` is only valid on a top-level genus declaration, not on {placement}"
            ))
        }
        "annotatio_unknown_target" => {
            let target = arg_value(args, "target")?;
            Some(format!("unknown annotation contract target `{target}`; expected `functio`"))
        }
        "annotatio_invalid_field_type" => {
            let field = arg_value(args, "field")?;
            Some(format!(
                "annotation contract field `{field}` must use a scalar type (`textus`, `numerus`, `fractus`, or `bivalens`)"
            ))
        }
        "annotatio_unknown_contract" => {
            let name = arg_value(args, "name")?;
            Some(format!("unknown annotation contract `{name}`"))
        }
        "annotatio_not_a_contract" => {
            let name = arg_value(args, "name")?;
            Some(format!(
                "`{name}` is not an annotation contract; mark the genus with `@ annotatio`"
            ))
        }
        "annotatio_unknown_field" => {
            let field = arg_value(args, "field")?;
            Some(format!("unknown annotation field `{field}`"))
        }
        "annotatio_duplicate_field" => {
            let field = arg_value(args, "field")?;
            Some(format!("duplicate annotation field `{field}`"))
        }
        "annotatio_missing_field" => {
            let field = arg_value(args, "field")?;
            Some(format!("missing required annotation field `{field}`"))
        }
        "annotatio_invalid_payload" => {
            let field = arg_value(args, "field")?;
            let expected = arg_value(args, "expected")?;
            Some(format!(
                "annotation field `{field}` requires a compile-time `{expected}` constant"
            ))
        }
        "annotatio_invalid_attachment" => {
            let expected = arg_value(args, "expected")?;
            Some(format!("annotation contract may only be attached to `{expected}` declarations"))
        }
        "annotatio_method_unsupported" => {
            let method = arg_value(args, "method")?;
            Some(format!(
                "annotation contracts cannot declare methods such as `{method}`; use scalar fields only"
            ))
        }
        "annotatio_invalid_marker" => Some("invalid `@ annotatio` marker form".to_owned()),
        "annotatio_duplicate_marker" => Some("duplicate `@ annotatio` marker on the same genus declaration".to_owned()),
        _ => None,
    }
}

fn semantic_invalid_annotation_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
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
        | "unexpected_token"
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
        _ => None,
    }
}

fn semantic_conflicting_visibility_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "conflicting_visibility" => {
            let first = arg_value(args, "first")?;
            let second = arg_value(args, "second")?;
            Some(format!("conflicting visibility annotations: '{first}' and '{second}'"))
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

fn semantic_exhaustiveness_message(kind: SemanticErrorKind, args: &[DiagnosticArg]) -> Option<String> {
    match (kind, arg_value(args, "issue")?) {
        (SemanticErrorKind::DuplicatePattern, "duplicate_variant") => Some("duplicate pattern".to_owned()),
        (SemanticErrorKind::NonExhaustiveMatch, "missing_variants") => Some("non-exhaustive match".to_owned()),
        (SemanticErrorKind::OmniaForbidsCatchall, "catchall_arm") => {
            let keyword = quote_arg(args, "keyword")?;
            Some(format!("{keyword} forbids catchall arms; list every variant explicitly"))
        }
        (SemanticErrorKind::OmniaRequiresEnum, "enum_scrutinee") => {
            let keyword = quote_arg(args, "keyword")?;
            Some(format!("{keyword} requires an enum scrutinee"))
        }
        (SemanticErrorKind::OmniaRequiresEnum, "single_enum_scrutinee") => {
            let keyword = quote_arg(args, "keyword")?;
            Some(format!("{keyword} requires a single enum scrutinee"))
        }
        (SemanticErrorKind::UnreachablePattern, "after_catchall") => Some("unreachable pattern".to_owned()),
        _ => None,
    }
}

fn semantic_wildcard_absorbs_null_message(args: &[DiagnosticArg]) -> Option<String> {
    let casu = quote_arg(args, "casu")?;
    let nil = arg_value(args, "nil")?;
    Some(format!(
        "catchall arm absorbs null case of nullable scrutinee; use an explicit {casu} {nil} arm"
    ))
}

// WARNING SPECS

fn warning_spec(kind: WarningKind) -> DiagnosticSpec {
    match kind {
        WarningKind::UnusedVariable => {
            DiagnosticSpec { code: "WARN001", help: Some("remove the variable or prefix it with '_'") }
        }
        WarningKind::UnusedImport => DiagnosticSpec { code: "WARN002", help: Some("remove the unused import") },
        WarningKind::UnusedFunction => {
            DiagnosticSpec { code: "WARN003", help: Some("remove the function or mark it as used") }
        }
        WarningKind::UnreachableCode => DiagnosticSpec { code: "WARN004", help: Some("remove unreachable code") },
        WarningKind::UnnecessaryCast => DiagnosticSpec { code: "WARN005", help: Some("remove the redundant cast") },
        WarningKind::TargetNoop => {
            DiagnosticSpec { code: "WARN007", help: Some("remove the construct or gate it by target") }
        }
        WarningKind::UnusedMutRefParam => {
            DiagnosticSpec { code: "WARN008", help: Some("change 'in' to 'de' if mutation is unnecessary") }
        }
        WarningKind::UnusedMoveParam => {
            DiagnosticSpec { code: "WARN009", help: Some("change 'ex' to 'de' if ownership transfer is unnecessary") }
        }
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
            help: Some("use `fixum` when the binding is never reassigned or mutated"),
        },
        WarningKind::FileInterfaceExportSkipped => DiagnosticSpec {
            code: "WARN014",
            help: Some(
                "use a supported export from the module, or wait for fuller file-interface coverage of this signature",
            ),
        },
        WarningKind::UnrecognizedCurataAllocator => DiagnosticSpec {
            code: "WARN015",
            help: Some("use a recognized allocator name (\"arena\", \"page\", \"alloc\", or \"allocator\")"),
        },
        WarningKind::MonomorphicHoleSolvedToUnion => DiagnosticSpec {
            code: "WARN016",
            help: Some("use an explicit union type or a `∪` hole if a sum was intended"),
        },
        WarningKind::UnionHoleSolvedMonomorphically => {
            DiagnosticSpec { code: "WARN017", help: Some("use `_` if only one type is intended") }
        }
        WarningKind::PreferStringTemplate => DiagnosticSpec {
            code: "WARN018",
            help: Some("use a string template (`\"…§…\"(args)`) for long concatenation chains"),
        },
    }
}

fn semantic_file_interface_export_skipped_message(args: &[DiagnosticArg]) -> Option<String> {
    let namespace = arg_value(args, "namespace").unwrap_or("?");
    let export = arg_value(args, "export").unwrap_or("?");
    let reason = arg_value(args, "reason").unwrap_or("unsupported");
    Some(format!(
        "file interface skipped export `{namespace}.{export}` ({reason}); other exports still available"
    ))
}

fn semantic_unrecognized_curata_allocator_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "unrecognized_curata_allocator" => {
            let name = arg_value(args, "name")?;
            Some(format!("curata allocator name '{name}' is not a recognized allocator contract"))
        }
        _ => None,
    }
}

fn semantic_monomorphic_hole_solved_to_union_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "monomorphic_hole_solved_to_union" => {
            let union = arg_value(args, "union")?;
            Some(format!("monomorphic hole `_` solved to multi-member union `{union}`"))
        }
        _ => None,
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

fn semantic_union_hole_solved_monomorphically_message(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "union_hole_solved_monomorphically" => {
            let ty = arg_value(args, "type")?;
            Some(format!("union hole `∪` solved to single type `{ty}`"))
        }
        _ => None,
    }
}

fn semantic_union_hole_solved_monomorphically_help(args: &[DiagnosticArg]) -> Option<String> {
    match arg_value(args, "issue")? {
        "union_hole_solved_monomorphically" => Some("use `_` if only one type is intended".to_owned()),
        _ => None,
    }
}
