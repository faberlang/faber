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

/// Unique per-test temp directory. Drop cleans up (including on panic).
fn test_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("create temp dir")
}

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
    let mut sermo = frame::sermo_open("runtime:echo");
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
fn rejecting_host_falls_back_to_builtin_dispatch_for_builtin_route() {
    // A host that does not manifest a builtin-covered route must not shadow the
    // builtin delivery (dual-backend contract; e.g. `processus:exi` in
    // host=native product builds). Before the dispatch fallback this deadlocked.
    let mut sermo = frame::sermo_open_with_dispatch("tempus:nunc", Arc::new(RejectingDispatch));
    frame::sermo_set_opener(&mut sermo, Valor::Nihil);

    let item = frame::sermo_recv(&mut sermo).expect("builtin fallback item");
    assert_eq!(item.status, FrameStatus::Item);
    assert!(matches!(item.data, Valor::Instans(_)));
    let done = frame::sermo_recv(&mut sermo).expect("builtin fallback done");
    assert_eq!(done.status, FrameStatus::Done);
    assert!(sermo.incoming_drained());
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
    let mut sermo = frame::sermo_open("tempus:dormiet");
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
        matches!(frame.data, Valor::Textus(message) if message.contains("unsupported ad route"))
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
fn async_receive_poll_does_not_sleep_for_timer_route() {
    let mut sermo = frame::sermo_open("tempus:dormiet");
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
    let mut sermo = frame::sermo_open("runtime:echo");
    frame::sermo_set_opener(&mut sermo, Valor::Textus("salve".into()));
    assert!(!sermo.incoming_drained());
    frame::sermo_materialize_vacuum(&mut sermo);
    assert!(sermo.incoming_drained());
}

#[test]
fn sermo_materialize_textus_concatenates_string_frames() {
    let mut sermo = frame::sermo_open("runtime:echo");
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
    let mut sermo = frame::sermo_open("runtime:echo");
    frame::sermo_set_opener(&mut sermo, Valor::Numerus(7));
    let out: i64 = frame::sermo_materialize_scalar(&mut sermo);
    assert_eq!(out, 7);
}

#[test]
fn tempus_nunc_route_materializes_instans() {
    let mut sermo = frame::sermo_open("tempus:nunc");
    let out = frame::sermo_materialize_instans(&mut sermo, crate::InstansPraecisio::Nanosecunda);

    assert_eq!(out.praecisio(), crate::InstansPraecisio::Nanosecunda);
}

#[test]
fn tempus_activum_route_returns_nanoseconds() {
    let request = SermoRequest {
        conversation_id: "tempus-activum".into(),
        route: "tempus:activum".into(),
        opener: Valor::Nihil,
        target: None,
    };
    let first = frame::builtin_route_frames(request.clone());
    std::thread::sleep(std::time::Duration::from_millis(5));
    let second = frame::builtin_route_frames(request);

    let [(FrameStatus::Item, Valor::Numerus(first)), (FrameStatus::Done, Valor::Nihil)] =
        first.as_slice()
    else {
        panic!("tempus:activum must return one numeric item and done");
    };
    let [(FrameStatus::Item, Valor::Numerus(second)), (FrameStatus::Done, Valor::Nihil)] =
        second.as_slice()
    else {
        panic!("tempus:activum must return one numeric item and done");
    };
    assert!(*second > *first);
    assert!(
        second.saturating_sub(*first) >= 1_000_000,
        "active time must be nanoseconds: {first} -> {second}"
    );
}

#[test]
fn tempus_dormiet_rejects_invalid_duration() {
    for opener in [Valor::Textus("slow".into()), Valor::Numerus(-1)] {
        let mut sermo = frame::sermo_open("tempus:dormiet");
        frame::sermo_set_opener(&mut sermo, opener);
        let error = frame::try_sermo_materialize_vacuum(&mut sermo)
            .expect_err("invalid sleep duration must fail");
        assert!(error.to_string().contains("tempus:dormiet"));
        assert!(error.to_string().contains("must be"));
    }
}

fn solum_crea_fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = test_dir();
    let file = dir.path().join("payload.bin");
    (dir, file)
}

#[test]
fn solum_crea_creates_directory() {
    let (dir, _file) = solum_crea_fixture();
    let dir_s = dir.path().to_string_lossy().into_owned();

    let mut crea = frame::sermo_open("solum:crea");
    frame::sermo_set_opener(&mut crea, Valor::Textus(dir_s));
    frame::sermo_materialize_vacuum(&mut crea);
    assert!(dir.path().is_dir(), "crea must create directory");
}

#[test]
fn solum_hauri_reads_file_bytes() {
    let (dir, file) = solum_crea_fixture();
    std::fs::create_dir_all(dir.path()).expect("create dir");
    std::fs::write(&file, [9u8, 8, 7]).expect("write fixture");
    let file_s = file.to_string_lossy().into_owned();

    let mut hauri = frame::sermo_open("solum:hauri");
    frame::sermo_set_opener(&mut hauri, Valor::Textus(file_s));
    let bytes: Vec<u8> = frame::sermo_materialize_octeti(&mut hauri);
    assert_eq!(bytes, vec![9, 8, 7]);
}

