use std::collections::HashSet;

use super::{
    ABORT_LINE_PREFIX, TRAP_LINE_PREFIX, TRAP_TABLE, TrapKind, abort_exit_class, abort_exit_status,
    exit_class, exit_status, fault_code_kind, io_fault_kind, trap_line,
};
use crate::fault_codes::FAULT_CODES;

/// Every kind, listed exhaustively: adding a variant breaks this match.
fn all_kinds() -> Vec<TrapKind> {
    let kinds = [
        TrapKind::CliOperand,
        TrapKind::Conversion,
        TrapKind::NotFound,
        TrapKind::Unavailable,
        TrapKind::ProgramLogic,
        TrapKind::OsSpawn,
        TrapKind::CannotCreate,
        TrapKind::OtherIo,
        TrapKind::Timeout,
        TrapKind::Protocol,
        TrapKind::Permission,
    ];
    for kind in kinds {
        match kind {
            TrapKind::CliOperand
            | TrapKind::Conversion
            | TrapKind::NotFound
            | TrapKind::Unavailable
            | TrapKind::ProgramLogic
            | TrapKind::OsSpawn
            | TrapKind::CannotCreate
            | TrapKind::OtherIo
            | TrapKind::Timeout
            | TrapKind::Protocol
            | TrapKind::Permission => {}
        }
    }
    kinds.to_vec()
}

#[test]
fn every_row_of_the_table_has_its_ruled_class() {
    let ruled = [
        (TrapKind::CliOperand, 64),
        (TrapKind::Conversion, 65),
        (TrapKind::NotFound, 66),
        (TrapKind::Unavailable, 69),
        (TrapKind::ProgramLogic, 70),
        (TrapKind::OsSpawn, 71),
        (TrapKind::CannotCreate, 73),
        (TrapKind::OtherIo, 74),
        (TrapKind::Timeout, 75),
        (TrapKind::Protocol, 76),
        (TrapKind::Permission, 77),
    ];
    assert_eq!(ruled.len(), TRAP_TABLE.len());
    for (kind, class) in ruled {
        assert_eq!(exit_class(kind), class, "{kind:?}");
        assert_eq!(exit_status(kind), i32::from(class), "{kind:?}");
    }
}

#[test]
fn the_table_lists_every_kind_exactly_once() {
    let kinds = all_kinds();
    assert_eq!(kinds.len(), TRAP_TABLE.len());
    let mut seen = HashSet::new();
    for row in TRAP_TABLE {
        assert!(seen.insert(row.kind), "duplicate row for {:?}", row.kind);
    }
    for kind in kinds {
        assert!(seen.contains(&kind), "no row for {kind:?}");
    }
}

#[test]
fn classes_are_sysexits_and_never_shell_or_signal_statuses() {
    let mut classes = HashSet::new();
    for row in TRAP_TABLE {
        assert!(
            (64..=77).contains(&row.class),
            "{:?} class {} is outside sysexits 64..=77",
            row.kind,
            row.class
        );
        assert!(!matches!(row.class, 126 | 127), "{:?}", row.kind);
        assert!(row.class < 128, "{:?} collides with 128+N", row.kind);
        assert!(row.name.starts_with("EX_"), "{:?}", row.kind);
        assert!(classes.insert(row.class), "class {} reused", row.class);
    }
}

#[test]
fn the_trap_line_is_error_then_the_text() {
    assert_eq!(TRAP_LINE_PREFIX, "error: ");
    assert_eq!(ABORT_LINE_PREFIX, "abort: ");
    assert_eq!(trap_line("numerus overflow"), "error: numerus overflow");
}

#[test]
fn every_io_fault_variant_has_a_kind() {
    let mut variants = 0;
    for entry in FAULT_CODES.iter().filter(|entry| entry.family == "IoFault") {
        variants += 1;
        let kind = io_fault_kind(entry.variant)
            .unwrap_or_else(|| panic!("IoFault::{} has no trap kind", entry.variant));
        assert_eq!(fault_code_kind(entry.code), Some(kind), "{}", entry.code);
    }
    assert!(variants > 0);
    assert_eq!(io_fault_kind("NoSuchVariant"), None);
    assert_eq!(fault_code_kind("ERR999"), None);
    assert_eq!(fault_code_kind("not a code"), None);
}

#[test]
fn io_fault_variants_map_to_the_ruled_classes() {
    let ruled = [
        ("NotFound", 66),
        ("Permission", 77),
        ("NotADirectory", 74),
        ("IsADirectory", 74),
        ("InvalidArgument", 70),
        ("AlreadyExists", 73),
        ("TimedOut", 75),
        ("ConnectionRefused", 69),
        ("ConnectionReset", 74),
        ("AddrInUse", 73),
        ("Io", 74),
        ("Closed", 74),
        ("Unset", 70),
        ("Unavailable", 69),
        ("Cancelled", 75),
    ];
    for (variant, class) in ruled {
        let kind = io_fault_kind(variant).unwrap_or_else(|| panic!("{variant}"));
        assert_eq!(exit_class(kind), class, "{variant}");
    }
}

#[test]
fn an_abort_takes_the_class_of_a_leading_fault_code() {
    assert_eq!(abort_exit_class("ERR001 NotFound"), 66);
    assert_eq!(abort_exit_class("ERR002 Permission"), 77);
    assert_eq!(abort_exit_status("ERR007 TimedOut"), 75);
    assert_eq!(abort_exit_class("ERR005 InvalidArgument"), 70);
}

#[test]
fn any_other_abort_is_program_logic() {
    for reason in [
        "values differ",
        "negative input",
        "",
        "ERR999 unregistered",
        "ERR001",
        "NotFound",
    ] {
        assert_eq!(abort_exit_class(reason), 70, "{reason:?}");
    }
}
