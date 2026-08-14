use crate::frame::{
    self, Cancellation, DispatchError, FrameStatus, HostDispatch, ResponseSender, Scrinium,
    SermoRequest,
};
use crate::Valor;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::task::{Context, Poll, Wake, Waker};

#[derive(Default)]
struct CountingWake {
    count: AtomicUsize,
}

impl Wake for CountingWake {
    fn wake(self: Arc<Self>) {
        self.count.fetch_add(1, Ordering::SeqCst);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.count.fetch_add(1, Ordering::SeqCst);
    }
}

fn test_waker(wake: &Arc<CountingWake>) -> Waker {
    Waker::from(wake.clone())
}

fn block_on<F: Future>(future: F) -> F::Output {
    let wake = Arc::new(CountingWake::default());
    let waker = test_waker(&wake);
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match Future::poll(Pin::as_mut(&mut future), &mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::yield_now(),
        }
    }
}

#[test]
fn runtime_echo_returns_opener_then_done() {
    let mut sermo = inline_sermo("runtime:echo");
    frame::sermo_set_opener(&mut sermo, Valor::Textus("salve".into()));

    let item = frame::sermo_recv(&mut sermo).expect("echo item frame");
    assert_eq!(item.status, FrameStatus::Item);
    assert_eq!(
        item.parent_id.as_deref(),
        Some(sermo.conversation_id().as_str())
    );
    assert_eq!(item.call, "runtime:echo");
    assert_eq!(item.data, Valor::Textus("salve".into()));
    assert_eq!(item.from.as_deref(), Some("faber-runtime"));

    let done = frame::sermo_recv(&mut sermo).expect("echo terminal frame");
    assert_eq!(done.status, FrameStatus::Done);
    assert!(sermo.incoming_drained());
    assert!(frame::sermo_recv(&mut sermo).is_none());
}

#[test]
fn runtime_echo_builtin_covers_hostless_dispatch() {
    // S1-U3 stabilization: the builtin `runtime:echo` route works with no host
    // dispatch installed (the bare-binary e2e product path without host=native).
    let mut sermo = frame::sermo_open("runtime:echo");
    frame::sermo_set_opener(&mut sermo, Valor::Textus("salve".into()));

    let item = frame::sermo_recv(&mut sermo).expect("builtin echo item frame");
    assert_eq!(item.status, FrameStatus::Item);
    assert_eq!(item.data, Valor::Textus("salve".into()));

    let done = frame::sermo_recv(&mut sermo).expect("builtin echo terminal frame");
    assert_eq!(done.status, FrameStatus::Done);
    assert!(sermo.incoming_drained());
    assert!(frame::sermo_recv(&mut sermo).is_none());
}

#[test]
fn runtime_echo_falls_back_to_builtin_when_host_rejects() {
    // An installed host that does not manifest `runtime:echo` must not shadow
    // the builtin route (native-host fallback ordering, dual-backend contract).
    let mut sermo = frame::sermo_open_with_dispatch("runtime:echo", Arc::new(RejectingDispatch));
    frame::sermo_set_opener(&mut sermo, Valor::Textus("salve".into()));

    let item = frame::sermo_recv(&mut sermo).expect("fallback echo item frame");
    assert_eq!(item.status, FrameStatus::Item);
    assert_eq!(item.data, Valor::Textus("salve".into()));

    let done = frame::sermo_recv(&mut sermo).expect("fallback echo terminal frame");
    assert_eq!(done.status, FrameStatus::Done);
    assert!(sermo.incoming_drained());
    assert!(frame::sermo_recv(&mut sermo).is_none());
}