#[test]
fn solum_enumera_lists_directory_contents() {
    let (dir, file) = solum_crea_fixture();
    std::fs::create_dir_all(dir.path()).expect("create dir");
    std::fs::write(&file, [9u8, 8, 7]).expect("write fixture");
    let dir_s = dir.path().to_string_lossy().into_owned();

    let mut enumera = frame::sermo_open("solum:enumera");
    frame::sermo_set_opener(&mut enumera, Valor::Textus(dir_s));
    let names: Vec<String> = frame::sermo_materialize_lista(&mut enumera);
    assert_eq!(names, vec!["payload.bin".to_owned()]);
}

#[test]
fn solum_temporarium_route_materializes_temp_dir_textus() {
    let mut sermo = frame::sermo_open("solum:temporarium");
    let path: String = frame::sermo_materialize_textus(&mut sermo);
    assert!(!path.is_empty(), "temporarium must return a non-empty path");
    assert!(
        std::path::Path::new(&path).is_dir(),
        "temporarium path must be an existing directory: {path}"
    );
    let expected = std::env::temp_dir().to_string_lossy().into_owned();
    assert_eq!(path, expected);
}

#[test]
fn solum_tange_existing_socket_returns_error_instead_of_done() {
    use std::os::unix::net::UnixListener;

    let dir = test_dir();
    let path = dir.path().join("tange.socket");
    let listener = UnixListener::bind(&path).expect("bind socket fixture");
    let path_s = path.to_string_lossy().into_owned();

    let mut sermo = frame::sermo_open("solum:tange");
    frame::sermo_set_opener(&mut sermo, Valor::Textus(path_s.clone()));
    let error = frame::try_sermo_materialize_vacuum(&mut sermo)
        .expect_err("touching an unopenable existing path must fail");
    assert!(error.to_string().contains("solum:tange"));
    assert!(error.to_string().contains(&path_s));

    drop(listener);
}

#[test]
fn solum_carpe_route_materializes_line_lista() {
    let dir = test_dir();
    let path = dir.path().join("carpe.txt");
    std::fs::write(&path, "alpha\nbeta\ngamma\n").expect("write carpe fixture");
    let path_s = path.to_string_lossy().into_owned();

    let mut sermo = frame::sermo_open("solum:carpe");
    frame::sermo_set_opener(&mut sermo, Valor::Textus(path_s));
    let lines: Vec<String> = frame::sermo_materialize_lista(&mut sermo);
    assert_eq!(
        lines,
        vec!["alpha".to_owned(), "beta".to_owned(), "gamma".to_owned()]
    );
}

#[test]
fn solum_lege_text_as_scalar() {
    let dir = test_dir();
    let text_path = dir.path().join("lege.txt");
    std::fs::write(&text_path, "prima\nsecunda\n").expect("write text fixture");
    let text_path = text_path.to_string_lossy().into_owned();

    let mut sermo = frame::sermo_open("solum:lege");
    frame::sermo_set_opener(&mut sermo, Valor::Textus(text_path.clone()));
    let text: String = frame::sermo_materialize_scalar(&mut sermo);
    assert_eq!(text, "prima\nsecunda\n");
}

#[test]
fn solum_lege_text_as_line_list() {
    let dir = test_dir();
    let text_path = dir.path().join("lege_lines.txt");
    std::fs::write(&text_path, "prima\nsecunda\n").expect("write text fixture");
    let text_path = text_path.to_string_lossy().into_owned();

    let mut sermo = frame::sermo_open("solum:lege");
    frame::sermo_set_opener(&mut sermo, Valor::Textus(text_path.clone()));
    let lines: Vec<String> = frame::sermo_materialize_lista(&mut sermo);
    assert_eq!(lines, vec!["prima".to_owned(), "secunda".to_owned()]);
}

#[test]
fn solum_lege_bytes_as_scalar() {
    let dir = test_dir();
    let bin_path = dir.path().join("lege.bin");
    std::fs::write(&bin_path, [1u8, 2, 3]).expect("write byte fixture");
    let bin_path = bin_path.to_string_lossy().into_owned();

    let mut sermo = frame::sermo_open("solum:lege");
    frame::sermo_set_opener(&mut sermo, Valor::Textus(bin_path.clone()));
    let bytes: Vec<u8> = frame::sermo_materialize_scalar(&mut sermo);
    assert_eq!(bytes, vec![1, 2, 3]);
}

#[test]
fn solum_lege_auto_detects_lista_for_vec() {
    let dir = test_dir();
    let text_path = dir.path().join("auto_lista.txt");
    std::fs::write(&text_path, "prima\nsecunda\n").expect("write text fixture");
    let text_path = text_path.to_string_lossy().into_owned();

    let mut sermo = frame::sermo_open("solum:lege");
    frame::sermo_set_opener(&mut sermo, Valor::Textus(text_path.clone()));
    let auto: Vec<String> = frame::try_sermo_materialize_auto(&mut sermo).expect("auto lista");
    assert_eq!(auto, vec!["prima".to_owned(), "secunda".to_owned()]);
}

