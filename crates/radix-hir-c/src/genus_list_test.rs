//! Emission assertions for concrete genus list storage and identity.

use crate::test_support::emitted;

#[test]
fn genus_lists_use_pointer_families_and_preserve_object_identity() {
    let code = emitted(
        "class Point {\n    mut int x = 0\n}\nfn make_points() → list<Point> {\n    let Point p ← { x = 1 }\n    return [p]\n}\nmain {\n    mut list<Point> points ← make_points()\n    let Point alias ← points[0]\n    points[0] ← alias\n    for from points let point {\n        print point.x\n    }\n}",
    );

    for expected in [
        "typedef g_Point *faber_elem_g_Point;",
        "FABER_LIST_FAMILY(g_Point, faber_elem_g_Point, NULL)",
        "faber_list_g_Point_of(faber_a, 1, (faber_elem_g_Point[])",
        "faber_list_g_Point_at(l_points,",
        "faber_list_g_Point_set(l_points,",
        "faber_list_g_Point *const",
        "->items[faber_i",
    ] {
        assert!(
            code.contains(expected),
            "{expected:?} missing from:\n{code}"
        );
    }
}

#[test]
fn an_entry_local_genus_list_requests_its_family_before_declarations() {
    let code = emitted(
        "class Point {\n    mut int x = 0\n}\nmain {\n    let Point point ← { x = 3 }\n    let list<Point> points ← [point]\n    print points[0].x\n}",
    );

    assert!(
        code.contains("FABER_LIST_FAMILY(g_Point, faber_elem_g_Point, NULL)"),
        "{code}"
    );
    assert!(code.contains("faber_list_g_Point_of(faber_a, 1"), "{code}");
}

#[test]
fn genus_lists_support_empty_values_append_and_nested_fields() {
    let code = emitted(
        "class Point {\n    mut int x = 0\n}\nclass Group {\n    mut list<Point> points\n}\nmain {\n    mut list<Point> points ← []\n    mut Point point ← { x = 3 }\n    points.append(point)\n    mut Group group ← { points = points }\n    print group.points[0].x\n}",
    );

    for expected in [
        "faber_list_g_Point_new(faber_a, 0)",
        "faber_list_g_Point_push(faber_a,",
        "faber_list_g_Point_at(",
        "faber_elem_g_Point",
    ] {
        assert!(
            code.contains(expected),
            "{expected:?} missing from:\n{code}"
        );
    }
}