#[test]
fn non_builtin_route_rejected_by_host_does_not_fall_back() {
    // The builtin fallback covers only builtin-classified routes; an unrelated
    // route rejected by the installed host still surfaces the host error.
    let mut sermo = frame::sermo_open_with_dispatch("solum:lege", Arc::new(RejectingDispatch));
    frame::sermo_set_opener(&mut sermo, Valor::Textus("data.txt".into()));

    let frame = frame::sermo_recv(&mut sermo).expect("rejection terminal");
    assert_eq!(frame.status, FrameStatus::Error);
    assert_eq!(frame.call, "solum:lege");
    assert!(
        matches!(frame.data, Valor::Textus(message) if message.contains("unsupported native host route"))
    );
    assert!(sermo.incoming_drained());
}

struct InlineDispatch;

impl HostDispatch for InlineDispatch {
    fn start(
        &self,
        request: SermoRequest,
        responses: ResponseSender,
        _cancellation: Cancellation,
    ) -> Result<(), DispatchError> {
        std::thread::spawn(move || {
            responses.item(request.opener).expect("inline item");
            responses.done().expect("inline done");
        });
        Ok(())
    }
}

fn inline_sermo(route: &str) -> frame::Sermo {
    frame::sermo_open_with_dispatch(route, Arc::new(InlineDispatch))
}

struct DelayedDispatch;

impl HostDispatch for DelayedDispatch {
    fn start(
        &self,
        request: SermoRequest,
        responses: ResponseSender,
        _cancellation: Cancellation,
    ) -> Result<(), DispatchError> {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(75));
            let _ = responses.item(request.opener);
            let _ = responses.done();
        });
        Ok(())
    }
}

struct PanicDispatch;

impl HostDispatch for PanicDispatch {
    fn start(
        &self,
        _request: SermoRequest,
        _responses: ResponseSender,
        _cancellation: Cancellation,
    ) -> Result<(), DispatchError> {
        panic!("preloaded incoming frames must not start host dispatch")
    }
}

struct RejectingDispatch;

impl HostDispatch for RejectingDispatch {
    fn start(
        &self,
        request: SermoRequest,
        _responses: ResponseSender,
        _cancellation: Cancellation,
    ) -> Result<(), DispatchError> {
        Err(DispatchError::new(
            "host_unsupported_route",
            format!("unsupported native host route `{}`", request.route),
        ))
    }
}

#[test]
fn explicit_dispatcher_does_not_use_global_installation() {
    let mut sermo = frame::sermo_open_with_dispatch("custom:echo", Arc::new(InlineDispatch));
    frame::sermo_set_opener(&mut sermo, Valor::Textus("isolated".into()));

    let item = frame::sermo_recv(&mut sermo).expect("explicit item");
    assert_eq!(item.status, FrameStatus::Item);
    assert_eq!(item.data, Valor::Textus("isolated".into()));
    assert_eq!(
        frame::sermo_recv(&mut sermo).expect("explicit done").status,
        FrameStatus::Done
    );
}

#[test]
fn rejecting_host_sends_terminal_error_outside_the_sermo_lock() {
    // A non-builtin route rejected by the installed host: the terminal Error
    // response must be delivered from outside the sermo lock instead of
    // deadlocking (the receive path holds the lock while starting dispatch).
    let mut sermo = frame::sermo_open_with_dispatch("missing:route", Arc::new(RejectingDispatch));
    frame::sermo_set_opener(&mut sermo, Valor::Nihil);

    let frame = frame::sermo_recv(&mut sermo).expect("rejection terminal");
    assert_eq!(frame.status, FrameStatus::Error);
    assert_eq!(frame.call, "missing:route");
    assert!(
        matches!(frame.data, Valor::Textus(message) if message.contains("unsupported native host route"))
    );
    assert!(sermo.incoming_drained());
}

#[test]
fn sermo_recv_async_registers_runtime_neutral_wake() {
    let (mut sermo, _sender, _cancellation) = frame::test_response_sender("test:manual-wake");
    let wake = Arc::new(CountingWake::default());
    let waker = test_waker(&wake);
    let mut cx = Context::from_waker(&waker);
    {
        let mut future = Box::pin(frame::sermo_recv_async(&mut sermo));
        assert!(matches!(
            Future::poll(Pin::as_mut(&mut future), &mut cx),
            Poll::Pending
        ));
    }

    sermo.push_incoming(Scrinium {
        id: "manual".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:manual-wake".into(),
        status: FrameStatus::Item,
        data: Valor::Textus("awakened".into()),
        created_ms: 0,
        from: Some("test".into()),
        trace: None,
    });
    assert_eq!(wake.count.load(Ordering::SeqCst), 1);

    let frame = block_on(frame::sermo_recv_async(&mut sermo)).expect("manual frame");
    assert_eq!(frame.data, Valor::Textus("awakened".into()));
}