#[test]
fn solum_lege_auto_detects_scalar_for_string() {
    let dir = test_dir();
    let text_path = dir.path().join("auto_scalar.txt");
    std::fs::write(&text_path, "prima\nsecunda\n").expect("write text fixture");
    let text_path = text_path.to_string_lossy().into_owned();

    let mut sermo = frame::sermo_open("solum:lege");
    frame::sermo_set_opener(&mut sermo, Valor::Textus(text_path.clone()));
    let auto_s: String = frame::try_sermo_materialize_auto(&mut sermo).expect("auto text");
    assert_eq!(auto_s, "prima\nsecunda\n");
}

#[test]
fn solum_inveni_empty_pattern_is_found_at_start() {
    let dir = test_dir();
    let path = dir.path().join("inveni_empty.bin");
    std::fs::write(&path, b"payload").expect("write search fixture");
    let path = path.to_string_lossy().into_owned();

    let mut sermo = frame::sermo_open("solum:inveni");
    frame::sermo_set_opener(
        &mut sermo,
        Valor::Lista(vec![
            Valor::Textus(path.clone()),
            Valor::Textus(String::new()),
            Valor::Numerus(3),
            Valor::Numerus(8),
        ]),
    );
    let offset: i64 = frame::sermo_materialize_scalar(&mut sermo);
    assert_eq!(offset, 3);
}

#[test]
fn solum_dele_missing_path_is_success() {
    let dir = test_dir();
    let missing = dir.path().join("dele_missing.txt");
    let missing = missing.to_string_lossy().into_owned();
    assert!(!std::path::Path::new(&missing).exists());

    let mut sermo = frame::sermo_open("solum:dele");
    frame::sermo_set_opener(&mut sermo, Valor::Textus(missing));
    frame::sermo_materialize_vacuum(&mut sermo);
}

#[test]
fn solum_partem_route_materializes_dense_bounded_byte_range() {
    let dir = test_dir();
    let path = dir.path().join("partem_dense.bin");
    std::fs::write(&path, [10u8, 11, 12, 13, 14]).expect("write byte range fixture");
    let path = path.to_string_lossy().into_owned();

    let mut sermo = frame::sermo_open("solum:partem");
    frame::sermo_set_opener(
        &mut sermo,
        Valor::Lista(vec![
            Valor::Textus(path.clone()),
            Valor::Numerus(1),
            Valor::Numerus(3),
        ]),
    );
    let chunk = frame::sermo_recv(&mut sermo).expect("byte frame");
    assert_eq!(chunk.status, FrameStatus::Byte);
    assert_eq!(chunk.data, Valor::Octeti(vec![11, 12, 13]));
    let done = frame::sermo_recv(&mut sermo).expect("done frame");
    assert_eq!(done.status, FrameStatus::Done);
}

#[test]
fn solum_partem_route_materializes_large_range_without_valor_list() {
    let dir = test_dir();
    let path = dir.path().join("partem_large.bin");
    let mut data = vec![42u8; 2 * 1024 * 1024];
    data[0] = 7;
    let last = data.len() - 1;
    data[last] = 9;
    std::fs::write(&path, &data).expect("write large range fixture");
    let path = path.to_string_lossy().into_owned();

    let mut sermo = frame::sermo_open("solum:partem");
    frame::sermo_set_opener(
        &mut sermo,
        Valor::Lista(vec![
            Valor::Textus(path.clone()),
            Valor::Numerus(0),
            // SAFETY: test data is small.
            #[allow(clippy::cast_possible_wrap)]
            Valor::Numerus(data.len() as i64),
        ]),
    );
    let chunk = frame::sermo_recv(&mut sermo).expect("large byte frame");
    assert_eq!(chunk.status, FrameStatus::Byte);
    let Valor::Octeti(bytes) = chunk.data else {
        panic!("solum:partem must return dense octeti");
    };
    assert_eq!(bytes.len(), data.len());
    assert_eq!(bytes[0], 7);
    assert_eq!(bytes[bytes.len() - 1], 9);
}

#[test]
fn solum_partem_route_materializes_octeti() {
    let dir = test_dir();
    let path = dir.path().join("partem_octeti.bin");
    std::fs::write(&path, [20u8, 21, 22, 23, 24]).expect("write octeti range fixture");
    let path = path.to_string_lossy().into_owned();

    let mut sermo = frame::sermo_open("solum:partem");
    frame::sermo_set_opener(
        &mut sermo,
        Valor::Lista(vec![
            Valor::Textus(path.clone()),
            Valor::Numerus(2),
            Valor::Numerus(2),
        ]),
    );
    let bytes = frame::sermo_materialize_octeti(&mut sermo);
    assert_eq!(bytes, vec![22, 23]);
}

#[test]
fn solum_mensura_route_materializes_file_size() {
    let dir = test_dir();
    let path = dir.path().join("mensura.bin");
    std::fs::write(&path, [30u8, 31, 32, 33]).expect("write size fixture");
    let path = path.to_string_lossy().into_owned();

    let mut sermo = frame::sermo_open("solum:mensura");
    frame::sermo_set_opener(&mut sermo, Valor::Textus(path.clone()));
    let size: i64 = frame::sermo_materialize_scalar(&mut sermo);
    assert_eq!(size, 4);
}

