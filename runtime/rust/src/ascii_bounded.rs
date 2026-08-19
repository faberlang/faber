//! Faber `ascii<N>` runtime carrier: length plus `N` bytes.
//!
//! Distinct from unbounded [`crate::Ascii`]. Runtime `len` is `0..=N`; spare
//! `len..N` is unreadable. Overflow on append/grow fails closed — no truncate,
//! no silent realloc. `AsciiN<1>` is always inline [`Copy`] and is what
//! [`AsciiN<1>::from_byte`] constructs.

use crate::Ascii;

/// Recoverable overflow when a payload would exceed declared `N`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsciiNOverflow {
    /// Declared capacity `N`.
    pub capacity: usize,
    /// Attempted total length that did not fit.
    pub attempted: usize,
}

/// Bounded ASCII cord `ascii<N>`: `len` plus `N` bytes.
///
/// `N` is type identity. [`AsciiN<1>`] is [`Copy`]; other `N` are [`Clone`]
/// only. Unbounded [`Ascii`] is a different type.
///
/// ```
/// fn needs_copy<T: Copy>() {}
/// needs_copy::<faber::AsciiN<1>>();
/// ```
#[derive(Clone)]
pub struct AsciiN<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl Copy for AsciiN<1> {}

impl<const N: usize> PartialEq for AsciiN<N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl<const N: usize> Eq for AsciiN<N> {}

impl<const N: usize> std::fmt::Debug for AsciiN<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AsciiN")
            .field("n", &N)
            .field("payload", &self.as_str())
            .finish()
    }
}

impl<const N: usize> AsciiN<N> {
    /// Empty value: `len == 0`. Spare `0..N` stays unreadable.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            bytes: [0u8; N],
            len: 0,
        }
    }

    /// Build from an ASCII payload of length `≤ N`.
    ///
    /// Returns [`Err`] if `value.len() > N` (overflow uses declared `N`; no
    /// truncate). Panics if `value` is not ASCII.
    pub fn new(value: &str) -> Result<Self, AsciiNOverflow> {
        assert!(
            value.is_ascii(),
            "ascii payload must be ASCII (0x00..=0x7f), got non-ASCII input"
        );
        Self::from_ascii_bytes(value.as_bytes())
    }

    fn from_ascii_bytes(payload: &[u8]) -> Result<Self, AsciiNOverflow> {
        if payload.len() > N {
            return Err(AsciiNOverflow {
                capacity: N,
                attempted: payload.len(),
            });
        }
        let mut bytes = [0u8; N];
        bytes[..payload.len()].copy_from_slice(payload);
        Ok(Self {
            bytes,
            len: payload.len(),
        })
    }

    /// Append one ASCII byte. Fails closed when `len == N`.
    pub fn appende(&mut self, byte: u8) -> Result<(), AsciiNOverflow> {
        assert!(
            byte.is_ascii(),
            "ascii byte must be in 0x00..=0x7f, got 0x{byte:02x}"
        );
        if self.len >= N {
            return Err(AsciiNOverflow {
                capacity: N,
                attempted: self.len.saturating_add(1),
            });
        }
        self.bytes[self.len] = byte;
        self.len += 1;
        Ok(())
    }

    /// Runtime `len`, never `N`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// `longitudo` — same as [`Self::len`], as `i64`.
    #[must_use]
    pub fn longitudo(&self) -> i64 {
        self.len as i64
    }

    /// Whether `len == 0`.
    #[must_use]
    pub fn vacua(&self) -> bool {
        self.len == 0
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        // SAFETY-equivalent: every constructor stores only ASCII, a subset of UTF-8.
        std::str::from_utf8(self.as_bytes()).expect("ascii payload must be valid utf-8")
    }

    /// Scalar `[i]` → `ascii<1>`. Spare `len..N` is not a readable index.
    #[must_use]
    pub fn get(&self, i: usize) -> Option<AsciiN<1>> {
        self.as_bytes().get(i).copied().map(AsciiN::<1>::from_byte)
    }

    /// Bounded → unbounded: copy the valid prefix.
    #[must_use]
    pub fn to_ascii(&self) -> Ascii {
        Ascii::new(self.as_str())
    }

    /// Unbounded → bounded: fail closed when `len > N`.
    pub fn try_from_ascii(ascii: &Ascii) -> Result<Self, AsciiNOverflow> {
        Self::new(ascii.as_ref())
    }
}

impl AsciiN<1> {
    /// Construct a one-byte `ascii<1>` with no allocation — the scalar-index
    /// return path.
    ///
    /// # Panics
    ///
    /// Panics if `byte >= 0x80`.
    #[must_use]
    pub fn from_byte(byte: u8) -> Self {
        assert!(
            byte.is_ascii(),
            "ascii byte must be in 0x00..=0x7f, got 0x{byte:02x}"
        );
        Self {
            bytes: [byte],
            len: 1,
        }
    }

    /// The single byte of a one-byte value, or `None` for empty.
    #[must_use]
    pub fn to_byte(&self) -> Option<u8> {
        match self.as_bytes() {
            [byte] => Some(*byte),
            _ => None,
        }
    }
}

impl<const N: usize> From<&AsciiN<N>> for Ascii {
    fn from(value: &AsciiN<N>) -> Self {
        value.to_ascii()
    }
}

impl<const N: usize> From<AsciiN<N>> for Ascii {
    fn from(value: AsciiN<N>) -> Self {
        value.to_ascii()
    }
}