#[test]
fn dropping_pending_async_receive_cancels_runtime_response() {
    let mut sermo = frame::sermo_open_with_dispatch("test:delayed", Arc::new(DelayedDispatch));
    frame::sermo_set_opener(&mut sermo, Valor::Numerus(25));
    let wake = Arc::new(CountingWake::default());
    let waker = test_waker(&wake);
    let mut cx = Context::from_waker(&waker);

    {
        let mut future = Box::pin(frame::sermo_recv_async(&mut sermo));
        assert!(matches!(
            Future::poll(Pin::as_mut(&mut future), &mut cx),
            Poll::Pending
        ));
    }

    let terminal = frame::sermo_recv(&mut sermo).expect("cancel terminal");
    assert_eq!(terminal.status, FrameStatus::Cancel);
    assert!(sermo.incoming_drained());
}

#[test]
fn unsupported_route_resolves_to_error_terminal() {
    let mut sermo = frame::sermo_open("missing:route");
    frame::sermo_set_opener(&mut sermo, Valor::Nihil);

    let frame = frame::sermo_recv(&mut sermo).expect("unsupported route terminal");

    assert_eq!(frame.status, FrameStatus::Error);
    assert_eq!(frame.call, "missing:route");
    assert!(
        matches!(frame.data, Valor::Textus(message) if message.contains("no host dispatch installed"))
    );
}

#[test]
fn response_sender_enforces_one_terminal_frame() {
    let (_sermo, sender, _cancellation) = frame::test_response_sender("test:sender-terminal");

    sender.done().expect("first terminal succeeds");
    let err = sender
        .error("late error")
        .expect_err("second terminal must fail");
    assert_eq!(err.issue, "frame_response_terminal_already_sent");
    let err = sender
        .item(Valor::Textus("late".into()))
        .expect_err("content after terminal must fail");
    assert_eq!(err.issue, "frame_response_after_terminal");
}

#[test]
fn response_sender_keeps_terminal_last_across_concurrent_clones() {
    for _ in 0..200 {
        let (mut sermo, sender, _cancellation) =
            frame::test_response_sender("test:sender-concurrent-terminal");
        let content_sender = sender.clone();
        let barrier = Arc::new(Barrier::new(3));
        let content_barrier = Arc::clone(&barrier);
        let content = std::thread::spawn(move || {
            content_barrier.wait();
            content_sender.item(Valor::Textus("item".into()))
        });
        let terminal_barrier = Arc::clone(&barrier);
        let terminal = std::thread::spawn(move || {
            terminal_barrier.wait();
            sender.done()
        });

        barrier.wait();
        let _ = content.join().expect("content producer");
        let _ = terminal.join().expect("terminal producer");

        let mut statuses = Vec::new();
        while let Some(frame) = frame::sermo_recv(&mut sermo) {
            statuses.push(frame.status);
            if frame.status.is_terminal() {
                break;
            }
        }
        assert!(statuses.last().is_some_and(|status| status.is_terminal()));
    }
}

#[test]
fn dropped_last_response_sender_enqueues_producer_dropped_error() {
    let (mut sermo, sender, _cancellation) = frame::test_response_sender("test:producer-drop");

    drop(sender);
    let frame = frame::sermo_recv(&mut sermo).expect("producer drop terminal");

    assert_eq!(frame.status, FrameStatus::Error);
    assert!(matches!(frame.data, Valor::Textus(message) if message.contains("producer dropped")));
}

