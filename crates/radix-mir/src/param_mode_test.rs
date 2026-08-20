use crate::MirParamMode;

/// The D-14 Rust names change, but each mode keeps its existing serialized
/// label so MIR artifacts remain wire-compatible.
#[test]
fn mir_param_mode_serde_labels_round_trip() {
    let cases = [
        (MirParamMode::Owned, r#""Owned""#),
        (MirParamMode::Ref, r#""De""#),
        (MirParamMode::MutRef, r#""In""#),
        (MirParamMode::Move, r#""Ex""#),
    ];

    for (mode, wire_label) in cases {
        assert_eq!(
            serde_json::to_string(&mode).expect("mode serializes"),
            wire_label
        );
        assert_eq!(
            serde_json::from_str::<MirParamMode>(wire_label).expect("wire label deserializes"),
            mode
        );
        let round_trip = serde_json::from_str::<MirParamMode>(
            &serde_json::to_string(&mode).expect("mode serializes for round-trip"),
        )
        .expect("serialized mode deserializes for round-trip");
        assert_eq!(round_trip, mode);
    }
}
