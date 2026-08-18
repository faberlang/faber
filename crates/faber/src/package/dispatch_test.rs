use super::{host_required_routes, is_builtin_ad_route};
use std::collections::BTreeSet;

#[test]
fn builtin_matcher_covers_echo_and_terminal_exi() {
    assert!(is_builtin_ad_route("runtime:echo"));
    assert!(is_builtin_ad_route("processus:exi"));
    assert!(!is_builtin_ad_route("processus:argumenta"));
    assert!(!is_builtin_ad_route("solum:lege"));
}

#[test]
fn host_required_routes_drop_terminal_exi() {
    let routes = BTreeSet::from(["processus:argumenta".to_owned(), "processus:exi".to_owned()]);
    let required = host_required_routes(&routes);
    assert!(required.contains("processus:argumenta"));
    assert!(!required.contains("processus:exi"));
}