#[test]
fn response_sender_suppresses_content_after_cancellation() {
    let (_sermo, sender, cancellation) = frame::test_response_sender("test:cancelled-response");

    cancellation.cancel();
    let err = sender
        .item(Valor::Textus("late".into()))
        .expect_err("content after cancellation must fail");
    assert_eq!(err.issue, "frame_response_cancelled");
    sender.cancel().expect("cancel terminal still succeeds");
}

#[test]
fn async_receive_poll_does_not_run_dispatch_synchronously() {
    let mut sermo = frame::sermo_open_with_dispatch("test:delayed", Arc::new(DelayedDispatch));
    frame::sermo_set_opener(&mut sermo, Valor::Numerus(75));
    let wake = Arc::new(CountingWake::default());
    let waker = test_waker(&wake);
    let mut cx = Context::from_waker(&waker);
    let started = std::time::Instant::now();

    let mut future = Box::pin(frame::sermo_recv_async(&mut sermo));
    let polled = Future::poll(Pin::as_mut(&mut future), &mut cx);

    assert!(matches!(polled, Poll::Pending));
    assert!(
        started.elapsed() < std::time::Duration::from_millis(25),
        "pending async receive poll must not run the timer route synchronously"
    );
}

// ---- `sermo ↦ T` materializers ----

#[test]
fn sermo_materialize_vacuum_drains_to_terminal() {
    let mut sermo = inline_sermo("runtime:echo");
    frame::sermo_set_opener(&mut sermo, Valor::Textus("salve".into()));
    assert!(!sermo.incoming_drained());
    frame::sermo_materialize_vacuum(&mut sermo);
    assert!(sermo.incoming_drained());
}

#[test]
fn sermo_materialize_textus_concatenates_string_frames() {
    let mut sermo = inline_sermo("runtime:echo");
    frame::sermo_set_opener(&mut sermo, Valor::Textus("salve, munde".into()));
    let out = frame::sermo_materialize_textus(&mut sermo);
    assert_eq!(out, "salve, munde");
}

#[test]
fn try_sermo_materialize_textus_rejects_non_text_frames() {
    let mut sermo = frame::sermo_open("test:skip-frames");
    sermo.push_incoming(Scrinium {
        id: "t1".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:skip-frames".into(),
        status: FrameStatus::Item,
        data: Valor::Textus("alpha".into()),
        created_ms: 0,
        from: None,
        trace: None,
    });
    sermo.push_incoming(Scrinium {
        id: "n1".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:skip-frames".into(),
        status: FrameStatus::Item,
        data: Valor::Numerus(42),
        created_ms: 0,
        from: None,
        trace: None,
    });
    sermo.push_incoming(Scrinium {
        id: "done".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:skip-frames".into(),
        status: FrameStatus::Done,
        data: Valor::Nihil,
        created_ms: 0,
        from: None,
        trace: None,
    });
    let err =
        frame::try_sermo_materialize_textus(&mut sermo).expect_err("non-text frame must fail");
    assert_eq!(err.issue, "frame_textus_payload_not_textus");
    assert!(sermo.incoming_drained());
}

#[test]
fn sermo_materialize_octeti_concatenates_bytes() {
    let mut sermo = frame::sermo_open("test:bytes");
    sermo.push_incoming(Scrinium {
        id: "b1".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:bytes".into(),
        status: FrameStatus::Item,
        data: Valor::Lista(vec![Valor::Numerus(1), Valor::Numerus(2)]),
        created_ms: 0,
        from: None,
        trace: None,
    });
    sermo.push_incoming(Scrinium {
        id: "b2".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:bytes".into(),
        status: FrameStatus::Item,
        data: Valor::Lista(vec![Valor::Numerus(3)]),
        created_ms: 0,
        from: None,
        trace: None,
    });
    sermo.push_incoming(Scrinium {
        id: "done".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:bytes".into(),
        status: FrameStatus::Done,
        data: Valor::Nihil,
        created_ms: 0,
        from: None,
        trace: None,
    });
    let out = frame::sermo_materialize_octeti(&mut sermo);
    assert_eq!(out, vec![1u8, 2, 3]);
}

