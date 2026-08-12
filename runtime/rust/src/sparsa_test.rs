use super::Sparsa;

#[test]
fn vacua_rank_zero_shape() {
    let s: Sparsa<f32> = Sparsa::vacua(&[]).expect("empty shape");
    assert_eq!(s.longitudo(), 0);
    assert_eq!(s.element_count(), Some(1));
    assert_eq!(s.nonnihil(), Ok(0));
}

#[test]
fn vacua_rank_one_shape() {
    let s: Sparsa<f64> = Sparsa::vacua(&[5]).expect("valid shape");
    assert_eq!(s.longitudo(), 1);
    assert_eq!(s.magnitudines(), vec![5]);
    assert_eq!(s.element_count(), Some(5));
    assert_eq!(s.nonnihil(), Ok(0));
}

#[test]
fn vacua_rejects_negative_dimension() {
    let err = Sparsa::<f32>::vacua(&[-1, 3]).unwrap_err();
    assert_eq!(err, "sparsa shape dimension must be non-negative");
}

#[test]
fn accipe_returns_default_for_absent_entry() {
    let s: Sparsa<i64> = Sparsa::vacua(&[2, 3]).expect("valid shape");
    assert_eq!(s.accipe(&[0, 0]), Ok(0i64));
    assert_eq!(s.accipe(&[1, 2]), Ok(0i64));
}

#[test]
fn accipe_rejects_negative_index() {
    let s: Sparsa<f32> = Sparsa::vacua(&[2, 3]).expect("valid shape");
    assert_eq!(s.accipe(&[-1, 0]), Err("sparsa index must be non-negative"));
}

#[test]
fn accipe_rejects_out_of_bounds() {
    let s: Sparsa<f32> = Sparsa::vacua(&[2, 3]).expect("valid shape");
    assert_eq!(s.accipe(&[2, 0]), Err("sparsa index out of bounds"));
}

#[test]
fn accipe_rejects_rank_mismatch() {
    let s: Sparsa<f32> = Sparsa::vacua(&[2, 3]).expect("valid shape");
    assert_eq!(
        s.accipe(&[0]),
        Err("sparsa index rank does not match shape rank")
    );
}

#[test]
fn ponde_stores_and_retrieves() {
    let mut s: Sparsa<f64> = Sparsa::vacua(&[4, 4]).expect("valid shape");
    assert!(s.ponde(&[1, 2], 2.5).is_ok());
    assert_eq!(s.accipe(&[1, 2]), Ok(2.5));
    assert_eq!(s.nonnihil(), Ok(1));
    // Other positions remain default.
    assert_eq!(s.accipe(&[0, 0]), Ok(0.0));
}

#[test]
fn ponde_removes_entry_on_default() {
    let mut s: Sparsa<i64> = Sparsa::vacua(&[3, 3]).expect("valid shape");
    s.ponde(&[0, 0], 42).expect("store");
    assert_eq!(s.nonnihil(), Ok(1));
    // Write default — entry should be removed.
    s.ponde(&[0, 0], 0).expect("store default");
    assert_eq!(s.nonnihil(), Ok(0));
    assert_eq!(s.accipe(&[0, 0]), Ok(0i64));
}

#[test]
fn ponde_replaces_existing_value() {
    let mut s: Sparsa<f32> = Sparsa::vacua(&[2, 2]).expect("valid shape");
    s.ponde(&[0, 0], 1.0).expect("store");
    s.ponde(&[0, 0], 2.0).expect("replace");
    assert_eq!(s.accipe(&[0, 0]), Ok(2.0));
    assert_eq!(s.nonnihil(), Ok(1)); // Still one stored entry.
}

#[test]
fn ponde_rejects_negative_index() {
    let mut s: Sparsa<f32> = Sparsa::vacua(&[2, 2]).expect("valid shape");
    assert_eq!(
        s.ponde(&[-1, 0], 1.0),
        Err("sparsa index must be non-negative")
    );
}

#[test]
fn ponde_rejects_out_of_bounds() {
    let mut s: Sparsa<f32> = Sparsa::vacua(&[2, 2]).expect("valid shape");
    assert_eq!(s.ponde(&[9, 9], 9.0), Err("sparsa index out of bounds"));
}

#[test]
fn ponde_rejects_rank_mismatch() {
    let mut s: Sparsa<f32> = Sparsa::vacua(&[2, 2]).expect("valid shape");
    assert_eq!(
        s.ponde(&[0], 1.0),
        Err("sparsa index rank does not match shape rank")
    );
}

