use super::*;

#[test]
fn artifact_version_is_7_for_the_device_artifact_packet_schema() {
    assert_eq!(PACKAGE_MIR_ARTIFACT_VERSION, 7);
}

#[test]
fn device_program_wire_version_is_8_after_the_gi3_projection_break() {
    // Stage 3R U1 bumped the nested device-program wire version 3 → 4
    // (S1); S5-U5b bumps it 4 → 5 (the declared RepeatingStep step count
    // rides the lifetime variant); S5A-U1 bumps it 5 → 6 (every result
    // row's observation fact carries the explicit PerStep/EndOfRun
    // cadence); S6-C2 bumps it 6 → 7 (the recipe-variant wire mirrors and
    // the typed rank-extension broadcast fact change the canonical wire
    // bytes); council-7/council-8 (CB-2/CB-3) bumps it 7 → 8 (the GI3-1
    // shape changes become version-visible — wire-7 was an unpublished
    // clean break, no pre-GI3-1 wire-7 artifact escaped the repo — and
    // every AxisReduction plan + reduced buffer version carries the
    // producer-defined reduced-resource projection). The nested wire
    // version ratchets independently of the outer image version (S8) —
    // it currently does NOT coincide numerically with
    // `PACKAGE_MIR_ARTIFACT_VERSION`; the two are admitted by separate
    // gates.
    assert_eq!(WIRE_DEVICE_PROGRAM_VERSION, 8);
}

#[test]
fn distributed_section_versions_leave_the_accepted_wire_untouched() {
    // MD2-W1 (A7): the distributed section rides the accepted wire 8 as
    // an optional sibling field with its OWN version ratchet. The nested
    // device-program wire version, the outer image version, and the
    // section's own version are admitted by separate gates; none of the
    // frozen version constants changes with the distributed section.
    assert_eq!(WIRE_DISTRIBUTED_SECTION_VERSION, 1);
    assert_eq!(WIRE_DEVICE_PROGRAM_VERSION, 8);
    assert_eq!(PACKAGE_MIR_ARTIFACT_VERSION, 7);
}

#[test]
fn target_tags_are_configured() {
    assert_eq!(FMIR_TEXT_TARGET_NAME, "fmir-text");
    assert_eq!(FMIR_TARGET_NAME, "fmir");
}

#[test]
fn host_requirements_allowlist() {
    assert!(is_known_host_requirement("host:argv"));
    assert!(is_known_host_requirement("host:fs"));
    assert!(is_known_host_requirement("host:process"));
    assert!(!is_known_host_requirement("host:nope"));
    assert!(!is_known_host_requirement("kernel:foo.bar"));
    assert!(!is_known_host_requirement(""));
}

#[test]
fn target_feature_allowlist_extends_the_host_surface() {
    // DDCP2-U3: the device requirements migrate to typed target
    // features — `device:metal`/`device:cuda` spellings move to the
    // typed backend/target identity on the artifact. The known target
    // features are the compiled-in leaves' declared feature ids.
    assert!(is_known_target_feature("metal"));
    assert!(is_known_target_feature("cuda"));
    assert!(is_known_target_feature("macos-arm64"));
    assert!(is_known_target_feature("sm_120"));
    assert!(!is_known_target_feature("device:metal"));
    assert!(!is_known_target_feature("vulkan"));
    assert!(!is_known_target_feature(""));
}
