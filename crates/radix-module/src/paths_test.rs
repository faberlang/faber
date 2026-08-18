//! Layout proofs for [`super::faberlang_home_from`].

use super::{faberlang_home, faberlang_home_from, is_worktree_packet_root};
use crate::test_support::test_temp_dir;
use std::fs;
use std::path::Path;

fn mkdir(path: &Path) {
    fs::create_dir_all(path).expect("create layout dir");
}

fn write_container(root: &Path) {
    mkdir(&root.join("examples"));
    mkdir(&root.join("radix"));
    mkdir(&root.join("norma/src"));
    mkdir(&root.join("hosts/llvm"));
    mkdir(&root.join("worktrees"));
}

#[test]
fn home_walks_past_examples_plus_radix_packet() {
    // This packet's shape: readable examples + writable radix, no hosts/norma.
    let tmp = test_temp_dir("home-partial-examples-radix");
    let container = tmp.path().join("faberlang");
    write_container(&container);

    let packet = container.join("worktrees/hand-x");
    mkdir(&packet.join("examples"));
    mkdir(&packet.join("radix/crates/faber"));

    let start = packet.join("radix/crates/faber");
    let home = faberlang_home_from([start]).expect("container reachable from partial packet");
    assert_eq!(home, container);
    assert!(home.join("hosts/llvm").is_dir());
    assert!(home.join("norma/src").is_dir());
    assert!(home.join("worktrees").is_dir());
}

#[test]
fn home_walks_past_radix_only_packet() {
    // hand-79 shape: radix member only. No examples/ at the packet, so the
    // old walker would already continue — keep that path pinned.
    let tmp = test_temp_dir("home-partial-radix-only");
    let container = tmp.path().join("faberlang");
    write_container(&container);

    let packet = container.join("worktrees/hand-79");
    mkdir(&packet.join("radix/crates/radix-module"));

    let start = packet.join("radix/crates/radix-module");
    let home = faberlang_home_from([start]).expect("container reachable from radix-only packet");
    assert_eq!(home, container);
}

#[test]
fn live_home_is_not_a_worktree_packet() {
    let home = faberlang_home().expect("container reachable from this crate");
    assert!(
        home.join("examples").is_dir(),
        "live home must hold examples/: {}",
        home.display()
    );
    assert!(
        home.join("radix").is_dir() || home.join("norma").is_dir(),
        "live home must hold radix/ or norma/: {}",
        home.display()
    );
    assert!(
        !is_worktree_packet_root(&home),
        "live home must not stop at a packet root: {}",
        home.display()
    );
}

#[test]
fn deployed_layout_without_worktrees_still_qualifies() {
    let tmp = test_temp_dir("home-deployed");
    let container = tmp.path().join("faberlang");
    mkdir(&container.join("examples"));
    mkdir(&container.join("radix/crates/faber"));
    let start = container.join("radix/crates/faber");
    assert_eq!(faberlang_home_from([start]), Some(container));
}
