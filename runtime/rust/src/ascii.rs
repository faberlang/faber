//! Faber `ascii` runtime newtype.

/// Inline SSO bucket for short [`Ascii`] values.
///
/// Representation only — not a type identity and not a constructor limit.
/// Payloads longer than this go to the heap. `from_byte` always stays here.
const INLINE: usize = 23;

#[derive(Clone, Debug)]
enum Repr {
    Inline { bytes: [u8; INLINE], len: u8 },
    Heap(Box<str>),
}

/// ASCII-only text carrier for Faber `ascii`.
///
/// Unbounded growable cord: `Clone`, not `Copy`. Short payloads stay inline
/// (zero-alloc); longer payloads own a heap buffer. Every constructor
/// enforces an ASCII payload (a strict subset of UTF-8) in debug and release
/// builds, so the stored bytes always form a valid UTF-8 string.
///
/// ```compile_fail
/// fn needs_copy<T: Copy>() {}
/// needs_copy::<faber::Ascii>();
/// ```
#[derive(Clone, Debug)]
pub struct Ascii {
    repr: Repr,
}

impl PartialEq for Ascii {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl Eq for Ascii {}

impl Ascii {
    /// Build an [`Ascii`] value from a string payload of any length.
    ///
    /// # Panics
    ///
    /// Panics if `value` is not ASCII (any byte `>= 0x80`). The ASCII check
    /// is unconditional — in debug *and* release — so a non-ASCII payload is
    /// trapped at the host boundary instead of producing an [`Ascii`] value
    /// that violates its invariant.
    #[must_use]
    pub fn new(value: &str) -> Self {
        assert!(
            value.is_ascii(),
            "ascii payload must be ASCII (0x00..=0x7f), got non-ASCII input"
        );
        Self::from_ascii_bytes(value.as_bytes())
    }

    #[must_use]
    pub fn try_from_textus(text: &str) -> Option<Self> {
        text.is_ascii().then(|| Self::new(text))
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
        if !bytes.is_ascii() {
            return None;
        }
        // SAFETY: ASCII is a strict subset of UTF-8, so any ASCII byte
        // sequence is valid UTF-8.
        let text = std::str::from_utf8(bytes).expect("ascii byte slice must be valid utf-8");
        Some(Self::new(text))
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
        let mut bytes = [0u8; INLINE];
        bytes[0] = byte;
        Self {
            repr: Repr::Inline { bytes, len: 1 },
        }
    }

    /// The single byte of a one-byte value, or `None` for empty/multi-byte.
    #[must_use]
    pub fn to_byte(&self) -> Option<u8> {
        match self.as_bytes() {
            [byte] => Some(*byte),
            _ => None,
        }
    }

    #[must_use]
    pub fn to_textus(&self) -> String {
        self.as_str().to_owned()
    }

    fn from_ascii_bytes(payload: &[u8]) -> Self {
        if payload.len() <= INLINE {
            let mut bytes = [0u8; INLINE];
            bytes[..payload.len()].copy_from_slice(payload);
            Self {
                repr: Repr::Inline {
                    bytes,
                    len: payload.len() as u8,
                },
            }
        } else {
            let text = std::str::from_utf8(payload).expect("ascii payload must be valid utf-8");
            Self {
                repr: Repr::Heap(text.into()),
            }
        }
    }

    fn as_bytes(&self) -> &[u8] {
        match &self.repr {
            Repr::Inline { bytes, len } => &bytes[..*len as usize],
            Repr::Heap(text) => text.as_bytes(),
        }
    }

    fn as_str(&self) -> &str {
        // SAFETY: every constructor guarantees an ASCII payload, which is a
        // strict subset of UTF-8, so the stored slice is always valid UTF-8.
        match &self.repr {
            Repr::Inline { bytes, len } => std::str::from_utf8(&bytes[..*len as usize])
                .expect("ascii payload must be valid utf-8"),
            Repr::Heap(text) => text,
        }
    }

    #[cfg(test)]
    fn is_inline(&self) -> bool {
        matches!(self.repr, Repr::Inline { .. })
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
    use super::Ascii;

    fn assert_clone<T: Clone>() {}

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
        assert!(
            a.is_inline(),
            "from_byte must stay on the zero-alloc inline path"
        );
    }

    #[test]
    fn from_byte_accepts_top_of_ascii_range() {
        // 0x7f (DEL) is the highest byte the ASCII invariant admits; it must
        // construct and round-trip, and its payload must stay valid UTF-8.
        let a = Ascii::from_byte(0x7f);
        assert_eq!(a.to_byte(), Some(0x7f));
        assert_eq!(&*a, "\x7f");
        assert_eq!(a.to_textus(), "\x7f");
        assert!(a.is_inline());
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
        assert_eq!(Ascii::try_from_bytes(&[0x68, 0x69]), Some(Ascii::new("hi")));
        assert_eq!(Ascii::try_from_bytes(&[0xff]), None);
    }

    #[test]
    fn display_roundtrips() {
        assert_eq!(Ascii::new("a").to_string(), "a");
        assert_eq!(format!("{}", Ascii::new("hi")), "hi");
    }

    #[test]
    fn is_clone_not_copy() {
        assert_clone::<Ascii>();
        let a = Ascii::new("clone");
        let b = a.clone();
        assert_eq!(a, b);
        assert_eq!(b, Ascii::new("clone"));
    }

    #[test]
    fn clone_two_uses_in_one_function() {
        fn two_uses(a: Ascii) -> (Ascii, Ascii) {
            (a.clone(), a)
        }
        let (left, right) = two_uses(Ascii::new("multi"));
        assert_eq!(&*left, "multi");
        assert_eq!(&*right, "multi");
        assert_eq!(left, right);
    }

    #[test]
    fn loop_carried_values_compile_after_copy_drops() {
        fn loop_carried(mut acc: Ascii, n: usize) -> Ascii {
            for _ in 0..n {
                let next = acc.clone();
                acc = next;
            }
            acc
        }
        let out = loop_carried(Ascii::new("carry"), 4);
        assert_eq!(&*out, "carry");
    }

    #[test]
    fn sixty_five_byte_ascii_payload_constructs() {
        let big = "x".repeat(65);
        let a = Ascii::new(&big);
        let b = Ascii::try_from_textus(&big).expect("65-byte ASCII is a legal unbounded payload");
        let c = Ascii::try_from_bytes(big.as_bytes())
            .expect("65-byte ASCII is a legal unbounded payload");
        assert_eq!(&*a, big.as_str());
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert!(
            !a.is_inline(),
            "payloads past the SSO bucket own a heap buffer"
        );
    }

    #[test]
    fn try_paths_still_reject_non_ascii_when_long() {
        let mut big = "x".repeat(65);
        big.push('π');
        assert_eq!(Ascii::try_from_textus(&big), None);
        assert_eq!(Ascii::try_from_bytes(big.as_bytes()), None);
    }
}
