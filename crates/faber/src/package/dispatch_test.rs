use super::{host_required_routes, is_builtin_ad_route, is_leaf_ad_route};
use std::collections::BTreeSet;

#[test]
fn builtin_matcher_covers_echo_and_terminal_exi() {
    assert!(is_builtin_ad_route("runtime:echo"));
    assert!(is_builtin_ad_route("processus:exi"));
    assert!(!is_builtin_ad_route("processus:argumenta"));
    assert!(!is_builtin_ad_route("solum:lege"));
}

#[test]
fn leaf_matcher_covers_the_n2_families() {
    assert!(is_leaf_ad_route("solum:lege"));
    assert!(is_leaf_ad_route("solum:scribe"));
    assert!(is_leaf_ad_route("consolum:dic"));
    assert!(is_leaf_ad_route("tempus:dormiet"));
    assert!(is_leaf_ad_route("processus:argumenta"));
    assert!(is_leaf_ad_route("aleator:uuid"));
    assert!(!is_leaf_ad_route("http:get"));
}

#[test]
fn host_required_routes_drop_terminal_exi_and_leaf() {
    let routes = BTreeSet::from([
        "processus:argumenta".to_owned(),
        "processus:exi".to_owned(),
        "solum:lege".to_owned(),
        "http:get".to_owned(),
    ]);
    let required = host_required_routes(&routes);
    assert!(!required.contains("processus:argumenta"));
    assert!(!required.contains("processus:exi"));
    assert!(!required.contains("solum:lege"));
    assert!(required.contains("http:get"));
}