#[test]
fn sermo_materialize_octeti_accepts_dense_byte_payload() {
    let mut sermo = frame::sermo_open("test:bytes");
    sermo.push_incoming(Scrinium {
        id: "b1".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:bytes".into(),
        status: FrameStatus::Byte,
        data: Valor::Octeti(vec![1, 2, 3, 4]),
        created_ms: 0,
        from: None,
        trace: None,
    });
    sermo.push_incoming(Scrinium {
        id: "done".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:bytes".into(),
        status: FrameStatus::Done,
        data: Valor::Nihil,
        created_ms: 0,
        from: None,
        trace: None,
    });
    let out = frame::sermo_materialize_octeti(&mut sermo);
    assert_eq!(out, vec![1u8, 2, 3, 4]);
}

#[test]
fn try_sermo_materialize_octeti_rejects_out_of_range_bytes() {
    let mut sermo = frame::sermo_open("test:bytes");
    sermo.push_incoming(Scrinium {
        id: "b1".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:bytes".into(),
        status: FrameStatus::Item,
        data: Valor::Lista(vec![Valor::Numerus(300)]),
        created_ms: 0,
        from: None,
        trace: None,
    });
    sermo.push_incoming(Scrinium {
        id: "done".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:bytes".into(),
        status: FrameStatus::Done,
        data: Valor::Nihil,
        created_ms: 0,
        from: None,
        trace: None,
    });
    let err = frame::try_sermo_materialize_octeti(&mut sermo).expect_err("invalid byte must fail");
    assert_eq!(err.issue, "frame_octeti_byte_out_of_range");
}

#[test]
fn sermo_materialize_valor_returns_first_content_frame() {
    let mut sermo = frame::sermo_open("test:multiple");
    sermo.push_incoming(Scrinium {
        id: "c1".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:multiple".into(),
        status: FrameStatus::Item,
        data: Valor::Textus("first".into()),
        created_ms: 0,
        from: None,
        trace: None,
    });
    sermo.push_incoming(Scrinium {
        id: "c2".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:multiple".into(),
        status: FrameStatus::Item,
        data: Valor::Numerus(42),
        created_ms: 0,
        from: None,
        trace: None,
    });
    sermo.push_incoming(Scrinium {
        id: "done".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:multiple".into(),
        status: FrameStatus::Done,
        data: Valor::Nihil,
        created_ms: 0,
        from: None,
        trace: None,
    });
    let out = frame::sermo_materialize_valor(&mut sermo);
    assert_eq!(out, Valor::Textus("first".into()));
}

#[test]
fn sermo_materialize_valor_returns_nihil_when_no_content() {
    let mut sermo = frame::sermo_open("test:empty");
    sermo.push_incoming(Scrinium {
        id: "done".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:empty".into(),
        status: FrameStatus::Done,
        data: Valor::Nihil,
        created_ms: 0,
        from: None,
        trace: None,
    });
    let out = frame::sermo_materialize_valor(&mut sermo);
    assert_eq!(out, Valor::Nihil);
}

#[test]
fn sermo_materialize_lista_collects_extractable_frames() {
    let mut sermo = frame::sermo_open("test:lines");
    sermo.push_incoming(Scrinium {
        id: "l1".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:lines".into(),
        status: FrameStatus::Item,
        data: Valor::Textus("one".into()),
        created_ms: 0,
        from: None,
        trace: None,
    });
    sermo.push_incoming(Scrinium {
        id: "l2".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:lines".into(),
        status: FrameStatus::Item,
        data: Valor::Textus("two".into()),
        created_ms: 0,
        from: None,
        trace: None,
    });
    sermo.push_incoming(Scrinium {
        id: "done".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:lines".into(),
        status: FrameStatus::Done,
        data: Valor::Nihil,
        created_ms: 0,
        from: None,
        trace: None,
    });
    let out: Vec<String> = frame::sermo_materialize_lista(&mut sermo);
    assert_eq!(out, vec!["one".to_string(), "two".to_string()]);
}

