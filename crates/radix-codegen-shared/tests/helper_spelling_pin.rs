//! Ratchet: no emitter hand-spells a runtime helper name (codegen-readability
//! T1-L).
//!
//! Every helper a generated program calls is spelled by the one rule in
//! `radix_codegen_shared::runtime_api` from the canonical table, never typed
//! into an emitter. This test scans the non-test `src/**/*.rs` of the three
//! emitters that call a per-target runtime package (`radix-hir-rust`,
//! `radix-hir-ts`, `radix-hir-go`), skipping comment lines, and counts the
//! lines that carry a helper-shaped spelling:
//!
//! - **Legacy spellings** (`__faber_*`, `__Faber*`, `__faber*`, `faberX*` ...)
//!   are the per-file prelude names the namespaced runtime packages replace.
//!   They are budgeted per crate. A budget is an exact count and only ever goes
//!   down: a unit that moves helpers into a runtime package lowers the budget
//!   in the same commit, and each target's last unit sets it to zero.
//! - **New-shape spellings** (`faber::<ns>::`, `rt.<Ns>X`, a quoted
//!   `"<ns>.x`) derive from [`Namespace::ALL`]. Their only allowed home is
//!   `runtime_api.rs`, which lives outside the scanned crates, so the budget is
//!   meant to be zero everywhere. The one exception recorded below was
//!   already hand-spelled when this ratchet was added; the `sermo` namespace
//!   unit removes it.
//!
//! Not scanned, by design: test files and comments; `radix-hir-swift` and
//! `radix-hir-python` (their helpers stay emitted); the MIR and LLVM crates
//! (`__faber_rt_v1_*` is an ABI, not a helper).
//!
//! If a count is above its budget, an emitter hand-spelled a helper: route the
//! spelling through `runtime_api` instead. If it is below, lower the budget.

use hygiene_ratchet::{ScanConfig, collect_production_files};
use radix_codegen_shared::runtime_api::Namespace;
use std::path::{Path, PathBuf};

/// What must follow a literal for it to count as a match.
#[derive(Clone, Copy)]
enum Next {
    /// Nothing is required.
    Any,
    /// An ASCII uppercase letter.
    Upper,
    /// An ASCII lowercase letter.
    Lower,
    /// One or more of `[a-z0-9_]`.
    Snake,
}

/// One helper-shaped spelling: a literal, optionally at a word boundary, then
/// a constraint on the next character.
struct Pattern {
    literal: String,
    word_boundary_before: bool,
    next: Next,
}

impl Pattern {
    fn new(literal: impl Into<String>, word_boundary_before: bool, next: Next) -> Self {
        Self {
            literal: literal.into(),
            word_boundary_before,
            next,
        }
    }