#[test]
fn nonnihil_counts_stored_entries() {
    let mut s: Sparsa<f64> = Sparsa::vacua(&[10, 10]).expect("valid shape");
    assert_eq!(s.nonnihil(), Ok(0));
    s.ponde(&[0, 0], 1.0).expect("store");
    s.ponde(&[5, 5], 2.0).expect("store");
    s.ponde(&[9, 9], 3.0).expect("store");
    assert_eq!(s.nonnihil(), Ok(3));
}

#[test]
fn densata_produces_correct_dense_output() {
    let mut s: Sparsa<i64> = Sparsa::vacua(&[2, 3]).expect("valid shape");
    s.ponde(&[0, 0], 10).expect("store");
    s.ponde(&[1, 2], 20).expect("store");
    let dense = s.densata().expect("densata");
    assert_eq!(dense.magnitudines(), vec![2, 3]);
    // Row-major: [[10, 0, 0], [0, 0, 20]]
    assert_eq!(dense.planata(), vec![10, 0, 0, 0, 0, 20]);
}

#[test]
fn from_tensor_drops_default_values() {
    let dense = super::Tensor::structa(vec![0, 7, 0, 9], &[2, 2]).expect("dense");
    let sparse = Sparsa::from_tensor(&dense);
    assert_eq!(sparse.magnitudines(), vec![2, 2]);
    assert_eq!(sparse.nonnihil(), Ok(2));
    assert_eq!(sparse.accipe(&[0, 0]), Ok(0));
    assert_eq!(sparse.accipe(&[0, 1]), Ok(7));
    assert_eq!(sparse.accipe(&[1, 1]), Ok(9));
}

#[test]
fn from_tensor_preserves_rank_zero_non_default() {
    let dense = super::Tensor::structa(vec![5], &[]).expect("rank-zero dense");
    let sparse = Sparsa::from_tensor(&dense);
    assert_eq!(sparse.longitudo(), 0);
    assert_eq!(sparse.nonnihil(), Ok(1));
    assert_eq!(sparse.accipe(&[]), Ok(5));
}

#[test]
fn densata_empty_sparsa_produces_all_default() {
    let s: Sparsa<f32> = Sparsa::vacua(&[2, 2]).expect("valid shape");
    let dense = s.densata().expect("densata");
    assert_eq!(dense.magnitudines(), vec![2, 2]);
    assert_eq!(dense.planata(), vec![0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn densata_rank_zero() {
    let s: Sparsa<f64> = Sparsa::vacua(&[]).expect("empty shape");
    let dense = s.densata().expect("densata");
    assert_eq!(dense.longitudo(), 0);
    assert_eq!(dense.element_count(), 1);
}

#[test]
fn densata_zero_sized_dimension_produces_empty() {
    let s: Sparsa<f64> = Sparsa::vacua(&[0]).expect("zero-size shape");
    assert_eq!(s.element_count(), Some(0));
    let dense = s.densata().expect("densata zero dim");
    assert_eq!(dense.element_count(), 0);
    assert_eq!(dense.magnitudines(), vec![0]);
}

#[test]
fn densata_multiple_zero_dimensions() {
    let s: Sparsa<f64> = Sparsa::vacua(&[2, 0, 3]).expect("shape with zero dim");
    assert_eq!(s.element_count(), Some(0));
    let dense = s.densata().expect("densata multi zero");
    assert_eq!(dense.element_count(), 0);
    assert_eq!(dense.magnitudines(), vec![2, 0, 3]);
}

#[test]
fn ponde_and_accipe_round_trip_rank_zero() {
    let mut s: Sparsa<f64> = Sparsa::vacua(&[]).expect("empty shape");
    s.ponde(&[], 42.0).expect("store rank-zero");
    assert_eq!(s.accipe(&[]), Ok(42.0));
    assert_eq!(s.nonnihil(), Ok(1));
    // Overwrite with default removes entry.
    s.ponde(&[], 0.0).expect("store default rank-zero");
    assert_eq!(s.nonnihil(), Ok(0));
    assert_eq!(s.accipe(&[]), Ok(0.0));
}

#[test]
fn accipe_rank_zero_after_densata() {
    let s: Sparsa<i64> = Sparsa::vacua(&[]).expect("empty shape");
    let dense = s.densata().expect("densata rank-zero");
    assert_eq!(dense.element_count(), 1);
    assert_eq!(dense.planata(), vec![0]);
}
