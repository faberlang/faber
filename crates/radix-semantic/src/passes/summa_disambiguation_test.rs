//! SFR-1 (summa-filum-reduction) pins: the `.summa()` tensor receiver stays
//! an ordinary intrinsic method. Anchored at `examples/training/device-summa*`
//! (both exemplars call `a.summa()` / `medius.summa()` as a plain reduction
//! method). SFR-2+ must not move `summa` out of this ordinary-method row when
//! the head-position `summa ex` spelling is admitted.

use super::air_tensor_intrinsics::is_allowed_tensor_intrinsic;

#[test]
fn summa_receiver_remains_ordinary_tensor_intrinsic() {
    assert!(
        is_allowed_tensor_intrinsic("summa"),
        "`.summa()` receiver (examples/training/device-summa*) must stay an \
         admitted ordinary tensor intrinsic"
    );
}
