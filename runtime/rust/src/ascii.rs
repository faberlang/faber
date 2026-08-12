//! Faber `ascii` runtime newtype.

/// Maximum number of bytes an [`Ascii`] value can carry, inline.
///
/// WHY: multi-char ascii literals (`'hi'`, `'solum:lege'`) are legal today, so
/// a strict `u8` scalar would regress the literal surface. The inline
/// fixed-capacity buffer keeps [`Ascii`] `Copy` and heap-free, which is what
/// makes scalar-index returns non-allocating. Measured corpus max ascii-literal
/// payload is 20 bytes (2026-08-10; 2312 `.fab` files), so 64 carries >3×
/// headroom. Over-capacity literals are rejected at typecheck
/// (`ascii_literal_too_long`); the runtime `new` panics as a safety net.
pub const ASCII_CAPACITY: usize = 64;

/// ASCII-only text carrier for Faber `ascii`.
///
/// Byte-backed inline value: `Copy`, no heap allocation. Every constructor
/// enforces an ASCII payload (a strict subset of UTF-8) in debug and release
/// builds, so the stored bytes always form a valid UTF-8 string; pad bytes
/// beyond `len` are zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ascii {
    bytes: [u8; ASCII_CAPACITY],
    len: u8,
}

impl Ascii {
    /// Build an [`Ascii`] value from a string payload.
    ///
    /// # Panics
    ///
    /// Panics if `value` is not ASCII (any byte `>= 0x80`) or longer than
    /// [`ASCII_CAPACITY`] bytes. The ASCII check is unconditional — in debug
    /// *and* release — so a non-ASCII payload is trapped at the host boundary
    /// instead of producing an [`Ascii`] value that violates its invariant.
    #[must_use]
    pub fn new(value: &str) -> Self {
        assert!(
            value.is_ascii(),
            "ascii payload must be ASCII (0x00..=0x7f), got non-ASCII input"
        );
        let payload = value.as_bytes();
        assert!(
            payload.len() <= ASCII_CAPACITY,
            "ascii payload exceeds capacity {ASCII_CAPACITY} bytes"
        );
        let mut bytes = [0u8; ASCII_CAPACITY];
        bytes[..payload.len()].copy_from_slice(payload);
        Self {
            bytes,
            len: payload.len() as u8,
        }
    }

    #[must_use]
    pub fn try_from_textus(text: &str) -> Option<Self> {
        if text.is_ascii() && text.len() <= ASCII_CAPACITY {
            Some(Self::new(text))
        } else {
            None
        }
    }

    /// WHY: `octeti ↦ ascii` conversio needs a direct bytes→ascii path that fails
    /// on any byte ≥ 128, independent of UTF-8 validity. Going through `textus`
    /// first would conflate ASCII validity with UTF-8 validity. This validates
    /// ASCII directly and constructs the carrier.
    ///
    /// # Panics
    ///
    /// Panics if the byte slice is ASCII-valid but not valid UTF-8. This should
    /// never happen because ASCII is a strict subset of UTF-8.
    #[must_use]
    pub fn try_from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.is_ascii() && bytes.len() <= ASCII_CAPACITY {
            // SAFETY: ASCII is a strict subset of UTF-8, so any ASCII byte
            // sequence is valid UTF-8.
            let text = std::str::from_utf8(bytes).expect("ascii byte slice must be valid utf-8");
            Some(Self::new(text))
        } else {
            None
        }
    }

    /// Construct a single-byte value with no allocation — the scalar-index
    /// return path (e.g. `Ascii::from_byte(0x61)` yields the `'a'` value).
    ///
    /// # Panics
    ///
    /// Panics if `byte >= 0x80`. The trap fires before construction, so an
    /// [`Ascii`] value can never carry a non-ASCII byte — a value built here
    /// is always valid UTF-8 at `as_str` time.
    #[must_use]
    pub fn from_byte(byte: u8) -> Self {
        assert!(
            byte.is_ascii(),
            "ascii byte must be in 0x00..=0x7f, got 0x{byte:02x}"
        );
        let mut bytes = [0u8; ASCII_CAPACITY];
        bytes[0] = byte;
        Self { bytes, len: 1 }
    }

    /// The single byte of a one-byte value, or `None` for empty/multi-byte.
    #[must_use]
    pub fn to_byte(&self) -> Option<u8> {
        (self.len == 1).then_some(self.bytes[0])
    }

    #[must_use]
    pub fn to_textus(&self) -> String {
        self.as_str().to_owned()
    }

    fn as_str(&self) -> &str {
        // SAFETY: every constructor guarantees an ASCII payload, which is a
        // strict subset of UTF-8, so the stored slice is always valid UTF-8.
        std::str::from_utf8(&self.bytes[..self.len as usize])
            .expect("ascii payload must be valid utf-8")
    }
}