#[test]
fn try_sermo_materialize_lista_rejects_unextractable_frame() {
    let mut sermo = frame::sermo_open("test:lines");
    sermo.push_incoming(Scrinium {
        id: "l1".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:lines".into(),
        status: FrameStatus::Item,
        data: Valor::Numerus(1),
        created_ms: 0,
        from: None,
        trace: None,
    });
    sermo.push_incoming(Scrinium {
        id: "done".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:lines".into(),
        status: FrameStatus::Done,
        data: Valor::Nihil,
        created_ms: 0,
        from: None,
        trace: None,
    });
    let err =
        frame::try_sermo_materialize_lista::<String>(&mut sermo).expect_err("bad item must fail");
    assert_eq!(err.issue, "frame_lista_payload_element_type_mismatch");
}

#[test]
fn sermo_materialize_scalar_single_frame_succeeds() {
    let mut sermo = inline_sermo("runtime:echo");
    frame::sermo_set_opener(&mut sermo, Valor::Numerus(7));
    let out: i64 = frame::sermo_materialize_scalar(&mut sermo);
    assert_eq!(out, 7);
}

#[test]
fn externally_supplied_incoming_frames_suppress_runtime_fallback() {
    let mut sermo = frame::sermo_open_with_dispatch("test:preloaded", Arc::new(PanicDispatch));
    sermo.push_incoming(Scrinium {
        id: "host-done".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:preloaded".into(),
        status: FrameStatus::Done,
        data: Valor::Nihil,
        created_ms: 0,
        from: Some("host".into()),
        trace: None,
    });

    frame::sermo_materialize_vacuum(&mut sermo);
}

#[test]
fn try_sermo_materialize_scalar_returns_error_for_bad_payload() {
    let mut sermo = inline_sermo("runtime:echo");
    frame::sermo_set_opener(&mut sermo, Valor::Textus("not a number".into()));
    let err =
        frame::try_sermo_materialize_scalar::<i64>(&mut sermo).expect_err("bad scalar must fail");
    assert_eq!(err.issue, "frame_scalar_payload_target_type_mismatch");
}

#[test]
fn try_sermo_materialize_vacuum_fails_on_error_terminal() {
    let mut sermo = frame::sermo_open("test:error");
    sermo.push_incoming(Scrinium {
        id: "err".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:error".into(),
        status: FrameStatus::Error,
        data: Valor::Textus("boom".into()),
        created_ms: 0,
        from: None,
        trace: None,
    });
    let err =
        frame::try_sermo_materialize_vacuum(&mut sermo).expect_err("error terminal must fail");
    assert_eq!(err.issue, "frame_materialization_terminal_error");
}

#[test]
#[should_panic(expected = "no content frame")]
fn sermo_materialize_scalar_zero_content_frames_panics() {
    let mut sermo = frame::sermo_open("test:empty");
    sermo.push_incoming(Scrinium {
        id: "done".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:empty".into(),
        status: FrameStatus::Done,
        data: Valor::Nihil,
        created_ms: 0,
        from: None,
        trace: None,
    });
    let _: i64 = frame::sermo_materialize_scalar(&mut sermo);
}

#[test]
#[should_panic(expected = "more than one content frame")]
fn sermo_materialize_scalar_multiple_content_frames_panics() {
    let mut sermo = frame::sermo_open("test:many");
    sermo.push_incoming(Scrinium {
        id: "c1".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:many".into(),
        status: FrameStatus::Item,
        data: Valor::Numerus(1),
        created_ms: 0,
        from: None,
        trace: None,
    });
    sermo.push_incoming(Scrinium {
        id: "c2".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:many".into(),
        status: FrameStatus::Item,
        data: Valor::Numerus(2),
        created_ms: 0,
        from: None,
        trace: None,
    });
    sermo.push_incoming(Scrinium {
        id: "done".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:many".into(),
        status: FrameStatus::Done,
        data: Valor::Nihil,
        created_ms: 0,
        from: None,
        trace: None,
    });
    let _: i64 = frame::sermo_materialize_scalar(&mut sermo);
}

