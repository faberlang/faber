//! Tests for the radix contract roles and method constants.

use crate::radix_contract::{RadixContractRole, SINKING_FLUSH_METHOD, SINKING_METHOD};

#[test]
fn sinking_role_round_trips() {
    assert_eq!(
        RadixContractRole::parse("sinking"),
        Some(RadixContractRole::Sinking)
    );
    assert_eq!(RadixContractRole::Sinking.as_str(), "sinking");
    assert_eq!(SINKING_METHOD, "sink");
    assert_eq!(SINKING_FLUSH_METHOD, "flush");
}
