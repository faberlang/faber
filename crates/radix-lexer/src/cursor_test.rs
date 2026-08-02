//! Cursor character-level scanning contract: byte offset tracking, lookahead
//! without mutation, and conditional consumption (`eat_while`).

use super::Cursor;

#[test]
fn advance_tracks_utf8_byte_offsets() {
    let mut cursor = Cursor::new("aβ𐍈");

    assert_eq!(cursor.pos(), 0);
    assert_eq!(cursor.advance(), Some('a'));
    assert_eq!(cursor.pos(), 1);
    assert_eq!(cursor.advance(), Some('β'));
    assert_eq!(cursor.pos(), 3);
    assert_eq!(cursor.advance(), Some('𐍈'));
    assert_eq!(cursor.pos(), 7);
    assert_eq!(cursor.advance(), None);
}

#[test]
fn lookahead_does_not_advance_cursor() {
    let cursor = Cursor::new("ab");

    assert_eq!(cursor.peek(), Some('a'));
    assert_eq!(cursor.peek_next(), Some('b'));
    assert_eq!(cursor.pos(), 0);
}

#[test]
fn slice_and_rest_use_cursor_positions() {
    let mut cursor = Cursor::new("aβc");
    let start = cursor.pos();
    cursor.advance();
    cursor.advance();
    let end = cursor.pos();

    assert_eq!(cursor.slice(start, end), "aβ");
    assert_eq!(cursor.rest(), "c");
}

#[test]
fn eat_while_consumes_matching_prefix_only() {
    let mut cursor = Cursor::new("abc123");

    cursor.eat_while(|c| c.is_ascii_alphabetic());

    assert_eq!(cursor.pos(), 3);
    assert_eq!(cursor.rest(), "123");
}
