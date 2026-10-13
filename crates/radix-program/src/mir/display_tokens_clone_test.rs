//! Display tokens follow the library unit through the link and reach
//! cross-file generic clones (need, plumbing only).
//!
//! Two distinct token sets (Latin, English) are used so a swap would show.

use super::*;
use radix_mir::device::MirDeviceFunctionMetadataMap;
use radix_mir::{
    MirDiagnosticKind, MirDisplayTokens, MirIntrinsic, MirMonomorphization, MirMonomorphizationKey,
    MirRuntimeCall, MirStatement, MirStatementKind,
};
use radix_mir_lower::lower::MirTensorEdgePolicyMetadata;
use radix_module::lexer::Span;
use radix_module::runtime_contract::display_tokens::DisplayTokens;
use radix_types::{NumericWidth, Primitive, TypeTable};

fn english() -> DisplayTokens {
    DisplayTokens {
        true_: "true".to_owned(),
        false_: "false".to_owned(),
        none: "none".to_owned(),
        tuple: "tuple".to_owned(),
    }
}

fn function(id: u32, source: u32, unit: MirType) -> MirFunction {
    MirFunction {
        id: MirFunctionId(id),
        source: Some(DefId(source)),
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements: Vec::new(),
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: Span::default(),
            },
            span: Span::default(),
        }],
        return_ty: unit,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

/// A template whose body holds one print site.
fn printing_function(id: u32, source: u32, unit: MirType) -> MirFunction {
    let mut template = function(id, source, unit);
    template.blocks[0].statements.push(MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination: None,
            call: MirRuntimeCall {
                intrinsic: MirIntrinsic::Diagnostic(MirDiagnosticKind::Nota),
                args: Vec::new(),
                return_ty: unit,
            },
        },
        span: Span::default(),
    });
    template
}

fn unit_ty() -> MirType {
    let mut types = TypeTable::new();
    MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32))
}

fn clone_of(source: u32, function: u32) -> MirMonomorphization {
    MirMonomorphization {
        key: MirMonomorphizationKey {
            source: DefId(source),
            type_args: Vec::new(),
            shape_args: Vec::new(),
        },
        function: MirFunctionId(function),
    }
}

/// A library keeps its table through pruning (ids compact) and the merge shift.
#[test]
fn library_unit_display_tokens_survive_the_link() {
    let ty = unit_ty();
    let mut table = MirDisplayTokens::default();
    table.assign(MirFunctionId(1), &english());
    table.assign(MirFunctionId(3), &english());
    let mut libraries = vec![LibraryMirParts {
        program: MirProgram {
            functions: vec![function(1, 101, ty), function(3, 103, ty)],
        },
        environments: Vec::new(),
        templates: Vec::new(),
        generic_params: Vec::new(),
        tensor_edge_policy_metadata: MirTensorEdgePolicyMetadata::default(),
        device_function_metadata: MirDeviceFunctionMetadataMap::default(),
        display_tokens: table,
    }];
    // Only function 3 (source 103) is reachable; it becomes function 0.
    let mut reachable: HashSet<DefId> = [DefId(103)].into_iter().collect();
    prune_unreachable_library_functions(&mut libraries, &mut reachable);
    let library = libraries.remove(0);
    assert_eq!(library.program.functions.len(), 1);
    assert_eq!(library.display_tokens.by_function.len(), 1);
    assert_eq!(
        library.display_tokens.for_function(MirFunctionId(0)),
        Some(&english())
    );

    // The merged (entry) side prints Latin; the library lands at offset 4.
    let mut merged = MirDisplayTokens::default();
    merged.assign(MirFunctionId(0), &DisplayTokens::latin());
    merged.absorb(&library.display_tokens, |id| MirFunctionId(id.0 + 4));
    assert_eq!(
        merged.for_function(MirFunctionId(4)),
        Some(&english()),
        "library function keeps English"
    );
    assert_eq!(
        merged.for_function(MirFunctionId(0)),
        Some(&DisplayTokens::latin()),
        "entry function keeps Latin"
    );
}

/// A clone made in the merged program prints through the file that defines
/// its template; a template with no recorded tokens leaves the clone bare.
#[test]
fn cross_file_generic_clone_takes_the_defining_files_tokens() {
    let ty = unit_ty();
    let mut template_tokens = HashMap::new();
    record_template_display_tokens(
        &mut template_tokens,
        &[printing_function(7, 50, ty)],
        &english(),
    );
    // A template that prints nothing records nothing.
    record_template_display_tokens(&mut template_tokens, &[function(8, 60, ty)], &english());

    let mut merged = MirDisplayTokens::default();
    merged.assign(MirFunctionId(0), &DisplayTokens::latin());
    assign_clone_display_tokens(
        &mut merged,
        &[clone_of(50, 9), clone_of(60, 10)],
        &template_tokens,
    );
    assert_eq!(merged.for_function(MirFunctionId(9)), Some(&english()));
    assert_eq!(merged.for_function(MirFunctionId(10)), None);
    assert_eq!(merged.tokens.len(), 2, "Latin and English, deduplicated");
}

/// A clone lowered in its own file already holds an id; the link keeps it.
#[test]
fn local_generic_clone_keeps_its_own_tokens() {
    let mut template_tokens = HashMap::new();
    template_tokens.insert(DefId(50), english());
    let mut merged = MirDisplayTokens::default();
    merged.assign(MirFunctionId(9), &DisplayTokens::latin());
    assign_clone_display_tokens(&mut merged, &[clone_of(50, 9)], &template_tokens);
    assert_eq!(
        merged.for_function(MirFunctionId(9)),
        Some(&DisplayTokens::latin())
    );
}
