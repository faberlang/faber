use super::{Arena, ArenaHandle};

#[test]
fn insert_get_preserves_payload() {
    let mut arena = Arena::new();
    let a = arena.insert("geo-A".to_owned());
    let b = arena.insert("mat-B".to_owned());
    assert_eq!(arena.get(a).map(String::as_str), Some("geo-A"));
    assert_eq!(arena.get(b).map(String::as_str), Some("mat-B"));
    assert_eq!(arena.len(), 2);
}

#[test]
fn two_handles_share_one_resource_identity() {
    let mut arena = Arena::new();
    let geo = arena.insert("shared-mesh".to_owned());
    // Two logical "nodes" store the same handle value (identity, not copy of mesh).
    let node_left = geo;
    let node_right = geo;
    assert_eq!(node_left, node_right);
    assert_eq!(
        arena.get(node_left).map(String::as_str),
        Some("shared-mesh")
    );
    assert_eq!(
        arena.get(node_right).map(String::as_str),
        Some("shared-mesh")
    );
}

#[test]
fn reorder_of_handle_lists_does_not_retarget_identity() {
    let mut arena = Arena::new();
    let n0 = arena.insert("root".to_owned());
    let n1 = arena.insert("child-a".to_owned());
    let n2 = arena.insert("child-b".to_owned());
    let mut children = [n1, n2];
    children.swap(0, 1);
    assert_eq!(arena.get(children[0]).map(String::as_str), Some("child-b"));
    assert_eq!(arena.get(children[1]).map(String::as_str), Some("child-a"));
    // Original handles still name the same payloads.
    assert_eq!(arena.get(n1).map(String::as_str), Some("child-a"));
    assert_eq!(arena.get(n2).map(String::as_str), Some("child-b"));
    assert!(arena.contains(n0));
}

#[test]
fn remove_makes_handle_stale_and_rejects_lookup() {
    let mut arena = Arena::new();
    let h = arena.insert(42_i64);
    assert_eq!(arena.get(h), Some(&42));
    assert_eq!(arena.remove(h), Some(42));
    assert_eq!(arena.get(h), None);
    assert!(!arena.contains(h));
    // Reuse of the slot gets a new generation; old handle stays dead.
    let h2 = arena.insert(99_i64);
    assert_ne!(h, h2);
    assert_eq!(arena.get(h), None);
    assert_eq!(arena.get(h2), Some(&99));
}

#[test]
fn out_of_range_handle_is_stale() {
    let arena = Arena::<i32>::new();
    assert_eq!(arena.get(ArenaHandle::new(9, 0)), None);
}
