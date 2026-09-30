//! inf track U4: an FMIR image carries the `inf` type and a `Magnus` constant
//! through both codecs, and the appended serde indices of the new MIR variants
//! are pinned (inf-delivery rulings 34, 46; the append-only rule of the spec's
//! §2 runner row).

use crate::schema::{
    FMIR_TARGET_NAME, FmirBinaryImageFile, FmirTextCliRootSection, FmirTextCliSection,
    FmirTextRuntimeSection, FmirTextSourceIdentity, FmirTextSourcesSection,
    FmirTextToolchainSection, FmirTextTypesSection, PACKAGE_MIR_ARTIFACT_VERSION,
};
use crate::{decode_binary_image, encode_binary_image};
use radix_lexer::Span;
use radix_mir::{
    MirBlock, MirBlockId, MirConstant, MirFunction, MirFunctionId, MirOperand, MirProgram,
    MirScalarLayout, MirTerminator, MirTerminatorKind, MirType,
};
use radix_types::{NumericWidth, Primitive, Type, TypeTable};

const TWO_64: &str = "18446744073709551616";

fn image_returning(constants: &[MirConstant]) -> (FmirBinaryImageFile, radix_types::TypeId) {
    let mut types = TypeTable::new();
    let inf = types.sized_numeric(Primitive::Numerus, NumericWidth::Inf);
    let mut program = MirProgram::new();
    for (index, constant) in constants.iter().enumerate() {
        program.functions.push(MirFunction {
            id: MirFunctionId(u32::try_from(index).expect("small")),
            source: None,
            name: None,
            params: vec![],
            locals: vec![],
            temps: vec![],
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: vec![],
                terminator: MirTerminator {
                    kind: MirTerminatorKind::Return(Some(MirOperand::Constant(constant.clone()))),
                    span: Span::default(),
                },
                span: Span::default(),
            }],
            return_ty: MirType::semantic(inf),
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span: Span::default(),
        });
    }
    let image = FmirBinaryImageFile {
        version: PACKAGE_MIR_ARTIFACT_VERSION,
        target: FMIR_TARGET_NAME.to_owned(),
        package_root: ".".to_owned(),
        entry: "main.fab".to_owned(),
        entry_function: "run_entry".to_owned(),
        toolchain: FmirTextToolchainSection {
            faber_cli_version: "0.84.0".to_owned(),
        },
        runtime: FmirTextRuntimeSection {
            requirement: vec!["host:argv".to_owned()],
        },
        sources: FmirTextSourcesSection {
            source: vec![FmirTextSourceIdentity {
                file: "main.fab".to_owned(),
                hash: "fnv64:0000000000000000".to_owned(),
            }],
        },
        cli: Some(FmirTextCliSection {
            root: FmirTextCliRootSection {
                record: "run_entry".to_owned(),
                operand: Vec::new(),
            },
        }),
        exit_code: Some(0),
        types: FmirTextTypesSection {
            table: types.snapshot(),
        },
        interner: vec!["main".to_owned()],
        program,
        device: None,
    };
    (image, inf)
}

#[test]
fn an_inf_constant_round_trips_through_the_binary_image() {
    let constants = [
        MirConstant::Magnus(TWO_64.to_owned()),
        MirConstant::Magnus(format!("-{TWO_64}")),
        MirConstant::Magnus("1".repeat(400)),
        MirConstant::UInt(u64::MAX),
        MirConstant::Int(i64::MIN),
    ];
    let (image, inf) = image_returning(&constants);
    let bytes = encode_binary_image(&image).expect("encode");
    let decoded = decode_binary_image(&bytes, "0.84.0").expect("decode");
    assert_eq!(
        decoded.program, image.program,
        "every constant comes back byte for byte"
    );
    let restored: Vec<&MirConstant> = decoded
        .program
        .functions
        .iter()
        .map(|function| match &function.blocks[0].terminator.kind {
            MirTerminatorKind::Return(Some(MirOperand::Constant(constant))) => constant,
            other => panic!("not a constant return: {other:?}"),
        })
        .collect();
    assert_eq!(restored[0], &MirConstant::Magnus(TWO_64.to_owned()));
    assert_eq!(restored[2], &MirConstant::Magnus("1".repeat(400)));
    // The type table snapshot carries the `inf` width too.
    let table = TypeTable::from_snapshot(decoded.types.table).expect("snapshot");
    assert_eq!(
        *table.get(inf),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::Inf)
    );
}

#[test]
fn an_inf_constant_round_trips_through_the_text_program_json() {
    // The text image carries the program as JSON.
    let (image, _) = image_returning(&[MirConstant::Magnus(format!("-{TWO_64}"))]);
    let json = serde_json::to_string(&image.program).expect("json");
    let back: MirProgram = serde_json::from_str(&json).expect("decode");
    assert_eq!(back, image.program);
}

#[test]
fn the_new_variants_are_appended_so_every_earlier_index_is_unchanged() {
    // FMIR binary images are postcard, where a variant is its position.
    let index = |bytes: Vec<u8>| bytes[0];
    assert_eq!(
        index(postcard::to_allocvec(&MirConstant::Int(1)).expect("postcard")),
        0
    );
    assert_eq!(
        index(
            postcard::to_allocvec(&MirConstant::ShapeSize {
                shape: radix_types::IndexId(0),
                axis: None
            })
            .expect("postcard")
        ),
        11,
        "ShapeSize was the last constant before `inf`"
    );
    assert_eq!(
        index(postcard::to_allocvec(&MirConstant::Magnus(TWO_64.to_owned())).expect("postcard")),
        12
    );
    assert_eq!(
        index(postcard::to_allocvec(&MirScalarLayout::F64).expect("postcard")),
        13
    );
    assert_eq!(
        index(postcard::to_allocvec(&MirScalarLayout::Magnus).expect("postcard")),
        14
    );
}
