//! A7 proof (DECISIONS D8.3): a conversation releases when it completes,
//! `sermo_close` closes early and reports an error terminal, and the last
//! caller handle cancels an abandoned conversation.

// Handlers match the owned `StaticRoute::start` signature.
#![allow(clippy::needless_pass_by_value)]

use crate::Valor;
use crate::frame::{self, Cancellation, FrameStatus, ResponseSender, SermoRequest, StaticRoute};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

fn serve_twice(request: SermoRequest, responses: ResponseSender, _cancellation: Cancellation) {
    let Valor::Numerus(n) = request.opener else {
        let _ = responses.error("opener must be numerus");
        return;
    };
    let _ = responses.item(Valor::Numerus(n * 2));
    let _ = responses.done();
}

fn serve_fails(_request: SermoRequest, responses: ResponseSender, _cancellation: Cancellation) {
    let _ = responses.error("broken");
}

static CLOSED_SAW_CANCEL: AtomicBool = AtomicBool::new(false);
static DROPPED_SAW_CANCEL: AtomicBool = AtomicBool::new(false);
static FINI_SAW_CANCEL: AtomicBool = AtomicBool::new(false);

fn produce_until_cancelled(
    responses: &ResponseSender,
    cancellation: &Cancellation,
    seen: &AtomicBool,
) {
    let mut n = 0;
    while !cancellation.is_cancelled() {
        if responses.item(Valor::Numerus(n)).is_err() {
            break;
        }
        n += 1;
        std::thread::sleep(Duration::from_millis(1));
    }
    seen.store(cancellation.is_cancelled(), Ordering::SeqCst);
}

fn serve_until_closed(
    _request: SermoRequest,
    responses: ResponseSender,
    cancellation: Cancellation,
) {
    produce_until_cancelled(&responses, &cancellation, &CLOSED_SAW_CANCEL);
}

fn serve_until_dropped(
    _request: SermoRequest,
    responses: ResponseSender,
    cancellation: Cancellation,
) {
    produce_until_cancelled(&responses, &cancellation, &DROPPED_SAW_CANCEL);
}

fn serve_until_fini(_request: SermoRequest, responses: ResponseSender, cancellation: Cancellation) {
    produce_until_cancelled(&responses, &cancellation, &FINI_SAW_CANCEL);
}

static ROUTES: &[StaticRoute] = &[
    StaticRoute {
        route: "close:twice",
        takes_opener: true,
        start: serve_twice,
    },
    StaticRoute {
        route: "close:fails",
        takes_opener: false,
        start: serve_fails,
    },
    StaticRoute {
        route: "close:until-closed",
        takes_opener: false,
        start: serve_until_closed,
    },
    StaticRoute {
        route: "close:until-dropped",
        takes_opener: false,
        start: serve_until_dropped,
    },
    StaticRoute {
        route: "close:until-fini",
        takes_opener: false,
        start: serve_until_fini,
    },
];

fn wait_for(what: &str, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done() {
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn finished_conversations_release_while_their_handles_are_held() {
    let mut pool = Vec::new();
    for n in 0..4 {
        let mut sermo = frame::sermo_open_with_static_routes("close:twice", ROUTES, None);
        frame::sermo_set_opener(&mut sermo, Valor::Numerus(n));
        pool.push(sermo);
    }
    for sermo in &pool {
        wait_for("a finished handler released", || sermo.released());
        assert!(!sermo.handler_task_held());
    }
    // The finished record still reads as its result.
    for (n, sermo) in pool.iter_mut().enumerate() {
        let frame = frame::sermo_recv(sermo).expect("item frame");
        assert_eq!(frame.data, Valor::Numerus(2 * i64::try_from(n).unwrap()));
    }
}

#[test]
fn close_cancels_a_live_handler_and_is_idempotent() {
    let sermo = frame::sermo_open_with_static_routes("close:until-closed", ROUTES, None);
    assert!(!sermo.released());
    assert_eq!(frame::sermo_close(&sermo), Ok(true));
    assert!(sermo.released());
    wait_for("closed handler observed cancel", || {
        CLOSED_SAW_CANCEL.load(Ordering::SeqCst)
    });
    assert_eq!(frame::sermo_close(&sermo), Ok(false));
}

#[test]
fn close_reports_the_error_terminal() {
    let sermo = frame::sermo_open_with_static_routes("close:fails", ROUTES, None);
    wait_for("failed handler released", || sermo.released());
    assert_eq!(frame::sermo_close(&sermo), Err("broken".to_owned()));
    assert_eq!(frame::sermo_close(&sermo), Ok(false));
}

#[test]
fn close_after_success_is_ok() {
    let mut sermo = frame::sermo_open_with_static_routes("close:twice", ROUTES, None);
    frame::sermo_set_opener(&mut sermo, Valor::Numerus(1));
    wait_for("handler released", || sermo.released());
    assert_eq!(frame::sermo_close(&sermo), Ok(true));
    assert!(
        frame::sermo_recv(&mut sermo).is_none(),
        "a closed handle reads as closed"
    );
}

#[test]
fn last_caller_handle_cancels_an_abandoned_conversation() {
    let sermo = frame::sermo_open_with_static_routes("close:until-dropped", ROUTES, None);
    let tuus = frame::sermo_tuus::<i64>(&sermo);
    drop(sermo);
    std::thread::sleep(Duration::from_millis(5));
    assert!(
        !DROPPED_SAW_CANCEL.load(Ordering::SeqCst),
        "a live view keeps the conversation open"
    );
    drop(tuus);
    wait_for("abandoned handler observed cancel", || {
        DROPPED_SAW_CANCEL.load(Ordering::SeqCst)
    });
}

#[test]
fn fini_on_a_live_handler_cancels_it() {
    let sermo = frame::sermo_open_with_static_routes("close:until-fini", ROUTES, None);
    let tuus = frame::sermo_tuus::<i64>(&sermo);
    assert_eq!(frame::tuus_fini(&tuus), FrameStatus::Cancel);
    assert!(sermo.released());
    wait_for("fini'd handler observed cancel", || {
        FINI_SAW_CANCEL.load(Ordering::SeqCst)
    });
}