#[test]
fn vacuum_async_mirrors_sync_materializer() {
    let mut vacuum = frame::sermo_open("test:vacuum-async");
    vacuum.push_incoming(Scrinium {
        id: "done".into(),
        parent_id: Some(vacuum.conversation_id()),
        call: "test:vacuum-async".into(),
        status: FrameStatus::Done,
        data: Valor::Nihil,
        created_ms: 0,
        from: None,
        trace: None,
    });
    block_on(frame::sermo_materialize_vacuum_async(&mut vacuum));
    assert!(vacuum.incoming_drained());
}

#[test]
fn textus_async_mirrors_sync_materializer() {
    let mut textus = inline_sermo("runtime:echo");
    frame::sermo_set_opener(&mut textus, Valor::Textus("salve".into()));
    assert_eq!(
        block_on(frame::sermo_materialize_textus_async(&mut textus)),
        "salve"
    );
}

#[test]
fn octeti_async_mirrors_sync_materializer() {
    let mut octeti = frame::sermo_open("test:octeti-async");
    octeti.push_incoming(Scrinium {
        id: "bytes".into(),
        parent_id: Some(octeti.conversation_id()),
        call: "test:octeti-async".into(),
        status: FrameStatus::Byte,
        data: Valor::Octeti(vec![1, 2, 3]),
        created_ms: 0,
        from: None,
        trace: None,
    });
    octeti.push_incoming(Scrinium {
        id: "done".into(),
        parent_id: Some(octeti.conversation_id()),
        call: "test:octeti-async".into(),
        status: FrameStatus::Done,
        data: Valor::Nihil,
        created_ms: 0,
        from: None,
        trace: None,
    });
    assert_eq!(
        block_on(frame::sermo_materialize_octeti_async(&mut octeti)),
        vec![1, 2, 3]
    );
}

#[test]
fn valor_async_mirrors_sync_materializer() {
    let mut valor = inline_sermo("runtime:echo");
    frame::sermo_set_opener(&mut valor, Valor::Numerus(7));
    assert_eq!(
        block_on(frame::sermo_materialize_valor_async(&mut valor)),
        Valor::Numerus(7)
    );
}

#[test]
fn lista_async_mirrors_sync_materializer() {
    let mut lista = frame::sermo_open("test:lista-async");
    lista.push_incoming(Scrinium {
        id: "one".into(),
        parent_id: Some(lista.conversation_id()),
        call: "test:lista-async".into(),
        status: FrameStatus::Item,
        data: Valor::Textus("one".into()),
        created_ms: 0,
        from: None,
        trace: None,
    });
    lista.push_incoming(Scrinium {
        id: "done".into(),
        parent_id: Some(lista.conversation_id()),
        call: "test:lista-async".into(),
        status: FrameStatus::Done,
        data: Valor::Nihil,
        created_ms: 0,
        from: None,
        trace: None,
    });
    assert_eq!(
        block_on(frame::sermo_materialize_lista_async::<String>(&mut lista)),
        vec!["one".to_owned()]
    );
}

#[test]
fn scalar_async_mirrors_sync_materializer() {
    let mut scalar = inline_sermo("runtime:echo");
    frame::sermo_set_opener(&mut scalar, Valor::Numerus(9));
    assert_eq!(
        block_on(frame::sermo_materialize_scalar_async::<i64>(&mut scalar)),
        9
    );
}

#[test]
fn instans_async_mirrors_sync_materializer() {
    let mut instans = inline_sermo("runtime:echo");
    frame::sermo_set_opener(&mut instans, Valor::Instans("1970-01-01T00:00:00Z".into()));
    let materialized = block_on(frame::sermo_materialize_instans_async(
        &mut instans,
        crate::InstansPraecisio::Nanosecunda,
    ));
    assert_eq!(
        materialized.praecisio(),
        crate::InstansPraecisio::Nanosecunda
    );
}