impl<const N: usize> TryFrom<&Ascii> for AsciiN<N> {
    type Error = AsciiNOverflow;

    fn try_from(value: &Ascii) -> Result<Self, Self::Error> {
        Self::try_from_ascii(value)
    }
}

impl<const N: usize> std::fmt::Display for AsciiN<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(crate::display_text_payload(self.as_str()))
    }
}

impl<const N: usize> std::ops::Deref for AsciiN<N> {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl<const N: usize> AsRef<str> for AsciiN<N> {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::{AsciiN, AsciiNOverflow};
    use crate::Ascii;

    fn assert_clone<T: Clone>() {}
    fn assert_copy<T: Copy>() {}

    #[test]
    fn construct_n8_of_length_8() {
        let a = AsciiN::<8>::new("abcdefgh").expect("length 8 fits ascii<8>");
        assert_eq!(a.len(), 8);
        assert_eq!(a.longitudo(), 8);
        assert_eq!(a.as_str(), "abcdefgh");
        assert_eq!(a.as_bytes(), b"abcdefgh");
        assert!(!a.vacua());
    }

    #[test]
    fn append_of_ninth_byte_fails_closed() {
        let mut a = AsciiN::<8>::new("abcdefgh").expect("length 8 fits");
        let err = a
            .appende(b'i')
            .expect_err("9th byte must fail against declared N=8");
        assert_eq!(
            err,
            AsciiNOverflow {
                capacity: 8,
                attempted: 9,
            }
        );
        assert_eq!(a.as_str(), "abcdefgh");
        assert_eq!(a.len(), 8);
    }

    #[test]
    fn ascii1_is_copy() {
        assert_copy::<AsciiN<1>>();
        let a = AsciiN::<1>::from_byte(b'a');
        let b = a;
        let c = a;
        assert_eq!(b.as_str(), "a");
        assert_eq!(c.as_str(), "a");
        assert_eq!(a.to_byte(), Some(b'a'));
        assert_eq!(
            std::mem::size_of::<AsciiN<1>>(),
            std::mem::size_of::<SelfSize>()
        );
    }

    /// Layout probe: `ascii<1>` is the byte plus `len`, no heap slot.
    struct SelfSize {
        _bytes: [u8; 1],
        _len: usize,
    }

    #[test]
    fn ascii8_is_clone() {
        assert_clone::<AsciiN<8>>();
        let a = AsciiN::<8>::new("hi").unwrap();
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn spare_bytes_are_unreadable() {
        let a = AsciiN::<8>::new("ab").unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(a.as_bytes(), b"ab");
        assert_eq!(a.get(0).map(|c| c.to_byte()), Some(Some(b'a')));
        assert_eq!(a.get(1).map(|c| c.to_byte()), Some(Some(b'b')));
        assert_eq!(a.get(2), None);
        assert_eq!(a.get(7), None);
        assert_eq!(format!("{a:?}"), r#"AsciiN { n: 8, payload: "ab" }"#);
    }

    #[test]
    fn n0_is_empty_only() {
        let a = AsciiN::<0>::new("").expect("empty fits ascii<0>");
        assert!(a.vacua());
        assert_eq!(a.len(), 0);
        assert_eq!(a.as_str(), "");
        assert_eq!(a.as_bytes(), b"");
        let err = AsciiN::<0>::new("x").expect_err("any byte overflows N=0");
        assert_eq!(
            err,
            AsciiNOverflow {
                capacity: 0,
                attempted: 1,
            }
        );
        let mut empty = AsciiN::<0>::empty();
        assert!(empty.appende(b'x').is_err());
        assert!(empty.vacua());
    }

    #[test]
    fn new_rejects_payload_longer_than_n() {
        let err = AsciiN::<8>::new("abcdefghi").expect_err("9-byte literal vs N=8");
        assert_eq!(err.capacity, 8);
        assert_eq!(err.attempted, 9);
    }

    #[test]
    fn conversions_to_from_bare_ascii() {
        let bounded = AsciiN::<8>::new("hello").unwrap();
        let unbounded: Ascii = bounded.to_ascii();
        assert_eq!(&*unbounded, "hello");
        let round = AsciiN::<8>::try_from_ascii(&unbounded).unwrap();
        assert_eq!(round, bounded);
        let too_long = Ascii::new("abcdefghijk");
        assert!(AsciiN::<8>::try_from_ascii(&too_long).is_err());
        let from_ref = Ascii::from(&bounded);
        assert_eq!(from_ref, unbounded);
        let via_try = AsciiN::<8>::try_from(&unbounded).unwrap();
        assert_eq!(via_try.as_str(), "hello");
    }

    #[test]
    fn from_byte_rejects_non_ascii() {
        let result = std::panic::catch_unwind(|| AsciiN::<1>::from_byte(0x80));
        assert!(result.is_err());
    }

    #[test]
    #[should_panic(expected = "ascii payload must be ASCII")]
    fn new_rejects_non_ascii() {
        let _ = AsciiN::<8>::new("π");
    }

    #[test]
    fn empty_ascii1_is_copy_and_not_from_byte() {
        let a = AsciiN::<1>::empty();
        let b = a;
        assert!(a.vacua());
        assert_eq!(b.to_byte(), None);
        assert_eq!(AsciiN::<1>::from_byte(b'x').to_byte(), Some(b'x'));
    }
}
