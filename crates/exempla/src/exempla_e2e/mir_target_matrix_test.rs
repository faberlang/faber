use super::{classify_matrix_row, display_path, target_cell, MirTargetMatrixRow};
use radix::driver::Session;
use radix::mir::{Lowerability, MirCoverageTarget};
use radix::Config;
use rustc_hash::FxHashMap;
use std::path::PathBuf;

#[test]
fn target_cell_marks_non_mir_exempla_as_n_a() {
    let row = MirTargetMatrixRow {
        path: PathBuf::from("cli/cli.fab"),
        mir_bucket: "intentional-subset",
        mir_capable: false,
        targets: FxHashMap::default(),
        scena_structural: None,
    };
    assert_eq!(target_cell(&row, MirCoverageTarget::LlvmText), "n/a");
}

#[test]
fn display_path_normalizes_corpus_paths() {
    let path = crate::paths::corpus_dir().join("incipit/salve-munde.fab");
    let rendered = display_path(&path);
    assert!(rendered.ends_with("incipit/salve-munde.fab"));
}

#[test]
fn target_cell_reports_first_gap_shape() {
    let mut targets = FxHashMap::default();
    targets.insert(
        MirCoverageTarget::WgslText,
        Lowerability::Rejected(vec![radix::mir::CapabilityGap::TargetPolicyRejection {
            shape: "wgsl-text requires at least one @ nucleum function".to_owned(),
            slug: "target-policy-rejection",
        }]),
    );
    let row = MirTargetMatrixRow {
        path: PathBuf::from("incipit/salve-munde.fab"),
        mir_bucket: "mir-lowered",
        mir_capable: true,
        targets,
        scena_structural: None,
    };
    assert_eq!(
        target_cell(&row, MirCoverageTarget::WgslText),
        "wgsl-text requires at least one @ nucleum function"
    );
}

#[test]
fn matrix_row_uses_post_lowering_interner_for_dum_in_functione() {
    // This corpus row interns MIR diagnostic templates after analysis. Coverage
    // consumers must resolve symbols through the lowerer's final interner.
    let file = crate::paths::corpus_dir().join("dum/in-functione.fab");
    let row = classify_matrix_row(&Session::new(Config::default()), &file);

    assert!(
        row.mir_capable,
        "expected MIR lowering for {}",
        file.display()
    );
    assert!(
        matches!(
            row.targets.get(&MirCoverageTarget::LlvmText),
            Some(Lowerability::Capable)
        ),
        "the LLVM matrix classifier must preserve this row's capable result instead of panicking"
    );
}

#[test]
fn matrix_rows_match_sexp_aggregate_and_function_constant_support() {
    let session = Session::new(Config::default());
    let matrix = crate::paths::corpus_dir().join("gpu-core-types/matrix-register.fab");
    let matrix_row = classify_matrix_row(&session, &matrix);
    assert!(
        matrix_row.mir_capable,
        "expected MIR lowering for {}",
        matrix.display()
    );
    assert!(
        matches!(
            matrix_row.targets.get(&MirCoverageTarget::SexpStructural),
            Some(Lowerability::Capable)
        ),
        "matrix construction is structurally supported by the S-expression emitter: {:?}",
        matrix_row.targets.get(&MirCoverageTarget::SexpStructural)
    );

    for relative in [
        "clausa/clausa.fab",
        "clausura/clausura.fab",
        "integratio/arena-mixta.fab",
    ] {
        let file = crate::paths::corpus_dir().join(relative);
        let row = classify_matrix_row(&session, &file);
        assert!(
            row.mir_capable,
            "expected MIR lowering for {}",
            file.display()
        );
        for target in [MirCoverageTarget::SexpStructural, MirCoverageTarget::Sexp] {
            assert!(
                matches!(row.targets.get(&target), Some(Lowerability::Capable)),
                "{relative} must remain capable for {}: {:?}",
                target.name(),
                row.targets.get(&target)
            );
        }
    }
}