// ---- Edge and sad-path tests ----

#[test]
fn try_sermo_materialize_octeti_empty_lista_returns_empty_bytes() {
    let mut sermo = frame::sermo_open("test:bytes");
    sermo.push_incoming(Scrinium {
        id: "b1".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:bytes".into(),
        status: FrameStatus::Item,
        data: Valor::Lista(vec![]),
        created_ms: 0,
        from: None,
        trace: None,
    });
    sermo.push_incoming(Scrinium {
        id: "done".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "test:bytes".into(),
        status: FrameStatus::Done,
        data: Valor::Nihil,
        created_ms: 0,
        from: None,
        trace: None,
    });
    let out = frame::sermo_materialize_octeti(&mut sermo);
    assert!(out.is_empty());
}

#[test]
fn reject_start_error_after_cancellation_records_cancel_never_error() {
    // M02: once the receive future's drop recorded cancellation, the detached
    // start-error rejection must not push an `Error` terminal — exactly one
    // terminal, and it is the recorded `Cancel`, never `Error` after `Cancel`.
    let (mut sermo, sender, cancellation) = frame::test_response_sender("test:reject-cancel");
    cancellation.cancel(); // the dropped receive future recorded cancellation
    sender.reject_start_error(DispatchError::new(
        "host_unsupported_route",
        "late rejection",
    ));
    drop(sender); // last sender: its Drop records the Cancel terminal

    let terminal = frame::sermo_recv(&mut sermo).expect("one atomic terminal");
    assert_eq!(terminal.status, FrameStatus::Cancel);
    assert!(
        frame::sermo_recv(&mut sermo).is_none(),
        "exactly one terminal per run"
    );
}

#[test]
fn reject_start_error_without_cancellation_records_error_terminal() {
    let (mut sermo, sender, _cancellation) = frame::test_response_sender("test:reject-error");
    sender.reject_start_error(DispatchError::new("host_unsupported_route", "boom"));
    drop(sender);

    let terminal = frame::sermo_recv(&mut sermo).expect("one atomic terminal");
    assert_eq!(terminal.status, FrameStatus::Error);
    assert!(
        frame::sermo_recv(&mut sermo).is_none(),
        "exactly one terminal per run"
    );
}

#[test]
fn poll_drop_rejection_observes_one_atomic_terminal() {
    // M02 stress: for each run, start a route whose host dispatch fails, drop
    // the receive future before the detached rejection lands, and observe the
    // terminal. Exactly one atomic terminal state per run — either the recorded
    // `Cancel` or the rejection `Error`, never both.
    for _ in 0..200 {
        let mut sermo =
            frame::sermo_open_with_dispatch("missing:route", Arc::new(RejectingDispatch));
        let wake = Arc::new(CountingWake::default());
        let waker = test_waker(&wake);
        let mut cx = Context::from_waker(&waker);
        let mut statuses: Vec<FrameStatus> = Vec::new();
        {
            let mut future = Box::pin(frame::sermo_recv_async(&mut sermo));
            if let Poll::Ready(Some(frame)) = Future::poll(Pin::as_mut(&mut future), &mut cx) {
                // The rejection landed before the poll returned; its terminal
                // is the observed state for this run.
                statuses.push(frame.status);
            }
            // Drop before the detached rejection lands: records cancellation.
        }
        while let Some(frame) = frame::sermo_recv(&mut sermo) {
            statuses.push(frame.status);
            if frame.status.is_terminal() {
                break;
            }
        }
        assert_eq!(
            statuses.len(),
            1,
            "exactly one atomic terminal state per run"
        );
        let terminal = statuses[0];
        assert!(
            matches!(terminal, FrameStatus::Cancel | FrameStatus::Error),
            "terminal must be the recorded Cancel or the rejection Error, got {terminal:?}"
        );
    }
}
