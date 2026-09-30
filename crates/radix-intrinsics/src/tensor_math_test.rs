//! K5 registry contract: every lifted math function has exactly one tensor row
//! whose Latin name is the function's own and whose MIR op is that function.

use super::*;
use radix_types::MirTensorMath;

#[test]
fn every_lifted_function_has_one_tensor_row_under_its_latin_name() {
    for function in MirTensorMath::ALL {
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
            MirTensorMath::from_latin_name(function.latin_name()),
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
    assert_eq!(rows, MirTensorMath::ALL.len());
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
    let float_only: Vec<_> = MirTensorMath::ALL
        .into_iter()
        .filter(|function| function.float_only())
        .map(MirTensorMath::latin_name)
        .collect();
    assert!(!float_only.contains(&"absolutum"));
    assert_eq!(float_only.len(), MirTensorMath::ALL.len() - 1);
}

#[test]
fn scalars_do_not_gain_the_tensor_only_math_names() {
    // Scalar math stays `absolutum` (numeric) plus the `norma:math` module;
    // the lifted names are tensor rows, not numeric methods.
    for name in [
        "radix",
        "exponentia",
        "logarithmus",
        "sinus",
        "cosinus",
        "tangens",
    ] {
        assert!(
            lookup_method(IntrinsicReceiver::Numeric, name, 0).is_none(),
            "{name}"
        );
        assert!(
            lookup_method(IntrinsicReceiver::Fractus, name, 0).is_none(),
            "{name}"
        );
    }
}