    fn matches(&self, line: &str) -> bool {
        line.match_indices(&self.literal).any(|(start, _)| {
            let before_ok = !self.word_boundary_before
                || line[..start]
                    .chars()
                    .next_back()
                    .is_none_or(|c| !is_word_char(c));
            let after = &line[start + self.literal.len()..];
            before_ok && next_ok(self.next, after)
        })
    }
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn next_ok(next: Next, after: &str) -> bool {
    let first = after.chars().next();
    match next {
        Next::Any => true,
        Next::Upper => first.is_some_and(|c| c.is_ascii_uppercase()),
        Next::Lower => first.is_some_and(|c| c.is_ascii_lowercase()),
        Next::Snake => {
            first.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        }
    }
}

/// A set of spellings; a line matching any one counts once.
struct Patterns(Vec<Pattern>);

impl Patterns {
    /// The number of non-comment lines of `content` that match any pattern.
    fn count(&self, content: &str) -> usize {
        content
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .filter(|line| self.0.iter().any(|pattern| pattern.matches(line)))
            .count()
    }
}

/// `__faber_[a-z0-9_]+ | __Faber[A-Z]... | __faber[A-Z]... |
/// \bfaber(X|Big|Dec|Fmt|Tensor|Vector|Sparsa|Intervallum|Tuple|Compare|Order|
/// ParseInstans|ValorStruct)...`
fn legacy_patterns() -> Patterns {
    const SUFFIXES: [&str; 13] = [
        "X",
        "Big",
        "Dec",
        "Fmt",
        "Tensor",
        "Vector",
        "Sparsa",
        "Intervallum",
        "Tuple",
        "Compare",
        "Order",
        "ParseInstans",
        "ValorStruct",
    ];
    let mut patterns = vec![
        Pattern::new("__faber_", false, Next::Snake),
        Pattern::new("__Faber", false, Next::Upper),
        Pattern::new("__faber", false, Next::Upper),
    ];
    patterns.extend(
        SUFFIXES
            .iter()
            .map(|suffix| Pattern::new(format!("faber{suffix}"), true, Next::Any)),
    );
    Patterns(patterns)
}

/// `faber::<ns>::` for every namespace.
fn rust_new_shape() -> Patterns {
    Patterns(
        Namespace::ALL
            .iter()
            .map(|ns| Pattern::new(format!("faber::{}::", ns.as_str()), false, Next::Any))
            .collect(),
    )
}

/// `\brt\.<Ns>[A-Z]` for every namespace.
fn go_new_shape() -> Patterns {
    Patterns(
        Namespace::ALL
            .iter()
            .map(|ns| {
                Pattern::new(
                    format!("rt.{}", upper_first(ns.as_str())),
                    true,
                    Next::Upper,
                )
            })
            .collect(),
    )
}

/// A quoted string beginning `<ns>.<lower>` for every namespace.
fn ts_new_shape() -> Patterns {
    Patterns(
        Namespace::ALL
            .iter()
            .map(|ns| Pattern::new(format!("\"{}.", ns.as_str()), false, Next::Lower))
            .collect(),
    )
}

/// Namespaces are one word, so the Go `UpperCamel` is the capitalised name.
fn upper_first(name: &str) -> String {
    let mut chars = name.chars();
    chars
        .next()
        .map(|head| head.to_ascii_uppercase().to_string() + chars.as_str())
        .unwrap_or_default()
}

/// Exact-count budget check: above is a hand-spelled helper, below is a budget
/// that must come down.
fn check_budget(label: &str, count: usize, budget: usize) -> Result<(), String> {
    if count > budget {
        Err(format!(
            "{label}: {count} helper-spelling lines exceed the budget {budget}; spell the \
             helper through runtime_api instead of typing it"
        ))
    } else if count < budget {
        Err(format!(
            "{label}: {count} helper-spelling lines are under the budget {budget}; \
             lower the budget to {count}"
        ))
    } else {
        Ok(())
    }
}

/// One scanned emitter crate and its budgets.
struct Emitter {
    name: &'static str,
    legacy_budget: usize,
    new_shape_budget: usize,
    new_shape: fn() -> Patterns,
}

// Measured at the lane base of T1-L with the check command in
// `docs/factory/codegen-readability/delivery.md` (section T1-L). Budgets only
// go down; a lane that changes emitter lines re-measures.
const EMITTERS: [Emitter; 3] = [
    Emitter {
        name: "radix-hir-rust",
        legacy_budget: 254,
        new_shape_budget: 0,
        new_shape: rust_new_shape,
    },
    Emitter {
        name: "radix-hir-ts",
        legacy_budget: 238,
        // WHY: `sermo.open(` (write_ad_expr) and the `sermo.close accepts no
        // arguments` message in expr/access.rs were hand-spelled before this
        // ratchet existed. The sermo namespace unit removes both.
        new_shape_budget: 2,
        new_shape: ts_new_shape,
    },
    Emitter {
        name: "radix-hir-go",
        legacy_budget: 610,
        new_shape_budget: 0,
        new_shape: go_new_shape,
    },
];

fn crate_src(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/ above radix-codegen-shared")
        .join(name)
        .join("src")
}

fn scan(name: &str, patterns: &Patterns) -> usize {
    let files = collect_production_files(&ScanConfig {
        source_roots: vec![crate_src(name)],
        exclude_path_suffixes: Vec::new(),
        subtract_self_expect: false,
        check_companion_convention: false,
    });
    assert!(
        !files.is_empty(),
        "{name}: no production sources found; an unreadable crate must not read as clean"
    );
    files.iter().map(|file| patterns.count(&file.content)).sum()
}

#[test]
fn legacy_helper_spellings_stay_within_budget() {
    let patterns = legacy_patterns();
    let failures: Vec<String> = EMITTERS
        .iter()
        .filter_map(|emitter| {
            check_budget(
                &format!("{} legacy", emitter.name),
                scan(emitter.name, &patterns),
                emitter.legacy_budget,
            )
            .err()
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn new_shape_helper_spellings_stay_within_budget() {
    let failures: Vec<String> = EMITTERS
        .iter()
        .filter_map(|emitter| {
            check_budget(
                &format!("{} new-shape", emitter.name),
                scan(emitter.name, &(emitter.new_shape)()),
                emitter.new_shape_budget,
            )
            .err()
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const SYNTHETIC: &str = r#"
// __faber_comment_only is skipped
    /// __faberHelper in a doc comment is skipped
fn emit(w: &mut W) {
    w.write("__faber_checked_mul(");
    w.write("__FaberCursor");
    w.write("__faberBlockOn");
    w.write("faberBigAdd(");
    w.write("faber::exact::store::<i64>");
    w.write("rt.ExactStore(");
    w.write("\"exact.store(\"");
    w.write("myfaberX(");
    w.write("rt.exactStore(");
    w.write("myrt.ExactStore(");
    w.write("faber::exactness::x");
    w.write("__faber_");
}
"#;

#[test]
fn scanner_counts_synthetic_source_and_a_lower_budget_fails() {
    let legacy = legacy_patterns();
    assert_eq!(legacy.count(SYNTHETIC), 4);
    assert!(check_budget("synthetic", 4, 4).is_ok());
    assert!(check_budget("synthetic", 4, 3).is_err());
    assert!(check_budget("synthetic", 4, 5).is_err());

    assert_eq!(rust_new_shape().count(SYNTHETIC), 1);
    assert_eq!(go_new_shape().count(SYNTHETIC), 1);
    assert_eq!(ts_new_shape().count(SYNTHETIC), 1);
}
