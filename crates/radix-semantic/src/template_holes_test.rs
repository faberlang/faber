//! Position-law fixtures for named template holes.

use super::template_holes::{Hole, HoleKind, analyze_template};

fn hole(kind: HoleKind, position: usize) -> Hole {
    Hole { kind, position }
}

#[test]
fn named_hole_uses_the_first_automatic_position_and_erases() {
    let analysis = analyze_template("§{greet} world");

    assert_eq!(
        analysis.holes,
        vec![hole(HoleKind::Named("greet".to_owned()), 0)]
    );
    assert_eq!(analysis.label_position("greet"), Some(0));
    assert_eq!(analysis.erased, "§ world");
}

#[test]
fn named_and_anonymous_holes_share_one_position_domain() {
    let analysis = analyze_template("§{greet} §");

    assert_eq!(
        analysis.holes,
        vec![
            hole(HoleKind::Named("greet".to_owned()), 0),
            hole(HoleKind::Auto, 1),
        ]
    );
    assert_eq!(analysis.erased, "§ §");
}

#[test]
fn explicit_index_before_named_hole_keeps_existing_auto_positioning() {
    let analysis = analyze_template("§1 §{greet}");

    assert_eq!(
        analysis.holes,
        vec![
            hole(HoleKind::Index(1), 1),
            hole(HoleKind::Named("greet".to_owned()), 0),
        ]
    );
    assert_eq!(analysis.label_position("greet"), Some(0));
    assert_eq!(analysis.erased, "§1 §");
}

#[test]
fn explicit_index_can_address_a_named_hole_position() {
    let analysis = analyze_template("§{greet} §0");

    assert_eq!(
        analysis.holes,
        vec![
            hole(HoleKind::Named("greet".to_owned()), 0),
            hole(HoleKind::Index(0), 0),
        ]
    );
    assert_eq!(analysis.erased, "§ §0");
}

#[test]
fn repeated_label_shares_the_first_position() {
    let analysis = analyze_template("§{a} §{a}");

    assert_eq!(
        analysis.holes,
        vec![
            hole(HoleKind::Named("a".to_owned()), 0),
            hole(HoleKind::Named("a".to_owned()), 0),
        ]
    );
    assert_eq!(analysis.label_position("a"), Some(0));
    assert_eq!(analysis.erased, "§0 §0");
    assert_eq!(analysis.value_slots(), (1, 1));
}

#[test]
fn repeated_label_does_not_consume_an_auto_position() {
    let analysis = analyze_template("§{a} §{b} §{a} §");

    assert_eq!(
        analysis.holes,
        vec![
            hole(HoleKind::Named("a".to_owned()), 0),
            hole(HoleKind::Named("b".to_owned()), 1),
            hole(HoleKind::Named("a".to_owned()), 0),
            hole(HoleKind::Auto, 2),
        ]
    );
    assert_eq!(analysis.label_position("a"), Some(0));
    assert_eq!(analysis.label_position("b"), Some(1));
    assert_eq!(analysis.erased, "§0 §1 §0 §2");
    assert_eq!(analysis.value_slots(), (3, 3));
}

#[test]
fn templates_without_holes_pass_through_unchanged() {
    let analysis = analyze_template("plain text {not a hole}");

    assert!(analysis.holes.is_empty());
    assert!(analysis.label_positions.is_empty());
    assert_eq!(analysis.erased, "plain text {not a hole}");
}

#[test]
fn invalid_label_body_falls_back_to_literal_text_after_an_auto_hole() {
    let analysis = analyze_template("§{a b}");

    assert_eq!(analysis.holes, vec![hole(HoleKind::Auto, 0)]);
    assert!(analysis.label_positions.is_empty());
    assert_eq!(analysis.erased, "§{a b}");
}

#[test]
fn value_slots_require_the_highest_index_plus_one() {
    let analysis = analyze_template("§0, §0, §0, and §1");

    assert_eq!(analysis.value_slots(), (2, 2));
}

#[test]
fn value_slots_report_unread_positions_as_distinct_slots() {
    let analysis = analyze_template("§0 and §2");

    assert_eq!(analysis.value_slots(), (3, 2));
}

#[test]
fn first_unread_slot_names_the_first_missing_position() {
    let analysis = analyze_template("§0 and §2");

    assert_eq!(analysis.first_unread_slot(), Some(1));
}

#[test]
fn first_unread_slot_is_none_when_every_position_is_read() {
    assert_eq!(
        analyze_template("§0, §0, §0, and §1").first_unread_slot(),
        None
    );
    assert_eq!(analyze_template("§1 §").first_unread_slot(), None);
    assert_eq!(analyze_template("§2 § §{named}").first_unread_slot(), None);
    assert_eq!(analyze_template("plain").first_unread_slot(), None);
}

#[test]
fn value_slots_follow_automatic_and_explicit_positions() {
    assert_eq!(analyze_template("§1 §").value_slots(), (2, 2));
    assert_eq!(analyze_template("§{greet} §0").value_slots(), (1, 1));
    assert_eq!(analyze_template("plain text").value_slots(), (0, 0));
}

#[test]
fn explicit_indexes_do_not_consume_automatic_positions() {
    let analysis = analyze_template("§2 § §{named}");

    assert_eq!(
        analysis.holes,
        vec![
            hole(HoleKind::Index(2), 2),
            hole(HoleKind::Auto, 0),
            hole(HoleKind::Named("named".to_owned()), 1),
        ]
    );
    assert_eq!(analysis.erased, "§2 § §");
}