#[test]
fn solum_inveni_route_materializes_pattern_offset() {
    let dir = test_dir();
    let path = dir.path().join("inveni_pattern.bin");
    std::fs::write(&path, b"prefix-general.file_type-suffix").expect("write search fixture");
    let path = path.to_string_lossy().into_owned();

    let mut sermo = frame::sermo_open("solum:inveni");
    frame::sermo_set_opener(
        &mut sermo,
        Valor::Lista(vec![
            Valor::Textus(path.clone()),
            Valor::Textus("general.file_type".to_owned()),
            Valor::Numerus(0),
            Valor::Numerus(64),
        ]),
    );
    let offset: i64 = frame::sermo_materialize_scalar(&mut sermo);
    assert_eq!(offset, 7);
}

#[test]
fn solum_exstat_route_materializes_bool() {
    let dir = test_dir();
    let path = dir.path().join("exstat.txt");
    std::fs::write(&path, "present").expect("write existence fixture");
    let missing = dir.path().join("exstat.missing");
    let path = path.to_string_lossy().into_owned();
    let missing = missing.to_string_lossy().into_owned();

    let mut present_sermo = frame::sermo_open("solum:exstat");
    frame::sermo_set_opener(&mut present_sermo, Valor::Textus(path.clone()));
    assert!(frame::sermo_materialize_scalar::<bool>(&mut present_sermo));

    let mut missing_sermo = frame::sermo_open("solum:exstat");
    frame::sermo_set_opener(&mut missing_sermo, Valor::Textus(missing));
    assert!(!frame::sermo_materialize_scalar::<bool>(&mut missing_sermo));
}

fn solum_bool_fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = test_dir();
    let path = dir.path().join("bool.txt");
    let sub = dir.path().join("bool.dir");
    std::fs::write(&path, "present").expect("write path bool fixture");
    std::fs::create_dir(&sub).expect("create path bool directory");
    (dir, path, sub)
}

#[test]
fn solum_regularene_true_for_regular_file() {
    let (_dir, path, sub) = solum_bool_fixture();
    let file_path = path.to_string_lossy().into_owned();

    let mut regular_sermo = frame::sermo_open("solum:regularene");
    frame::sermo_set_opener(&mut regular_sermo, Valor::Textus(file_path));
    assert!(frame::sermo_materialize_scalar::<bool>(&mut regular_sermo));
    let _ = sub;
}

#[test]
fn solum_regularene_false_for_directory() {
    let (_dir, _path, sub) = solum_bool_fixture();
    let dir_path = sub.to_string_lossy().into_owned();

    let mut dir_regular_sermo = frame::sermo_open("solum:regularene");
    frame::sermo_set_opener(&mut dir_regular_sermo, Valor::Textus(dir_path));
    assert!(!frame::sermo_materialize_scalar::<bool>(
        &mut dir_regular_sermo
    ));
}

#[test]
fn solum_directoriumne_true_for_directory() {
    let (_dir, _path, sub) = solum_bool_fixture();
    let dir_path = sub.to_string_lossy().into_owned();

    let mut dir_sermo = frame::sermo_open("solum:directoriumne");
    frame::sermo_set_opener(&mut dir_sermo, Valor::Textus(dir_path));
    assert!(frame::sermo_materialize_scalar::<bool>(&mut dir_sermo));
}

#[test]
fn solum_legibilene_true_for_readable_file() {
    let (_dir, path, _sub) = solum_bool_fixture();
    let file_path = path.to_string_lossy().into_owned();

    let mut readable_sermo = frame::sermo_open("solum:legibilene");
    frame::sermo_set_opener(&mut readable_sermo, Valor::Textus(file_path));
    assert!(frame::sermo_materialize_scalar::<bool>(&mut readable_sermo));
}

#[test]
fn solum_scribe_route_materializes_vacuum_after_writing_file() {
    let dir = test_dir();
    let path = dir.path().join("scribe.txt");
    let path = path.to_string_lossy().into_owned();
    let mut sermo = frame::sermo_open("solum:scribe");
    frame::sermo_set_opener(
        &mut sermo,
        Valor::Lista(vec![
            Valor::Textus(path.clone()),
            Valor::Textus("salve".to_owned()),
        ]),
    );

    frame::sermo_materialize_vacuum(&mut sermo);

    assert_eq!(
        std::fs::read_to_string(&path).expect("read written file"),
        "salve"
    );
}

#[test]
fn externally_supplied_incoming_frames_suppress_runtime_fallback() {
    let dir = test_dir();
    let path = dir.path().join("appone.txt");
    let path = path.to_string_lossy().into_owned();
    let mut sermo = frame::sermo_open("solum:appone");
    frame::sermo_set_opener(
        &mut sermo,
        Valor::Lista(vec![
            Valor::Textus(path.clone()),
            Valor::Textus("salve".to_owned()),
        ]),
    );
    sermo.push_incoming(Scrinium {
        id: "host-done".into(),
        parent_id: Some(sermo.conversation_id()),
        call: "solum:appone".into(),
        status: FrameStatus::Done,
        data: Valor::Nihil,
        created_ms: 0,
        from: Some("host".into()),
        trace: None,
    });

    frame::sermo_materialize_vacuum(&mut sermo);

    assert!(!std::path::Path::new(&path).exists());
}

