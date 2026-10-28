//! Test-only observer for interval flow facts. Every read of a
//! named binding records the interval the checker holds for it at that point,
//! so the flow tests can see what the producers established and the engine
//! kept. Compiled out of every other build.

#![cfg(test)]

use std::cell::RefCell;

use super::flow_range::IntRange;
use super::*;

thread_local! {
    static READS: RefCell<Vec<(String, Option<IntRange>)>> = const { RefCell::new(Vec::new()) };
}

pub(super) fn clear_reads() {
    READS.with(|reads| reads.borrow_mut().clear());
}

/// The flow fact held at each read of `name`, in source order.
pub(super) fn reads_of(name: &str) -> Vec<Option<IntRange>> {
    READS.with(|reads| {
        reads
            .borrow()
            .iter()
            .filter(|(read, _)| read == name)
            .map(|(_, range)| range.clone())
            .collect()
    })
}

pub(super) fn observe(checker: &TypeChecker<'_>, def_id: DefId) {
    // Loop binders are named by the resolver, locals and parameters by the
    // checker's own record.
    let name = checker.binding_names.get(&def_id).copied().or_else(|| {
        checker
            .resolver
            .get_symbol(def_id)
            .map(|symbol| symbol.name)
    });
    let (Some(interner), Some(name)) = (checker.interner, name) else {
        return;
    };
    let range = checker.range_flow.get(def_id).cloned();
    READS.with(|reads| {
        reads
            .borrow_mut()
            .push((interner.resolve(name).to_owned(), range));
    });
}
