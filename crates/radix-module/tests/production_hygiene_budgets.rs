//! Carrier target for the radix-module production hygiene budgets.
//!
//! The checks themselves live in `hygiene.rs` (target `hygiene`); this target
//! exposes the same test binary under the budget test's own name so
//! `cargo test --test production_hygiene_budgets` runs it.

#[path = "hygiene.rs"]
mod hygiene;
