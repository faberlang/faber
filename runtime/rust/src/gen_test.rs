use super::{Gen, Yield};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// A generator over `0..n`: the shape generated code builds for `fiunt`.
fn count_to(n: i64, started: &Cell<bool>) -> Gen<'_, i64> {
    let slot = Rc::new(RefCell::new(None::<i64>));
    let yielder = Yield::new(Rc::clone(&slot));
    Gen::new(slot, async move {
        started.set(true);
        for i in 0..n {
            yielder.put(i).await;
        }
    })
}

#[test]
fn gen_yields_items_in_order_then_ends() {
    let started = Cell::new(false);
    let items: Vec<i64> = count_to(4, &started).collect();
    assert_eq!(items, vec![0, 1, 2, 3]);
}

#[test]
fn gen_is_lazy_until_pulled() {
    let started = Cell::new(false);
    let mut generator = count_to(3, &started);
    assert!(
        !started.get(),
        "the body must not run before the first pull"
    );
    assert_eq!(generator.next(), Some(0));
    assert!(started.get());
}

#[test]
fn gen_stays_ended_after_the_body_finishes() {
    let started = Cell::new(false);
    let mut generator = count_to(1, &started);
    assert_eq!(generator.next(), Some(0));
    assert_eq!(generator.next(), None);
    assert_eq!(generator.next(), None);
}

#[test]
fn empty_gen_yields_nothing() {
    let started = Cell::new(false);
    assert_eq!(count_to(0, &started).count(), 0);
}

#[test]
fn gen_debug_drains_the_remaining_items() {
    let started = Cell::new(false);
    let mut generator = count_to(3, &started);
    assert_eq!(generator.next(), Some(0));
    assert_eq!(format!("{generator:?}"), "[1, 2]");
    assert_eq!(generator.next(), None);
}
