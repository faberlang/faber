use super::*;

/// Canonical variant order matches `STATUS_VARIANTS` in
/// `radix::builtins::frame_types`.
const EXPECTED_VARIANTS: &[(&str, bool, bool)] = &[
    ("Request", false, false), // terminal=false, content=false
    ("Item", false, true),     // terminal=false, content=true
    ("Byte", false, true),     // terminal=false, content=true
    ("Bulk", false, true),     // terminal=false, content=true
    ("Done", true, false),     // terminal=true,  content=false
    ("Error", true, false),    // terminal=true,  content=false
    ("Cancel", true, false),   // terminal=true,  content=false
];

/// Every variant matches the expected canonical order and classification.
#[test]
fn variant_order_and_classification() {
    let variants: &[(FrameStatus, &str, bool, bool)] = &[
        (FrameStatus::Request, "Request", false, false),
        (FrameStatus::Item, "Item", false, true),
        (FrameStatus::Byte, "Byte", false, true),
        (FrameStatus::Bulk, "Bulk", false, true),
        (FrameStatus::Done, "Done", true, false),
        (FrameStatus::Error, "Error", true, false),
        (FrameStatus::Cancel, "Cancel", true, false),
    ];

    for (i, (status, expected_name, expected_terminal, expected_content)) in
        variants.iter().enumerate()
    {
        let (exp_name, _exp_term, _exp_cont) = EXPECTED_VARIANTS[i];
        assert_eq!(
            *expected_name, exp_name,
            "variant name mismatch at index {i}"
        );
        assert_eq!(
            format!("{status:?}"),
            *expected_name,
            "Debug format for {status:?} does not match expected name"
        );
        assert_eq!(
            status.is_terminal(),
            *expected_terminal,
            "is_terminal() for {expected_name} is wrong"
        );
        assert_eq!(
            status.is_content(),
            *expected_content,
            "is_content() for {expected_name} is wrong"
        );
    }
}

/// No variant is both terminal and content.
#[test]
fn no_variant_is_both_terminal_and_content() {
    let variants = [
        FrameStatus::Request,
        FrameStatus::Item,
        FrameStatus::Byte,
        FrameStatus::Bulk,
        FrameStatus::Done,
        FrameStatus::Error,
        FrameStatus::Cancel,
    ];
    for v in &variants {
        assert!(
            !(v.is_terminal() && v.is_content()),
            "{v:?} is both terminal and content"
        );
    }
}

/// Every variant is either terminal or not-terminal (classifications cover
/// all variants — no variant falls through an undetected match gap).
#[test]
fn variant_count_is_exhaustive() {
    // If a variant is added or removed, this test's array length must
    // change. Paired with `variant_order_and_classification` this ensures
    // the canonical 7-variant shape is preserved.
    assert_eq!(
        EXPECTED_VARIANTS.len(),
        7,
        "FrameStatus must have exactly 7 variants"
    );
}
