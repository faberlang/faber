//! K5 registry contract: every lifted math function has exactly one tensor row
//! whose Latin name is the function's own and whose MIR op is that function.

use super::*;
use radix_types::MirMathFunction;

#[test]
fn every_lifted_function_has_one_tensor_row_under_its_latin_name() {
    for function in MirMathFunction::ALL {
        let entry = lookup_method(IntrinsicReceiver::Tensor, function.latin_name(), 0)
            .unwrap_or_else(|| panic!("no tensor row for {}", function.latin_name()));
        assert_eq!(entry.return_kind, IntrinsicReturnKind::SameAsReceiver);
        assert!(entry.params.is_empty() && entry.generic.is_empty());
        assert_eq!(
            mir_collection_op(entry),
            Some(MirCollectionOp::TensorMath(function)),
            "{} row lowers to the wrong function",
            function.latin_name()
        );
        assert_eq!(
            MirMathFunction::from_latin_name(function.latin_name()),
            Some(function)
        );
    }
}

#[test]
fn no_tensor_row_lowers_to_an_unlisted_math_function() {
    let rows = INTRINSICS
        .iter()
        .filter(|entry| {
            matches!(
                mir_collection_op(entry),
                Some(MirCollectionOp::TensorMath(_))
            )
        })
        .count();
    assert_eq!(rows, MirMathFunction::ALL.len());
    assert!(
        INTRINSICS
            .iter()
            .filter(|entry| entry.receiver == IntrinsicReceiver::Tensor)
            .all(|entry| !matches!(
                mir_collection_op(entry),
                Some(MirCollectionOp::TensorMath(function)) if function.latin_name() != entry.faber_name
            ))
    );
}

#[test]
fn only_absolutum_takes_integer_elements() {
    let float_only: Vec<_> = MirMathFunction::ALL
        .into_iter()
        .filter(|function| function.float_only())
        .map(MirMathFunction::latin_name)
        .collect();
    assert!(!float_only.contains(&"absolutum"));
    assert_eq!(float_only.len(), MirMathFunction::ALL.len() - 1);
}

#[test]
fn every_scalar_function_has_one_fractus_row_and_no_integer_row() {
    // Scalar `absolutum` is the numeric `Absolute` row; the other seven are
    // float-only rows, so an integer receiver has no such method.
    for function in MirMathFunction::SCALAR {
        let entry = lookup_method(IntrinsicReceiver::Fractus, function.latin_name(), 0)
            .unwrap_or_else(|| panic!("no fractus row for {}", function.latin_name()));
        assert_eq!(entry.return_kind, IntrinsicReturnKind::SameAsReceiver);
        assert!(entry.params.is_empty() && entry.generic.is_empty());
        assert_eq!(
            mir_collection_op(entry),
            Some(MirCollectionOp::ScalarMath(function)),
            "{} row lowers to the wrong function",
            function.latin_name()
        );
        assert!(
            lookup_method(IntrinsicReceiver::Numeric, function.latin_name(), 0).is_none(),
            "{} must not be a numeric (integer) method",
            function.latin_name()
        );
    }
    let scalar_rows = INTRINSICS
        .iter()
        .filter(|entry| {
            matches!(
                mir_collection_op(entry),
                Some(MirCollectionOp::ScalarMath(_))
            )
        })
        .count();
    assert_eq!(scalar_rows, MirMathFunction::SCALAR.len());
    assert!(!MirMathFunction::SCALAR.contains(&MirMathFunction::Abs));
    assert_eq!(
        MirMathFunction::SCALAR.len(),
        MirMathFunction::ALL.len() - 1
    );
}
