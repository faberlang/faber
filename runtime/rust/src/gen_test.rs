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

mod async_cursor {
    use super::super::{AsyncCursor, AsyncCursorShared, block_on};
    use std::convert::Infallible;
    use std::sync::Arc;
    use std::thread;

    /// The producer shape generated code builds: a body answering each queued
    /// request through the shared state, then `finish`.
    fn counting_cursor(n: i64) -> (AsyncCursor<i64, Infallible>, thread::JoinHandle<()>) {
        let shared = Arc::new(AsyncCursorShared::<i64, Infallible>::new());
        let producer = Arc::clone(&shared);
        let body = thread::spawn(move || {
            for i in 0..n {
                let Some(request) = producer.next_request() else {
                    return;
                };
                let _ = request.send(Ok(Some(i)));
            }
            producer.finish(Ok(()));
        });
        (AsyncCursor::new(shared), body)
    }

    #[test]
    fn async_cursor_yields_items_in_order_then_ends() {
        let (cursor, body) = counting_cursor(3);
        for expected in 0..3 {
            assert_eq!(block_on(cursor.next()), Ok(Some(expected)));
        }
        assert_eq!(block_on(cursor.next()), Ok(None));
        assert_eq!(block_on(cursor.next()), Ok(None));
        body.join().expect("producer thread");
    }

    #[test]
    fn async_cursor_surfaces_the_error_once_then_ends() {
        let shared = Arc::new(AsyncCursorShared::<i64, String>::new());
        shared.finish(Err("boom".to_owned()));
        let cursor = AsyncCursor::new(shared);
        assert_eq!(block_on(cursor.next()), Err("boom".to_owned()));
        assert_eq!(block_on(cursor.next()), Ok(None));
    }

    #[test]
    fn dropping_the_cursor_closes_the_producer() {
        let (cursor, body) = counting_cursor(100);
        assert_eq!(block_on(cursor.next()), Ok(Some(0)));
        drop(cursor);
        body.join().expect("producer thread");
    }
}

#[test]
fn block_on_returns_the_value_of_an_immediately_ready_future() {
    assert_eq!(super::block_on(async { 7_i64 }), 7);
}

#[test]
fn block_on_polls_a_pending_future_until_it_is_ready() {
    let mut polls = 0;
    let value = super::block_on(std::future::poll_fn(|_cx| {
        polls += 1;
        if polls < 3 {
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(polls)
        }
    }));
    assert_eq!(value, 3);
}

#[cfg(feature = "tokio")]
#[test]
fn block_on_tokio_runs_the_future_on_a_runtime() {
    assert_eq!(super::block_on_tokio(async { 11_i64 }), 11);
}

#[test]
fn send_or_drop_delivers_to_a_live_receiver() {
    let (sender, receiver) = std::sync::mpsc::channel::<Result<Option<i64>, String>>();
    super::send_or_drop(&sender, Ok(Some(5)));
    assert_eq!(receiver.recv(), Ok(Ok(Some(5))));
}

#[test]
fn send_or_drop_ignores_a_receiver_that_hung_up() {
    let (sender, receiver) = std::sync::mpsc::channel::<Result<Option<i64>, String>>();
    drop(receiver);
    super::send_or_drop(&sender, Ok(Some(5)));
}