#[test]
fn solum_dele_route_materializes_vacuum_after_removing_file() {
    let dir = test_dir();
    let path = dir.path().join("dele.txt");
    std::fs::write(&path, "stale").expect("write temp file");
    let path = path.to_string_lossy().into_owned();
    let mut sermo = frame::sermo_open("solum:dele");
    frame::sermo_set_opener(&mut sermo, Valor::Textus(path.clone()));

    frame::sermo_materialize_vacuum(&mut sermo);

    assert!(!std::path::Path::new(&path).exists());
}

fn solum_link_dir() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = test_dir();
    let target = dir.path().join("target.txt");
    let link = dir.path().join("link.txt");
    std::fs::write(&target, b"x").expect("write target");
    (dir, target, link)
}

#[test]
fn solum_vincula_creates_symlink() {
    let (_dir, target, link) = solum_link_dir();
    let mut vincula = frame::sermo_open("solum:vincula");
    frame::sermo_set_opener(
        &mut vincula,
        Valor::Lista(vec![
            Valor::Textus(target.to_string_lossy().into_owned()),
            Valor::Textus(link.to_string_lossy().into_owned()),
        ]),
    );
    frame::sermo_materialize_vacuum(&mut vincula);
    assert!(link.is_symlink() || std::fs::symlink_metadata(&link).is_ok());
}

#[test]
fn solum_sequere_follows_symlink() {
    let (_dir, target, link) = solum_link_dir();
    // Create the link via solum:vincula
    let mut vincula = frame::sermo_open("solum:vincula");
    frame::sermo_set_opener(
        &mut vincula,
        Valor::Lista(vec![
            Valor::Textus(target.to_string_lossy().into_owned()),
            Valor::Textus(link.to_string_lossy().into_owned()),
        ]),
    );
    frame::sermo_materialize_vacuum(&mut vincula);

    let mut sequere = frame::sermo_open("solum:sequere");
    frame::sermo_set_opener(
        &mut sequere,
        Valor::Textus(link.to_string_lossy().into_owned()),
    );
    let followed = frame::sermo_materialize_textus(&mut sequere);
    assert!(
        followed.ends_with("target.txt") || followed.contains("target.txt"),
        "sequere={followed}"
    );
}

#[test]
fn solum_modum_sets_file_mode() {
    let (_dir, target, _link) = solum_link_dir();
    let mut modum = frame::sermo_open("solum:modum");
    frame::sermo_set_opener(
        &mut modum,
        Valor::Lista(vec![
            Valor::Textus(target.to_string_lossy().into_owned()),
            Valor::Numerus(0o644),
        ]),
    );
    frame::sermo_materialize_vacuum(&mut modum);
}

#[test]
fn solum_modus_reads_file_mode() {
    let (_dir, target, _link) = solum_link_dir();
    // Set mode first
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644))
            .expect("set mode");
    }

    let mut get_modus = frame::sermo_open("solum:modus");
    frame::sermo_set_opener(
        &mut get_modus,
        Valor::Textus(target.to_string_lossy().into_owned()),
    );
    let mode: i64 = frame::sermo_materialize_scalar(&mut get_modus);
    assert_eq!(mode & 0o777, 0o644);
}

#[test]
fn aleator_semina_seeds_rng() {
    let mut semina = frame::sermo_open("aleator:semina");
    frame::sermo_set_opener(&mut semina, Valor::Numerus(42));
    frame::sermo_materialize_vacuum(&mut semina);
}

#[test]
fn aleator_sortire_returns_bounded_value() {
    let mut semina = frame::sermo_open("aleator:semina");
    frame::sermo_set_opener(&mut semina, Valor::Numerus(42));
    frame::sermo_materialize_vacuum(&mut semina);

    let mut a = frame::sermo_open("aleator:sortire");
    frame::sermo_set_opener(
        &mut a,
        Valor::Lista(vec![Valor::Numerus(1), Valor::Numerus(10)]),
    );
    let n1: i64 = frame::sermo_materialize_scalar(&mut a);
    assert!((1..=10).contains(&n1));
}

#[test]
fn aleator_fractum_returns_unit_range() {
    let mut semina = frame::sermo_open("aleator:semina");
    frame::sermo_set_opener(&mut semina, Valor::Numerus(42));
    frame::sermo_materialize_vacuum(&mut semina);

    let mut fractum = frame::sermo_open("aleator:fractum");
    let f: f64 = frame::sermo_materialize_scalar(&mut fractum);
    assert!((0.0..1.0).contains(&f));
}

#[test]
fn aleator_uuid_returns_formatted_string() {
    let mut uuid = frame::sermo_open("aleator:uuid");
    let id = frame::sermo_materialize_textus(&mut uuid);
    assert_eq!(id.len(), 36);
    assert_eq!(id.chars().filter(|c| *c == '-').count(), 4);
}

