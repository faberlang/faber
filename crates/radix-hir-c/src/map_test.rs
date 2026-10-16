//! Maps (`tabula<K,V>`): parallel key/value lists behind an arena pointer.

use crate::test_support::emitted;

#[test]
fn map_basics_use_the_map_helpers() {
    let code = emitted(
        "main {\n    mut map<string, int> m ← empty\n    m.put(\"a\", 1)\n    m[\"b\"] ← 2\n    print m.length(), m.has(\"a\"), m[\"a\"]\n    let bool t ← m.delete(\"b\")\n    print t\n    let int ∪ none g ← m.get(\"a\")\n    print g\n}",
    );
    assert!(code.contains("faber_map_text_i64_new(faber_a)"), "{code}");
    assert!(code.contains("faber_map_text_i64_put(faber_a,"), "{code}");
    assert!(code.contains("faber_map_text_i64_has("), "{code}");
    assert!(code.contains("faber_map_text_i64_delete("), "{code}");
    assert!(code.contains("faber_map_text_i64_get("), "{code}");
    assert!(code.contains("tabula missing key"), "{code}");
}

#[test]
fn a_map_literal_is_a_json_object_construction() {
    let code = emitted(
        "main {\n    let map<string, int> m ← { \"x\": 1, \"y\": 2 }\n    print m.length()\n}",
    );
    assert!(code.contains("faber_map_text_i64_put(faber_a,"), "{code}");
    assert!(code.contains("\"x\""), "{code}");
}

#[test]
fn map_membership_is_key_containment() {
    let code = emitted(
        "main {\n    let map<string, int> m ← { \"x\": 1 }\n    print \"x\" ∈ m, \"z\" ∉ m\n}",
    );
    assert!(code.contains("(faber_map_text_i64_has("), "{code}");
}

#[test]
fn map_projections_pick_the_side_by_element_class() {
    let code = emitted(
        "main {\n    let map<string, int> m ← { \"x\": 1 }\n    print m ↦ list<string>, m ↦ list<int>\n}",
    );
    assert!(code.contains("faber_map_text_i64_keys(faber_a,"), "{code}");
    assert!(
        code.contains("faber_map_text_i64_values(faber_a,"),
        "{code}"
    );
}

#[test]
fn map_iteration_walks_values_with_the_key_coordinate() {
    let code = emitted(
        "main {\n    let map<string, int> m ← { \"a\": 1 }\n    for from m let v {\n        print v\n    }\n    for from m at [k] let v {\n        print k, v\n    }\n}",
    );
    assert!(code.contains("->values->items["), "{code}");
    assert!(code.contains("->keys->items["), "{code}");
    assert!(code.contains("map changed size during iteration"), "{code}");
}

#[test]
fn a_map_sort_orders_values_or_keys() {
    let code = emitted(
        "main {\n    let map<string, int> m ← { \"x\": 1 }\n    print sort via asc from m\n    print sort via desc ref m\n}",
    );
    assert!(
        code.contains("faber_list_i64_sorted(faber_a, faber_map_text_i64_values(faber_a,"),
        "{code}"
    );
    assert!(
        code.contains("faber_list_text_sorted(faber_a, faber_map_text_i64_keys(faber_a,"),
        "{code}"
    );
}
