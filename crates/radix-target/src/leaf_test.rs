//! Leaf identity pins: route prefixes, builtins, and the declared set.

use super::Leaf;
use crate::Target;

#[test]
fn from_route_maps_each_declared_family() {
    assert_eq!(Leaf::from_route("solum:lege"), Some(Leaf::Fs));
    assert_eq!(Leaf::from_route("consolum:dic"), Some(Leaf::Stdio));
    assert_eq!(Leaf::from_route("processus:argumenta"), Some(Leaf::Process));
    assert_eq!(Leaf::from_route("tempus:dormiet"), Some(Leaf::Clock));
    assert_eq!(Leaf::from_route("aleator:uuid"), Some(Leaf::Entropy));
    assert_eq!(Leaf::from_route("caelum:necte"), Some(Leaf::Tcp));
    assert_eq!(Leaf::from_route("http:get"), Some(Leaf::Http));
    assert_eq!(Leaf::from_route("websocket:connect"), Some(Leaf::Websocket));
    assert_eq!(Leaf::from_route("machina:specimen"), Some(Leaf::Machine));
}

#[test]
fn builtins_and_unknown_routes_are_not_leaves() {
    assert_eq!(Leaf::from_route("processus:exi"), None);
    assert_eq!(Leaf::from_route("runtime:echo"), None);
    assert_eq!(Leaf::from_route("ignotum:route"), None);
}

#[test]
fn all_is_the_declared_set() {
    assert_eq!(
        Leaf::ALL,
        &[
            Leaf::Fs,
            Leaf::Stdio,
            Leaf::Process,
            Leaf::Clock,
            Leaf::Entropy,
            Leaf::Tcp,
            Leaf::Http,
            Leaf::Websocket,
            Leaf::Machine,
        ]
    );
}

#[test]
fn supported_leaves_go_is_exactly_the_ported_set() {
    assert_eq!(
        Target::HirGo.supported_leaves(),
        &[Leaf::Stdio, Leaf::Clock, Leaf::Entropy]
    );
    for leaf in [
        Leaf::Fs,
        Leaf::Process,
        Leaf::Tcp,
        Leaf::Http,
        Leaf::Websocket,
        Leaf::Machine,
    ] {
        assert!(
            !Target::HirGo.supports_leaf(leaf),
            "go must refuse {leaf:?}: no native port"
        );
    }
}

#[test]
fn supported_leaves_wasm_is_stdio_only() {
    assert_eq!(Target::MirWasm.supported_leaves(), &[Leaf::Stdio]);
    assert!(Target::MirWasm.supports_leaf(Leaf::Stdio));
    for leaf in [
        Leaf::Fs,
        Leaf::Process,
        Leaf::Clock,
        Leaf::Entropy,
        Leaf::Tcp,
        Leaf::Http,
        Leaf::Websocket,
        Leaf::Machine,
    ] {
        assert!(
            !Target::MirWasm.supports_leaf(leaf),
            "wasm must refuse {leaf:?}: no WAT probe carrier"
        );
    }
}

#[test]
fn supported_leaves_other_emit_targets_refuse_every_leaf() {
    for target in Target::ALL {
        if matches!(
            target,
            Target::HirRust
                | Target::MirStepper
                | Target::MirFmir
                | Target::MirFmirBinary
                | Target::MirFmirBundle
                | Target::HirFaber
                | Target::HirFhir
                | Target::HirGo
                | Target::MirWasm
        ) {
            continue;
        }
        assert!(
            target.supported_leaves().is_empty(),
            "{target} has no native leaf port and must fail closed"
        );
    }
}