impl std::fmt::Display for Ascii {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(crate::display_text_payload(self.as_ref()))
    }
}

impl std::ops::Deref for Ascii {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl AsRef<str> for Ascii {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::{Ascii, ASCII_CAPACITY};

    #[test]
    fn new_single_char_roundtrips() {
        let a = Ascii::new("x");
        assert_eq!(&*a, "x");
        assert_eq!(a.to_textus(), "x");
        assert_eq!(a.as_ref(), "x");
    }

    #[test]
    fn new_multi_char_still_works() {
        let a = Ascii::new("hi");
        assert_eq!(&*a, "hi");
        assert_eq!(a.len(), 2);
        assert_eq!(a.to_textus(), "hi");
    }

    #[test]
    fn from_byte_yields_char_value_without_allocation() {
        let a = Ascii::from_byte(0x61);
        assert_eq!(a.to_byte(), Some(0x61));
        assert_eq!(&*a, "a");
        assert_eq!(a, Ascii::new("a"));
    }

    #[test]
    fn from_byte_accepts_top_of_ascii_range() {
        // 0x7f (DEL) is the highest byte the ASCII invariant admits; it must
        // construct and round-trip, and its payload must stay valid UTF-8.
        let a = Ascii::from_byte(0x7f);
        assert_eq!(a.to_byte(), Some(0x7f));
        assert_eq!(&*a, "\x7f");
        assert_eq!(a.to_textus(), "\x7f");
    }

    #[test]
    #[should_panic(expected = "ascii byte must be in 0x00..=0x7f")]
    fn from_byte_rejects_0x80() {
        let _ = Ascii::from_byte(0x80);
    }

    #[test]
    #[should_panic(expected = "ascii byte must be in 0x00..=0x7f")]
    fn from_byte_rejects_0xff() {
        // CTO ASCII-1 repro: `Ascii::from_byte(0xff).to_textus()` used to
        // construct an invalid value that panicked inside `as_str`; it now
        // traps before construction and never reaches `to_textus`.
        let _ = Ascii::from_byte(0xff).to_textus();
    }

    #[test]
    #[should_panic(expected = "ascii payload must be ASCII")]
    fn new_rejects_non_ascii() {
        // π (U+03C0) is valid UTF-8 but not ASCII; `Ascii::new` must reject it
        // in debug AND release builds (the assert is unconditional).
        let _ = Ascii::new("π");
    }

    #[test]
    fn to_byte_is_none_for_empty_and_multi_char() {
        assert_eq!(Ascii::new("").to_byte(), None);
        assert_eq!(Ascii::new("hi").to_byte(), None);
    }

    #[test]
    fn try_from_textus_and_bytes() {
        assert_eq!(Ascii::try_from_textus("ok"), Some(Ascii::new("ok")));
        assert_eq!(Ascii::try_from_textus("π"), None);
        assert_eq!(
            Ascii::try_from_bytes(&[0x68, 0x69]),
            Some(Ascii::new("hi"))
        );
        assert_eq!(Ascii::try_from_bytes(&[0xff]), None);
    }

    #[test]
    fn display_roundtrips() {
        assert_eq!(Ascii::new("a").to_string(), "a");
        assert_eq!(format!("{}", Ascii::new("hi")), "hi");
    }

    #[test]
    fn is_copy() {
        let a = Ascii::new("copy");
        let b = a; // would not compile if `Ascii` were not `Copy`
        assert_eq!(a, b);
        assert_eq!(b, Ascii::new("copy"));
    }

    #[test]
    fn over_capacity_try_paths_return_none() {
        let big = "x".repeat(ASCII_CAPACITY + 1);
        assert_eq!(Ascii::try_from_textus(&big), None);
        assert_eq!(Ascii::try_from_bytes(big.as_bytes()), None);
    }

    #[test]
    #[should_panic(expected = "ascii payload exceeds capacity")]
    fn new_over_capacity_panics() {
        let big = "x".repeat(ASCII_CAPACITY + 1);
        let _ = Ascii::new(&big);
    }
}
