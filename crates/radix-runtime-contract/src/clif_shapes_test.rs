use std::mem::{align_of, offset_of, size_of};

use super::{FaberClifLeafFault, FaberClifRawSlice, FaberClifTextHandle};

#[test]
fn clif_shapes_have_expected_64_bit_layout() {
    assert_eq!(size_of::<FaberClifRawSlice>(), 16);
    assert_eq!(size_of::<FaberClifTextHandle>(), 8);
    assert_eq!(size_of::<FaberClifLeafFault>(), 40);

    assert_eq!(align_of::<FaberClifRawSlice>(), 8);
    assert_eq!(align_of::<FaberClifTextHandle>(), 8);
    assert_eq!(align_of::<FaberClifLeafFault>(), 8);

    assert_eq!(offset_of!(FaberClifLeafFault, code), 0);
}
