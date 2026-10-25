use std::collections::HashSet;

use super::{FAULT_CODES, RETIRED_CODES, fault_code};

/// The `IoFault` variants in declaration order in `lib/norma/src/fault.fab`.
const IO_FAULT_VARIANTS: &[&str] = &[
    "NotFound",
    "Permission",
    "NotADirectory",
    "IsADirectory",
    "InvalidArgument",
    "AlreadyExists",
    "TimedOut",
    "ConnectionRefused",
    "ConnectionReset",
    "AddrInUse",
    "Io",
    "Closed",
    "Unset",
    "Unavailable",
    "Cancelled",
];

fn code_number(code: &str) -> u32 {
    code.strip_prefix("ERR")
        .and_then(|digits| digits.parse().ok())
        .unwrap_or_else(|| panic!("code `{code}` is not ERR + digits"))
}

#[test]
fn fault_codes_are_unique() {
    let mut seen = HashSet::new();
    for entry in FAULT_CODES {
        assert!(seen.insert(entry.code), "duplicate code `{}`", entry.code);
    }
}

#[test]
fn fault_codes_family_variant_pairs_are_unique() {
    let mut seen = HashSet::new();
    for entry in FAULT_CODES {
        assert!(
            seen.insert((entry.family, entry.variant)),
            "duplicate pair `{}`/`{}`",
            entry.family,
            entry.variant
        );
    }
}

#[test]
fn fault_codes_match_err_plus_three_digits() {
    for entry in FAULT_CODES {
        let digits = entry
            .code
            .strip_prefix("ERR")
            .unwrap_or_else(|| panic!("code `{}` lacks the ERR prefix", entry.code));
        assert!(
            digits.len() == 3 && digits.bytes().all(|byte| byte.is_ascii_digit()),
            "code `{}` is not ERR + exactly three digits",
            entry.code
        );
    }
}

#[test]
fn fault_codes_are_consecutive_except_retired() {
    let mut present: HashSet<u32> = FAULT_CODES
        .iter()
        .map(|entry| code_number(entry.code))
        .collect();
    for code in RETIRED_CODES {
        present.insert(code_number(code));
    }
    let highest = *present.iter().max().expect("registry is not empty");
    for number in 1..=highest {
        assert!(
            present.contains(&number),
            "ERR{number:03} is a gap not listed in RETIRED_CODES"
        );
    }
}

#[test]
fn fault_codes_retired_are_not_active() {
    let active: HashSet<&str> = FAULT_CODES.iter().map(|entry| entry.code).collect();
    for code in RETIRED_CODES {
        assert!(!active.contains(*code), "retired code `{code}` is active");
    }
}

#[test]
fn fault_codes_iofault_not_found_is_err001() {
    assert_eq!(fault_code("IoFault", "NotFound"), Some("ERR001"));
}

#[test]
fn fault_codes_unknown_iofault_variant_is_none() {
    assert_eq!(fault_code("IoFault", "NoSuch"), None);
}

#[test]
fn fault_codes_every_iofault_variant_has_a_code() {
    for (index, variant) in IO_FAULT_VARIANTS.iter().enumerate() {
        let expected = format!("ERR{:03}", index + 1);
        assert_eq!(
            fault_code("IoFault", variant),
            Some(expected.as_str()),
            "IoFault/{variant} is not registered in declaration order"
        );
    }
}

/// The C runtime repeats the registry as a static table; drift on either side
/// (a new row, a renamed variant, a renumbered code) fails here.
#[test]
fn c_runtime_carries_every_registered_fault_code() {
    let header = include_str!("../../../runtime/c/faber.h");
    for entry in FAULT_CODES {
        let row = format!(
            "{{\"{}\", \"{}\", \"{}\"}}",
            entry.family, entry.variant, entry.code
        );
        assert!(
            header.contains(&row),
            "runtime/c/faber.h lacks the registry row {row}"
        );
    }
}
