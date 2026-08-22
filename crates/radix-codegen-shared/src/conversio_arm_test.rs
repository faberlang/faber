use super::*;

#[test]
fn dedicated_frame_records_dedicated() {
    conversio_arm_clear();
    {
        let _guard = conversio_arm_begin(None, None);
    }
    let records = conversio_arm_take();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].arm, ConversioArm::Dedicated);
}

#[test]
fn fallback_mark_flips_top_frame() {
    conversio_arm_clear();
    {
        let _guard = conversio_arm_begin(None, None);
        conversio_arm_mark_fallback();
    }
    let records = conversio_arm_take();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].arm, ConversioArm::Fallback);
}

#[test]
fn nested_frames_mark_the_innermost() {
    conversio_arm_clear();
    {
        let outer = conversio_arm_begin(Some(TypeId(1)), Some(TypeId(2)));
        {
            let _inner = conversio_arm_begin(Some(TypeId(3)), Some(TypeId(4)));
            conversio_arm_mark_fallback();
        }
        drop(outer);
    }
    let records = conversio_arm_take();
    assert_eq!(records.len(), 2);
    // The inner conversion completes (drops) before the outer, so it is
    // recorded first — with the fallback mark, not the outer's dedicated.
    assert_eq!(records[0].arm, ConversioArm::Fallback);
    assert_eq!(records[0].source_ty, Some(TypeId(3)));
    assert_eq!(records[1].arm, ConversioArm::Dedicated);
    assert_eq!(records[1].source_ty, Some(TypeId(1)));
}
