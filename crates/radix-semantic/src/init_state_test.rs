//! Tests for definite-assignment initialization states.
//!
//! These tests prove that `InitState` variants (Assigned, Unassigned,
//! `MaybeAssigned`) correctly report their initialization status and
//! that state transitions follow the expected lattice.

use super::InitState;

#[test]
fn definitely_initialized_only_when_assigned() {
    assert!(InitState::Assigned.is_definitely_initialized());
    assert!(!InitState::Unassigned.is_definitely_initialized());
    assert!(!InitState::MaybeAssigned.is_definitely_initialized());
}

#[test]
fn join_paths_preserves_agreement_and_degrades_mixed_states() {
    assert_eq!(
        InitState::Assigned.join_paths(InitState::Assigned),
        InitState::Assigned
    );
    assert_eq!(
        InitState::Unassigned.join_paths(InitState::Unassigned),
        InitState::Unassigned
    );
    assert_eq!(
        InitState::Assigned.join_paths(InitState::Unassigned),
        InitState::MaybeAssigned
    );
    assert_eq!(
        InitState::MaybeAssigned.join_paths(InitState::Assigned),
        InitState::MaybeAssigned
    );
}