#[test]
fn consolum_dic_no_newline_succeeds() {
    let mut dic = frame::sermo_open("consolum:dic");
    frame::sermo_set_opener(&mut dic, Valor::Textus("salve".into()));
    frame::sermo_materialize_vacuum(&mut dic);
}

#[test]
fn consolum_scribe_with_newline_succeeds() {
    let mut scribe = frame::sermo_open("consolum:scribe");
    frame::sermo_set_opener(&mut scribe, Valor::Textus("salve".into()));
    frame::sermo_materialize_vacuum(&mut scribe);
}

#[test]
fn consolum_mone_warning_succeeds() {
    let mut mone = frame::sermo_open("consolum:mone");
    frame::sermo_set_opener(&mut mone, Valor::Textus("cave".into()));
    frame::sermo_materialize_vacuum(&mut mone);
}

#[test]
fn consolum_audit_returns_bool() {
    let mut audit = frame::sermo_open("consolum:audit");
    let _is_tty: bool = frame::sermo_materialize_scalar(&mut audit);
}

#[test]
fn processus_exsequi_route_materializes_stdout() {
    let mut sermo = frame::sermo_open("processus:exsequi");
    frame::sermo_set_opener(
        &mut sermo,
        Valor::Textus("printf runtime-process-ok".into()),
    );

    let output = frame::sermo_materialize_textus(&mut sermo);

    assert_eq!(output, "runtime-process-ok");
}

#[test]
fn processus_exsequi_route_rejects_nonzero_exit() {
    let mut sermo = frame::sermo_open("processus:exsequi");
    frame::sermo_set_opener(&mut sermo, Valor::Textus("exit 7".into()));

    let frame = frame::sermo_recv(&mut sermo).expect("nonzero exit terminal");

    assert_eq!(frame.status, FrameStatus::Error);
    assert!(matches!(
        frame.data,
        Valor::Textus(message)
            if message.contains("processus:exsequi failed")
                && message.contains("exit status 7")
                && !message.contains("stderr:")
    ));
}

#[test]
fn processus_exsequi_route_reports_stderr_on_nonzero_exit() {
    let mut sermo = frame::sermo_open("processus:exsequi");
    frame::sermo_set_opener(
        &mut sermo,
        Valor::Textus("printf runtime-process-error >&2; exit 7".into()),
    );

    let error = frame::try_sermo_materialize_textus(&mut sermo)
        .expect_err("nonzero exit with stderr must fail");

    assert_eq!(error.issue, "frame_materialization_terminal_error");
    assert!(error.message.contains("exit status 7"));
    assert!(error.message.contains("runtime-process-error"));
}

#[test]
fn processus_captura_route_materializes_status_stdout_and_stderr() {
    let mut sermo = frame::sermo_open("processus:captura");
    frame::sermo_set_opener(
        &mut sermo,
        Valor::Lista(vec![
            Valor::Textus("sh".into()),
            Valor::Textus("-c".into()),
            Valor::Textus("printf out; printf err >&2; exit 7".into()),
        ]),
    );

    let output = frame::sermo_materialize_valor(&mut sermo);

    let Valor::Tabula(fields) = output else {
        panic!("expected processus:captura to return a tabula");
    };
    assert_eq!(fields.get("status"), Some(&Valor::Numerus(7)));
    assert_eq!(fields.get("stdout"), Some(&Valor::Textus("out".into())));
    assert_eq!(fields.get("stderr"), Some(&Valor::Textus("err".into())));
}

#[test]
fn processus_scribe_sets_env_var() {
    let key = format!("FABER_PROCESSUS_TEST_{}", frame::next_frame_id());
    let value = "runtime-env-ok";
    let mut scribe = frame::sermo_open("processus:scribe");
    frame::sermo_set_opener(
        &mut scribe,
        Valor::Lista(vec![
            Valor::Textus(key.clone()),
            Valor::Textus(value.into()),
        ]),
    );
    frame::sermo_materialize_vacuum(&mut scribe);
    // Verify env var was set
    assert_eq!(std::env::var(&key).ok().as_deref(), Some(value));
    std::env::remove_var(&key);
}

#[test]
fn processus_lege_reads_env_var() {
    let key = format!("FABER_PROCESSUS_TEST_{}", frame::next_frame_id());
    let value = "runtime-env-ok";
    std::env::set_var(&key, value);

    let mut lege = frame::sermo_open("processus:lege");
    frame::sermo_set_opener(&mut lege, Valor::Textus(key.clone()));
    let read = frame::sermo_materialize_textus(&mut lege);
    assert_eq!(read, value);
    std::env::remove_var(&key);
}

#[test]
fn processus_sedes_returns_non_empty_cwd() {
    let mut sedes = frame::sermo_open("processus:sedes");
    let cwd = frame::sermo_materialize_textus(&mut sedes);
    assert!(!cwd.is_empty());
    assert!(std::path::Path::new(&cwd).is_dir());
}

#[test]
fn processus_identitas_returns_process_pid() {
    let mut identitas = frame::sermo_open("processus:identitas");
    let pid = frame::sermo_materialize_scalar::<i64>(&mut identitas);
    assert!(pid > 0);
    assert_eq!(pid, i64::from(std::process::id()));
}

