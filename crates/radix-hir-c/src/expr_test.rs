use super::{CAP_MAX, fold_bitwise};
use radix_hir::HirBinOp;

#[test]
fn zero_left_shift_folds_without_shifting_the_machine_word() {
    assert_eq!(fold_bitwise(HirBinOp::Shl, 0, 128), Some(0));
    assert_eq!(fold_bitwise(HirBinOp::Shl, 0, CAP_MAX), Some(0));
    assert_eq!(fold_bitwise(HirBinOp::Shl, 0, -1), None);
}
