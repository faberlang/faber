use super::*;

#[test]
fn raised_error_is_handed_out_once() {
    let cursor = GeneratorCursor(Rc::new(RefCell::new(CursorState {
        frame: None,
        sermo: None,
        current: None,
        pulled: 0,
        read: 0,
        raised: Some(Value::Str("nulla".to_owned())),
    })));
    assert_eq!(cursor.take_raised(), Some(Value::Str("nulla".to_owned())));
    assert_eq!(cursor.take_raised(), None);
}
