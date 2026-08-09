//! GI4-2 tests: the session/cadence wire types (the versioned session
//! surface) round-trip on the wire and carry stable spellings — the wire
//! mirrors of the GI4-1-frozen facts are serializable, deterministic, and
//! never silently widened.

use super::{
    FmirArtifactReflection, FmirReflectedKernel, FmirReflectedResource,
    WireInputUpdateCadence, WireInvocationMode, WireKvCacheDtype, WireKvCacheLayout,
    WireKvReservePolicy, WireSessionInput, WireSessionObservationCadence,
};

#[test]
fn reflection_carrier_content_round_trips_through_the_wire() {
    // DDCP4-U4 content: the reflection carrier (with the leaf-mapped content
    // shape) survives the binary wire verbatim with its own version ratchet.
    let reflection = FmirArtifactReflection {
        v: super::super::ARTIFACT_REFLECTION_VERSION,
        kernels: vec![FmirReflectedKernel {
            function: "addita".to_owned(),
            entry_name: "addita".to_owned(),
            resources: vec![
                FmirReflectedResource {
                    name: "x".to_owned(),
                    binding: 0,
                },
                FmirReflectedResource {
                    name: "y".to_owned(),
                    binding: 1,
                },
            ],
        }],
    };
    let bytes = postcard::to_allocvec(&reflection).expect("reflection serializes");
    let decoded: FmirArtifactReflection = postcard::from_bytes(&bytes).expect("reflection decodes");
    assert_eq!(decoded, reflection);
    assert_eq!(decoded.v, super::super::ARTIFACT_REFLECTION_VERSION);
}

#[test]
fn session_wire_types_round_trip_through_postcard() {
    // Input cadence: both variants survive the wire verbatim.
    for cadence in [
        WireInputUpdateCadence::Resident,
        WireInputUpdateCadence::PerInvocation,
    ] {
        let bytes = postcard::to_allocvec(&cadence).expect("cadence serializes");
        let decoded: WireInputUpdateCadence =
            postcard::from_bytes(&bytes).expect("cadence decodes");
        assert_eq!(decoded, cadence);
    }
    // Invocation modes: both workload modes survive the wire verbatim.
    for mode in [
        WireInvocationMode::Prefill,
        WireInvocationMode::ScalarDecode,
    ] {
        let bytes = postcard::to_allocvec(&mode).expect("mode serializes");
        let decoded: WireInvocationMode = postcard::from_bytes(&bytes).expect("mode decodes");
        assert_eq!(decoded, mode);
    }
    // Observation cadences: both cadences survive the wire verbatim.
    for cadence in [
        WireSessionObservationCadence::PerInvocation,
        WireSessionObservationCadence::PerSession,
    ] {
        let bytes = postcard::to_allocvec(&cadence).expect("cadence serializes");
        let decoded: WireSessionObservationCadence =
            postcard::from_bytes(&bytes).expect("cadence decodes");
        assert_eq!(decoded, cadence);
    }
}

#[test]
fn spellings_are_stable_and_mirror_the_contract_vocabulary() {
    // The regime labels are the GI4-1 frozen spellings (the S6 regime schema
    // — GI4 owns the `decode` label; no end-to-end number is mislabeled).
    assert_eq!(WireInvocationMode::Prefill.spelling(), "prefill");
    assert_eq!(WireInvocationMode::ScalarDecode.spelling(), "scalar_decode");
    assert_eq!(WireKvCacheDtype::F32.spelling(), "f32");
    assert_eq!(
        WireSessionObservationCadence::PerInvocation.spelling(),
        "per-invocation"
    );
    assert_eq!(
        WireSessionObservationCadence::PerSession.spelling(),
        "per-session"
    );
}

#[test]
fn kv_layout_round_trips_with_all_carried_facts_preserved() {
    let layout = WireKvCacheLayout {
        slots: 1,
        context_length: 8192,
        layer_count: 32,
        kv_head_count: 5,
        head_dim: 64,
        dtype: WireKvCacheDtype::F32,
        reserve_policy: WireKvReservePolicy { bytes: 0 },
    };
    let bytes = postcard::to_allocvec(&layout).expect("layout serializes");
    let decoded: WireKvCacheLayout = postcard::from_bytes(&bytes).expect("layout decodes");
    // Every carried fact survives the wire (the carriage is field-for-field).
    assert_eq!(decoded.slots, 1);
    assert_eq!(decoded.context_length, 8192);
    assert_eq!(decoded.layer_count, 32);
    assert_eq!(decoded.kv_head_count, 5);
    assert_eq!(decoded.head_dim, 64);
    assert_eq!(decoded.dtype, WireKvCacheDtype::F32);
    assert_eq!(decoded.reserve_policy, WireKvReservePolicy { bytes: 0 });
    // The dtype surface is closed: the wire carries exactly the declared f32
    // representation (a dtype change is a contract revision).
    assert!(matches!(decoded.dtype, WireKvCacheDtype::F32));
}

#[test]
fn session_input_slot_round_trips_with_its_cadence() {
    for cadence in [
        WireInputUpdateCadence::Resident,
        WireInputUpdateCadence::PerInvocation,
    ] {
        let input = WireSessionInput { slot: 7, cadence };
        let bytes = postcard::to_allocvec(&input).expect("input serializes");
        let decoded: WireSessionInput = postcard::from_bytes(&bytes).expect("input decodes");
        assert_eq!(decoded.slot, 7);
        assert_eq!(decoded.cadence, cadence);
    }
}
