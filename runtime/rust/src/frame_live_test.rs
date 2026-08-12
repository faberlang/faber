use crate::frame::{self, FrameStatus, Scrinium};
use crate::Valor;

fn seed_inbound(sermo: &mut frame::Sermo, frames: Vec<(FrameStatus, Valor)>) {
    for (status, data) in frames {
        sermo.push_incoming(Scrinium {
            id: frame::next_frame_id(),
            parent_id: Some(sermo.conversation_id()),
            call: "test:live".into(),
            status,
            data,
            created_ms: 0,
            from: None,
            trace: None,
        });
    }
}

#[test]
fn meus_da_and_fini_emit_outbound_frames() {
    let mut sermo = frame::sermo_open("test:live");
    let meus = frame::sermo_meus::<String>(&sermo);
    frame::meus_da(&meus, Valor::Textus("alpha".into())).expect("da");
    assert_eq!(frame::meus_fini(&meus), FrameStatus::Done);
    let err = frame::meus_da(&meus, Valor::Textus("late".into())).expect_err("closed");
    assert_eq!(err.issue, "frame_meus_half_stream_closed");
    let _ = &mut sermo;
}

#[test]
fn tuus_accipe_returns_nihil_after_terminal() {
    let mut sermo = frame::sermo_open("test:live");
    seed_inbound(
        &mut sermo,
        vec![
            (FrameStatus::Item, Valor::Textus("one".into())),
            (FrameStatus::Done, Valor::Nihil),
        ],
    );
    let tuus = frame::sermo_tuus::<String>(&sermo);
    let first = frame::tuus_accipe(&tuus).expect("content frame");
    assert_eq!(first.data, Valor::Textus("one".into()));
    assert!(frame::tuus_accipe(&tuus).is_none());
    assert_eq!(frame::tuus_fini(&tuus), FrameStatus::Done);
}

#[test]
fn tuus_accipe_and_cursor_share_inbound_queue() {
    let mut sermo = frame::sermo_open("test:live");
    seed_inbound(
        &mut sermo,
        vec![
            (FrameStatus::Item, Valor::Textus("a".into())),
            (FrameStatus::Item, Valor::Textus("b".into())),
            (FrameStatus::Done, Valor::Nihil),
        ],
    );
    let tuus = frame::sermo_tuus::<String>(&sermo);
    let first = frame::tuus_accipe(&tuus).expect("first");
    assert_eq!(first.data, Valor::Textus("a".into()));
    let cursor: Vec<_> = frame::tuus_cursor(&tuus).collect();
    assert_eq!(cursor.len(), 1);
    assert_eq!(cursor[0].data, Valor::Textus("b".into()));
    assert!(frame::tuus_accipe(&tuus).is_none());
}

#[test]
fn tuus_exhauri_does_not_close_meus_outbound() {
    let mut sermo = frame::sermo_open("test:live");
    seed_inbound(
        &mut sermo,
        vec![
            (FrameStatus::Item, Valor::Textus("hello".into())),
            (FrameStatus::Done, Valor::Nihil),
        ],
    );
    let tuus = frame::sermo_tuus::<String>(&sermo);
    let mut drain_sermo = frame::tuus_as_sermo(&tuus);
    let text = frame::sermo_materialize_textus(&mut drain_sermo);
    assert_eq!(text, "hello");
    let meus = frame::sermo_meus::<String>(&sermo);
    frame::meus_da(&meus, Valor::Textus("still open".into())).expect("meus open after exhauri");
}

#[test]
fn tuus_fini_preserves_error_terminal_after_accipe() {
    let mut sermo = frame::sermo_open("test:live");
    seed_inbound(
        &mut sermo,
        vec![
            (FrameStatus::Item, Valor::Textus("payload".into())),
            (FrameStatus::Error, Valor::Textus("E_TEST".into())),
        ],
    );
    let tuus = frame::sermo_tuus::<String>(&sermo);
    let first = frame::tuus_accipe(&tuus).expect("item frame");
    assert_eq!(first.data, Valor::Textus("payload".into()));
    assert!(frame::tuus_accipe(&tuus).is_none());
    assert_eq!(frame::tuus_fini(&tuus), FrameStatus::Error);
}

#[test]
fn interleaved_send_receive_on_shared_sermo() {
    let mut sermo = frame::sermo_open("test:live");
    let meus = frame::sermo_meus::<String>(&sermo);
    let tuus = frame::sermo_tuus::<String>(&sermo);

    frame::meus_da(&meus, Valor::Textus("req1".into())).expect("da1");
    seed_inbound(
        &mut sermo,
        vec![(FrameStatus::Item, Valor::Textus("rep1".into()))],
    );
    let rep1 = frame::tuus_accipe(&tuus).expect("rep1");
    assert_eq!(rep1.data, Valor::Textus("rep1".into()));

    frame::meus_da(&meus, Valor::Textus("req2".into())).expect("da2");
    seed_inbound(
        &mut sermo,
        vec![
            (FrameStatus::Item, Valor::Textus("rep2".into())),
            (FrameStatus::Done, Valor::Nihil),
        ],
    );
    let rep2 = frame::tuus_accipe(&tuus).expect("rep2");
    assert_eq!(rep2.data, Valor::Textus("rep2".into()));
    assert_eq!(frame::meus_fini(&meus), FrameStatus::Done);
    assert!(frame::tuus_accipe(&tuus).is_none());
    assert_eq!(frame::tuus_fini(&tuus), FrameStatus::Done);
    let _ = &mut sermo;
}
