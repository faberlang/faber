use super::*;
use std::time::{Duration, Instant};

#[test]
fn two_slow_handlers_overlap_and_update_state_deterministically() {
    let state = ApplicationState::new();
    let workers = HandlerWorkers::new(2, state.clone()).expect("workers");
    let delay = Duration::from_millis(120);
    let started = Instant::now();

    let first = workers.spawn("r1", move |state| async move {
        tokio::time::sleep(delay).await;
        Valor::Numerus(state.increment("requests").expect("increment"))
    });
    let second = workers.spawn("r2", move |state| async move {
        tokio::time::sleep(delay).await;
        Valor::Numerus(state.increment("requests").expect("increment"))
    });

    let (first, second) = workers
        .runtime
        .block_on(async { tokio::join!(first.complete(), second.complete()) });

    assert!(matches!(
        first,
        ResponseCompletion::Completed(Valor::Numerus(1 | 2))
    ));
    assert!(matches!(
        second,
        ResponseCompletion::Completed(Valor::Numerus(1 | 2))
    ));
    assert_ne!(first, second);
    assert_eq!(state.get("requests"), Ok(Some(Valor::Numerus(2))));
    assert!(
        started.elapsed() < delay * 2 - Duration::from_millis(20),
        "slow handlers did not overlap"
    );
}

#[test]
fn cancellation_suppresses_late_completion() {
    let workers = HandlerWorkers::new(1, ApplicationState::new()).expect("workers");
    let ticket = workers.spawn("cancelled", |_state| async move {
        tokio::time::sleep(Duration::from_millis(40)).await;
        Valor::Textus("late".into())
    });
    ticket.cancel();

    let completion = workers.runtime.block_on(ticket.complete());
    assert_eq!(completion, ResponseCompletion::Cancelled);
}

#[test]
fn non_numeric_state_increment_fails_closed() {
    let state = ApplicationState::new();
    state
        .set("requests", Valor::Textus("wrong".into()))
        .expect("set");
    assert_eq!(
        state.increment("requests"),
        Err(StateError::NotNumeric("requests".into()))
    );
}