#[test]
fn processus_muta_changes_and_restores_cwd() {
    let original = std::env::current_dir().expect("cwd");
    let target = std::env::temp_dir();
    let mut muta = frame::sermo_open("processus:muta");
    frame::sermo_set_opener(
        &mut muta,
        Valor::Textus(target.to_string_lossy().into_owned()),
    );
    frame::sermo_materialize_vacuum(&mut muta);
    assert_eq!(
        std::env::current_dir().expect("cwd after muta"),
        target.canonicalize().unwrap_or(target)
    );
    std::env::set_current_dir(original).expect("restore cwd");
}

#[test]
fn solum_parens_route_materializes_parent_path() {
    let mut sermo = frame::sermo_open("solum:parens");
    frame::sermo_set_opener(&mut sermo, Valor::Textus("/tmp/faber/path.txt".into()));

    let output = frame::sermo_materialize_textus(&mut sermo);

    assert_eq!(output, "/tmp/faber");
}

#[test]
fn try_sermo_materialize_scalar_returns_error_for_bad_payload() {
    let mut sermo = frame::sermo_open("runtime:echo");
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
    let mut textus = frame::sermo_open("runtime:echo");
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
    let mut valor = frame::sermo_open("runtime:echo");
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
    let mut scalar = frame::sermo_open("runtime:echo");
    frame::sermo_set_opener(&mut scalar, Valor::Numerus(9));
    assert_eq!(
        block_on(frame::sermo_materialize_scalar_async::<i64>(&mut scalar)),
        9
    );
}

#[test]
fn instans_async_mirrors_sync_materializer() {
    let mut instans = frame::sermo_open("tempus:nunc");
    let materialized = block_on(frame::sermo_materialize_instans_async(
        &mut instans,
        crate::InstansPraecisio::Nanosecunda,
    ));
    assert_eq!(
        materialized.praecisio(),
        crate::InstansPraecisio::Nanosecunda
    );
}

// ===========================================================================
// Edge and sad-path tests
// ===========================================================================

#[test]
fn tempus_dormiet_zero_duration_succeeds() {
    // Zero millis is a valid sleep duration.
    let mut sermo = frame::sermo_open("tempus:dormiet");
    frame::sermo_set_opener(&mut sermo, Valor::Numerus(0));
    frame::sermo_materialize_vacuum(&mut sermo);
}

#[test]
fn solum_lege_missing_file_returns_error() {
    let dir = test_dir();
    let missing = dir.path().join("lege_missing.txt");
    let missing = missing.to_string_lossy().into_owned();

    let mut sermo = frame::sermo_open("solum:lege");
    frame::sermo_set_opener(&mut sermo, Valor::Textus(missing));
    let frame = frame::sermo_recv(&mut sermo).expect("lege missing terminal");
    assert_eq!(frame.status, FrameStatus::Error);
}

#[test]
fn solum_mensura_missing_file_returns_error() {
    let dir = test_dir();
    let missing = dir.path().join("mensura_missing.txt");
    let missing = missing.to_string_lossy().into_owned();

    let mut sermo = frame::sermo_open("solum:mensura");
    frame::sermo_set_opener(&mut sermo, Valor::Textus(missing));
    let error = frame::try_sermo_materialize_scalar::<i64>(&mut sermo)
        .expect_err("mensura missing must fail");
    assert_eq!(error.issue, "frame_materialization_terminal_error");
}

#[test]
fn solum_partem_empty_file_returns_byte_frame() {
    let dir = test_dir();
    let path = dir.path().join("partem_empty.bin");
    std::fs::write(&path, []).expect("write empty fixture");
    let path = path.to_string_lossy().into_owned();

    let mut sermo = frame::sermo_open("solum:partem");
    frame::sermo_set_opener(
        &mut sermo,
        Valor::Lista(vec![
            Valor::Textus(path.clone()),
            Valor::Numerus(0),
            Valor::Numerus(0),
        ]),
    );
    let frame = frame::sermo_recv(&mut sermo).expect("empty byte frame");
    assert_eq!(frame.status, FrameStatus::Byte);
    assert_eq!(frame.data, Valor::Octeti(vec![]));
    let done = frame::sermo_recv(&mut sermo).expect("done after empty");
    assert_eq!(done.status, FrameStatus::Done);
}

#[test]
fn solum_inveni_pattern_not_found_returns_neg_one() {
    let dir = test_dir();
    let path = dir.path().join("inveni_notfound.bin");
    std::fs::write(&path, b"abcdef").expect("write search fixture");
    let path = path.to_string_lossy().into_owned();

    let mut sermo = frame::sermo_open("solum:inveni");
    frame::sermo_set_opener(
        &mut sermo,
        Valor::Lista(vec![
            Valor::Textus(path.clone()),
            Valor::Textus("notfound".to_owned()),
            Valor::Numerus(0),
            Valor::Numerus(64),
        ]),
    );
    let offset: i64 = frame::sermo_materialize_scalar(&mut sermo);
    assert_eq!(offset, -1);
}

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
fn solum_scribe_route_writes_empty_content() {
    let dir = test_dir();
    let path = dir.path().join("scribe_empty.txt");
    let path = path.to_string_lossy().into_owned();
    let mut sermo = frame::sermo_open("solum:scribe");
    frame::sermo_set_opener(
        &mut sermo,
        Valor::Lista(vec![
            Valor::Textus(path.clone()),
            Valor::Textus(String::new()),
        ]),
    );
    frame::sermo_materialize_vacuum(&mut sermo);
    assert_eq!(
        std::fs::read_to_string(&path).expect("read written file"),
        ""
    );
}

