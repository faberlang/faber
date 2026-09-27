//! Three-tier conversation router (A4/U5): static Faber table → builtins →
//! installed host → fail closed, plus the §2.7 conversation record.

// Handlers match the owned `StaticRoute::start` signature: the handler task
// owns its request, sender and cancellation even when a test only reads them.
#![allow(clippy::needless_pass_by_value)]

use crate::Valor;
use crate::frame::{
    self, AnsweringTier, Cancellation, DispatchError, FrameStatus, HostDispatch, ResponseSender,
    SermoRequest, StaticRoute,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Host that answers every route with one `host` item, so a test can tell
/// whether the host tier was consulted.
struct AnsweringHost;

impl HostDispatch for AnsweringHost {
    fn start(
        &self,
        _request: SermoRequest,
        responses: ResponseSender,
        _cancellation: Cancellation,
    ) -> Result<(), DispatchError> {
        std::thread::spawn(move || {
            let _ = responses.item(Valor::Textus("host".into()));
            let _ = responses.done();
        });
        Ok(())
    }
}

fn serve_static(request: SermoRequest, responses: ResponseSender, _cancellation: Cancellation) {
    let _ = responses.item(Valor::Textus(format!("static:{:?}", request.opener)));
    let _ = responses.done();
}

fn serve_panics(_request: SermoRequest, _responses: ResponseSender, _cancellation: Cancellation) {
    panic!("handler failure under test");
}

static CURSOR_SAW_CANCEL: AtomicBool = AtomicBool::new(false);

fn serve_forever(_request: SermoRequest, responses: ResponseSender, cancellation: Cancellation) {
    let mut n = 0;
    loop {
        if cancellation.is_cancelled() {
            CURSOR_SAW_CANCEL.store(true, Ordering::SeqCst);
            return;
        }
        if responses.item(Valor::Numerus(n)).is_err() {
            CURSOR_SAW_CANCEL.store(cancellation.is_cancelled(), Ordering::SeqCst);
            return;
        }
        n += 1;
        std::thread::sleep(Duration::from_millis(1));
    }
}

static STARTED_AT_OPEN: AtomicBool = AtomicBool::new(false);

fn serve_records_start(
    _request: SermoRequest,
    responses: ResponseSender,
    _cancellation: Cancellation,
) {
    STARTED_AT_OPEN.store(true, Ordering::SeqCst);
    let _ = responses.done();
}

static ROUTES: &[StaticRoute] = &[
    StaticRoute {
        route: "solum:lege",
        takes_opener: true,
        start: serve_static,
    },
    StaticRoute {
        route: "test:static",
        takes_opener: true,
        start: serve_static,
    },
    StaticRoute {
        route: "test:panics",
        takes_opener: false,
        start: serve_panics,
    },
    StaticRoute {
        route: "test:forever",
        takes_opener: false,
        start: serve_forever,
    },
    StaticRoute {
        route: "test:eager",
        takes_opener: false,
        start: serve_records_start,
    },
];

fn first_item(sermo: &mut frame::Sermo) -> Valor {
    let frame = frame::sermo_recv(sermo).expect("content frame");
    assert_eq!(frame.status, FrameStatus::Item);
    frame.data
}

#[test]
fn static_route_beats_host_route() {
    let host: Arc<dyn HostDispatch> = Arc::new(AnsweringHost);
    let mut sermo = frame::sermo_open_with_static_routes("solum:lege", ROUTES, Some(host));
    frame::sermo_set_opener(&mut sermo, Valor::Textus("data.txt".into()));

    assert_eq!(
        first_item(&mut sermo),
        Valor::Textus("static:Textus(\"data.txt\")".into())
    );
    assert_eq!(sermo.answering_tier(), Some(AnsweringTier::Static));
    assert_eq!(
        frame::sermo_recv(&mut sermo).expect("done").status,
        FrameStatus::Done
    );
}

#[test]
fn builtin_route_beats_host_route() {
    let host: Arc<dyn HostDispatch> = Arc::new(AnsweringHost);
    let mut sermo = frame::sermo_open_with_static_routes("runtime:echo", ROUTES, Some(host));
    frame::sermo_set_opener(&mut sermo, Valor::Textus("salve".into()));

    assert_eq!(first_item(&mut sermo), Valor::Textus("salve".into()));
    assert_eq!(sermo.answering_tier(), Some(AnsweringTier::Builtin));
}

#[test]
fn host_answers_route_outside_static_and_builtin_tiers() {
    let host: Arc<dyn HostDispatch> = Arc::new(AnsweringHost);
    let mut sermo = frame::sermo_open_with_static_routes("tempus:nunc", ROUTES, Some(host));

    assert_eq!(first_item(&mut sermo), Valor::Textus("host".into()));
    assert_eq!(sermo.answering_tier(), Some(AnsweringTier::Host));
}

#[test]
fn unknown_route_without_host_fails_closed() {
    let mut sermo = frame::sermo_open_with_static_routes("ignotum:via", ROUTES, None);
    frame::sermo_set_opener(&mut sermo, Valor::Nihil);

    let frame = frame::sermo_recv(&mut sermo).expect("fail-closed terminal");
    assert_eq!(frame.status, FrameStatus::Error);
    assert_eq!(sermo.answering_tier(), Some(AnsweringTier::None));
    assert!(sermo.incoming_drained());
}

#[test]
fn handler_panic_gives_error_terminal() {
    let mut sermo = frame::sermo_open_with_static_routes("test:panics", ROUTES, None);

    let frame = frame::sermo_recv(&mut sermo).expect("panic terminal");
    assert_eq!(frame.status, FrameStatus::Error);
    assert!(matches!(frame.data, Valor::Textus(message) if message.contains("test:panics")));
    assert!(sermo.incoming_drained());
}

#[test]
fn cancel_reaches_cursor_handler() {
    let mut sermo = frame::sermo_open_with_static_routes("test:forever", ROUTES, None);
    for expected in 0..3 {
        assert_eq!(first_item(&mut sermo), Valor::Numerus(expected));
    }
    // Dropping a pending async receive is the caller-side cancel path.
    drop(frame::sermo_recv_async(&mut sermo));

    let deadline = Instant::now() + Duration::from_secs(5);
    while !CURSOR_SAW_CANCEL.load(Ordering::SeqCst) {
        assert!(
            Instant::now() < deadline,
            "cursor handler never observed cancel"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut terminal = None;
    while let Some(frame) = frame::sermo_recv(&mut sermo) {
        if frame.status.is_terminal() {
            terminal = Some(frame.status);
            break;
        }
    }
    assert_eq!(terminal, Some(FrameStatus::Cancel));
}

#[test]
fn zero_opener_static_handler_starts_at_open() {
    let sermo = frame::sermo_open_with_static_routes("test:eager", ROUTES, None);

    let deadline = Instant::now() + Duration::from_secs(5);
    while !STARTED_AT_OPEN.load(Ordering::SeqCst) {
        assert!(
            Instant::now() < deadline,
            "tier-1 handler did not start at open"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(sermo.answering_tier(), Some(AnsweringTier::Static));
}

#[test]
fn opener_static_handler_starts_when_opener_is_set() {
    let mut sermo = frame::sermo_open_with_static_routes("test:static", ROUTES, None);
    assert_eq!(sermo.answering_tier(), None);

    frame::sermo_set_opener(&mut sermo, Valor::Numerus(7));
    assert_eq!(sermo.answering_tier(), Some(AnsweringTier::Static));
    assert_eq!(
        first_item(&mut sermo),
        Valor::Textus("static:Numerus(7)".into())
    );
}

#[test]
fn release_point_drops_handler_task_when_the_handler_finishes() {
    let mut sermo = frame::sermo_open_with_static_routes("test:static", ROUTES, None);
    frame::sermo_set_opener(&mut sermo, Valor::Nihil);

    // D8.3: completion releases without the caller reading anything.
    let deadline = Instant::now() + Duration::from_secs(5);
    while sermo.handler_task_held() {
        assert!(Instant::now() < deadline, "handler task never released");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(sermo.released());
    assert_eq!(first_item(&mut sermo), Valor::Textus("static:Nihil".into()));
}

#[test]
fn installed_static_table_answers_plain_sermo_open() {
    static GLOBAL: &[StaticRoute] = &[StaticRoute {
        route: "test-global:route",
        takes_opener: true,
        start: serve_static,
    }];
    frame::install_static_routes(GLOBAL).expect("first install");
    assert!(frame::install_static_routes(GLOBAL).is_err());

    let mut sermo = frame::sermo_open("test-global:route");
    frame::sermo_set_opener(&mut sermo, Valor::Bivalens(true));
    assert_eq!(
        first_item(&mut sermo),
        Valor::Textus("static:Bivalens(true)".into())
    );
    assert_eq!(sermo.answering_tier(), Some(AnsweringTier::Static));
}
