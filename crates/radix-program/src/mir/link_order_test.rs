//! Link-time interning order must not depend on the process hash seed.

use super::DefId;
use super::link::ordered_method_keys;
use std::collections::HashMap;

#[test]
fn method_keys_visit_in_stable_order_across_hash_seeds() {
    let entries: Vec<((DefId, String), DefId)> = (0..64u32)
        .map(|n| ((DefId(n % 7), format!("verb{}", n % 11)), DefId(1000 + n)))
        .collect();
    let mut expected: Vec<(DefId, String)> = entries.iter().map(|(key, _)| key.clone()).collect();
    expected.sort_by(|a, b| (a.0.0, &a.1).cmp(&(b.0.0, &b.1)));
    expected.dedup();

    // Each `HashMap` draws its own random seed, so several maps over the same
    // entries iterate in different orders.
    for _ in 0..16 {
        let methods: HashMap<(DefId, String), DefId> = entries.iter().cloned().collect();
        let ordered: Vec<(DefId, String)> =
            ordered_method_keys(&methods).into_iter().cloned().collect();
        assert_eq!(ordered, expected);
    }
}