// ---- CG2 Stage 3 S3-U2: Sermo terminal-state repair (M01 + M02) ----

fn seed_aleator(seed: i64) {
    let mut semina = frame::sermo_open("aleator:semina");
    frame::sermo_set_opener(&mut semina, Valor::Numerus(seed));
    frame::sermo_materialize_vacuum(&mut semina);
}

fn fractum_fraction() -> f64 {
    let mut fractum = frame::sermo_open("aleator:fractum");
    frame::sermo_materialize_scalar(&mut fractum)
}

#[test]
fn builtin_route_probe_is_pure_route_key_lookup() {
    // M01: the classification probe must be a pure route-key lookup — probing
    // `aleator:fractum` (whose dispatch draws from the global RNG) must leave
    // the seeded RNG state untouched. The RNG mutex is held for the whole
    // sequence (no parallel aleator test can interleave a draw), and the
    // production classification function — the one the rejecting-host fallback
    // probes — is called directly against the live RNG state.
    frame::with_aleator_rng_lock(|rng| {
        rng.seed(42);
        let before = rng.state();
        assert!(frame::is_builtin_route("aleator:fractum"));
        assert!(frame::is_builtin_route("consolum:lege"));
        assert!(!frame::is_builtin_route("missing:route"));
        assert!(!frame::is_builtin_route("consolum:"));
        let after = rng.state();
        assert_eq!(
            before, after,
            "the builtin-route probe must be a pure route-key lookup — it consumed seeded RNG state"
        );
    });
}

#[test]
fn rejecting_host_aleator_fractum_probe_preserves_seeded_output() {
    // M01: with a rejecting host, the fallback probe of `aleator:fractum` must
    // not consume the seeded RNG, so the fraction produced after the probe
    // equals the un-probed seed's first fraction. An impure probe shifts the
    // output by one draw on every attempt (consecutive seeded draws are
    // distinct), so a bounded retry only absorbs parallel-suite RNG noise —
    // it never masks a defect.
    for _ in 0..8 {
        seed_aleator(42);
        let baseline = fractum_fraction();
        seed_aleator(42);
        let mut probed_sermo =
            frame::sermo_open_with_dispatch("aleator:fractum", Arc::new(RejectingDispatch));
        let probed: f64 = frame::sermo_materialize_scalar(&mut probed_sermo);
        if probed == baseline {
            return;
        }
    }
    panic!(
        "the aleator:fractum classification probe consumed seeded RNG state — \
         the probed fraction differs from the un-probed seed"
    );
}

#[test]
fn rejecting_host_consolum_lege_probe_returns_promptly_with_unreadable_stdin() {
    // M01: probing `consolum:lege` must not execute its stdin-reading frame
    // builder. The probe runs in a child whose stdin is an open pipe that
    // never delivers data and never EOFs — any stdin read blocks. The probe
    // must return within the deadline; otherwise the child never exits and the
    // parent kills it and fails.
    const CHILD_ENV: &str = "FABER_S3U2_LEGE_PROBE_CHILD";
    if std::env::var_os(CHILD_ENV).is_some() {
        let started = std::time::Instant::now();
        let mut sermo = frame::sermo_open_with_dispatch("consolum:lege", Arc::new(RejectingDispatch));
        let wake = Arc::new(CountingWake::default());
        let waker = test_waker(&wake);
        let mut cx = Context::from_waker(&waker);
        let prompt = {
            let mut future = Box::pin(frame::sermo_recv_async(&mut sermo));
            let polled = Future::poll(Pin::as_mut(&mut future), &mut cx);
            drop(future);
            matches!(polled, Poll::Pending)
        };
        let prompt = prompt && started.elapsed() <= std::time::Duration::from_secs(2);
        std::process::exit(if prompt { 0 } else { 42 });
    }

    let mut child = std::process::Command::new(std::env::current_exe().expect("current test binary"))
        .arg("rejecting_host_consolum_lege_probe_returns_promptly_with_unreadable_stdin")
        .arg("--nocapture")
        .env(CHILD_ENV, "1")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("spawn probe child");
    // Hold the pipe's write end open: the child's stdin stays an open pipe
    // with no readable data (never EOFs), so any stdin read blocks.
    let stdin_writer = child.stdin.take().expect("child stdin write end");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let status = loop {
        if let Some(status) = child.try_wait().expect("child status") {
            break status;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            drop(stdin_writer);
            panic!(
                "rejecting-host consolum:lege probe did not return within the 2s deadline — \
                 the classification is executing the stdin-reading frame builder"
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    drop(stdin_writer);
    assert!(status.success(), "consolum:lege probe child failed: {status:?}");
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
