//! The C leaf-header route table: route identity, header name and the
//! fail-closed admission.

use crate::leaf_routes::{DECLARED_PREFIXES, declared, route_header, route_symbol};

#[test]
fn leaf_routes_dispatch_by_route_prefix() {
    assert_eq!(
        route_symbol("solum:lege").as_deref(),
        Some("faber_route_solum_lege")
    );
    assert_eq!(route_symbol("runtime:printer"), None);
    assert_eq!(route_symbol("processus:exi"), None);
    assert_eq!(route_header("solum"), "solum.h");
    assert!(
        DECLARED_PREFIXES.is_empty(),
        "the C leaf set has landed: a declared prefix needs its runtime/c header and wrappers \
         in the same unit"
    );
    assert!(!declared("solum"), "no C leaf prefix is declared yet");
}
