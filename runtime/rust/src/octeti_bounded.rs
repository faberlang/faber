//! Faber `octeti<N>` runtime carrier: length plus `N` bytes.
//!
//! Distinct from unbounded `Vec<u8>` (bare `octeti`). Runtime `len` is `0..=N`;
//! spare `len..N` is unreadable. Overflow on append/grow fails closed — no
//! truncate, no silent realloc. No ASCII-byte invariant: any `u8` is a legal
//! payload. `[i]` is the live byte return.

/// Recoverable overflow when a payload would exceed declared `N`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OctetiNOverflow {
    /// Declared capacity `N`.
    pub capacity: usize,
    /// Attempted total length that did not fit.
    pub attempted: usize,
}

/// Bounded byte buffer `octeti<N>`: `len` plus `N` bytes.
///
/// `N` is type identity. [`OctetiN<1>`] is [`Copy`]; other `N` are [`Clone`]
/// only. Unbounded `Vec<u8>` is a different type.
///
/// ```
/// fn needs_copy<T: Copy>() {}
/// needs_copy::<faber::OctetiN<1>>();
/// ```
#[derive(Clone)]
pub struct OctetiN<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl Copy for OctetiN<1> {}

impl<const N: usize> PartialEq for OctetiN<N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl<const N: usize> Eq for OctetiN<N> {}

impl<const N: usize> std::fmt::Debug for OctetiN<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OctetiN")
            .field("n", &N)
            .field("payload", &self.as_bytes())
            .finish()
    }
}

impl<const N: usize> OctetiN<N> {
    /// Empty value: `len == 0`. Spare `0..N` stays unreadable.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            bytes: [0u8; N],
            len: 0,
        }
    }

    /// Build from a byte payload of length `≤ N`.
    ///
    /// Returns [`Err`] if `value.len() > N` (overflow uses declared `N`; no
    /// truncate). Any byte value is legal — there is no ASCII check.
    pub fn new(value: &[u8]) -> Result<Self, OctetiNOverflow> {
        if value.len() > N {
            return Err(OctetiNOverflow {
                capacity: N,
                attempted: value.len(),
            });
        }
        let mut bytes = [0u8; N];
        bytes[..value.len()].copy_from_slice(value);
        Ok(Self {
            bytes,
            len: value.len(),
        })
    }

    /// Append one byte. Fails closed when `len == N`.
    pub fn appende(&mut self, byte: u8) -> Result<(), OctetiNOverflow> {
        if self.len >= N {
            return Err(OctetiNOverflow {
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

    /// Initialized prefix only. Spare `len..N` is not in this slice.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    /// Byte `[i]`. Spare `len..N` is not a readable index.
    #[must_use]
    pub fn get(&self, i: usize) -> Option<u8> {
        self.as_bytes().get(i).copied()
    }

    /// Bounded → unbounded: copy the valid prefix.
    #[must_use]
    pub fn to_octeti(&self) -> Vec<u8> {
        self.as_bytes().to_vec()
    }

    /// Unbounded → bounded: fail closed when `len > N`.
    pub fn try_from_octeti(bytes: &[u8]) -> Result<Self, OctetiNOverflow> {
        Self::new(bytes)
    }
}

impl OctetiN<1> {
    /// Construct a one-byte `octeti<1>` with no allocation.
    #[must_use]
    pub fn from_byte(byte: u8) -> Self {
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

impl<const N: usize> From<&OctetiN<N>> for Vec<u8> {
    fn from(value: &OctetiN<N>) -> Self {
        value.to_octeti()
    }
}

impl<const N: usize> From<OctetiN<N>> for Vec<u8> {
    fn from(value: OctetiN<N>) -> Self {
        value.to_octeti()
    }
}

impl<const N: usize> TryFrom<&[u8]> for OctetiN<N> {
    type Error = OctetiNOverflow;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        Self::try_from_octeti(value)
    }
}

impl<const N: usize> TryFrom<&Vec<u8>> for OctetiN<N> {
    type Error = OctetiNOverflow;

    fn try_from(value: &Vec<u8>) -> Result<Self, Self::Error> {
        Self::try_from_octeti(value)
    }
}

impl<const N: usize> std::fmt::Display for OctetiN<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<{} bytes>", self.len)
    }
}

impl<const N: usize> std::ops::Deref for OctetiN<N> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_bytes()
    }
}

impl<const N: usize> AsRef<[u8]> for OctetiN<N> {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::{OctetiN, OctetiNOverflow};

    fn assert_clone<T: Clone>() {}
    fn assert_copy<T: Copy>() {}

    #[test]
    fn construct_n8_of_length_8() {
        let a = OctetiN::<8>::new(&[1, 2, 3, 4, 5, 6, 7, 8]).expect("length 8 fits octeti<8>");
        assert_eq!(a.len(), 8);
        assert_eq!(a.longitudo(), 8);
        assert_eq!(a.as_bytes(), &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(!a.vacua());
    }

    #[test]
    fn append_of_ninth_byte_fails_closed() {
        let mut a = OctetiN::<8>::new(&[1, 2, 3, 4, 5, 6, 7, 8]).expect("length 8 fits");
        let err = a
            .appende(9)
            .expect_err("9th byte must fail against declared N=8");
        assert_eq!(
            err,
            OctetiNOverflow {
                capacity: 8,
                attempted: 9,
            }
        );
        assert_eq!(a.as_bytes(), &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(a.len(), 8);
    }

    #[test]
    fn octeti1_is_copy() {
        assert_copy::<OctetiN<1>>();
        let a = OctetiN::<1>::from_byte(0xff);
        let b = a;
        let c = a;
        assert_eq!(b.as_bytes(), &[0xff]);
        assert_eq!(c.as_bytes(), &[0xff]);
        assert_eq!(a.to_byte(), Some(0xff));
        assert_eq!(
            std::mem::size_of::<OctetiN<1>>(),
            std::mem::size_of::<SelfSize>()
        );
    }

    /// Layout probe: `octeti<1>` is the byte plus `len`, no heap slot.
    struct SelfSize {
        _bytes: [u8; 1],
        _len: usize,
    }

    #[test]
    fn octeti8_is_clone() {
        assert_clone::<OctetiN<8>>();
        let a = OctetiN::<8>::new(&[0x80, 0xff]).unwrap();
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn spare_bytes_are_unreadable() {
        let a = OctetiN::<8>::new(&[0xab, 0xcd]).unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(a.as_bytes(), &[0xab, 0xcd]);
        assert_eq!(a.get(0), Some(0xab));
        assert_eq!(a.get(1), Some(0xcd));
        assert_eq!(a.get(2), None);
        assert_eq!(a.get(7), None);
        assert_eq!(format!("{a:?}"), "OctetiN { n: 8, payload: [171, 205] }");
        assert_eq!(format!("{a}"), "<2 bytes>");
    }

    #[test]
    fn n0_is_empty_only() {
        let a = OctetiN::<0>::new(&[]).expect("empty fits octeti<0>");
        assert!(a.vacua());
        assert_eq!(a.len(), 0);
        assert_eq!(a.as_bytes(), b"");
        let err = OctetiN::<0>::new(&[1]).expect_err("any byte overflows N=0");
        assert_eq!(
            err,
            OctetiNOverflow {
                capacity: 0,
                attempted: 1,
            }
        );
        let mut empty = OctetiN::<0>::empty();
        assert!(empty.appende(0xff).is_err());
        assert!(empty.vacua());
    }

    #[test]
    fn new_rejects_payload_longer_than_n() {
        let err =
            OctetiN::<8>::new(&[0, 1, 2, 3, 4, 5, 6, 7, 8]).expect_err("9-byte payload vs N=8");
        assert_eq!(err.capacity, 8);
        assert_eq!(err.attempted, 9);
    }

    #[test]
    fn conversions_to_from_bare_octeti() {
        let bounded = OctetiN::<8>::new(&[1, 2, 3]).unwrap();
        let unbounded: Vec<u8> = bounded.to_octeti();
        assert_eq!(unbounded, vec![1, 2, 3]);
        let round = OctetiN::<8>::try_from_octeti(&unbounded).unwrap();
        assert_eq!(round, bounded);
        let too_long = vec![0u8; 11];
        assert!(OctetiN::<8>::try_from_octeti(&too_long).is_err());
        let from_ref = Vec::<u8>::from(&bounded);
        assert_eq!(from_ref, unbounded);
        let via_try = OctetiN::<8>::try_from(unbounded.as_slice()).unwrap();
        assert_eq!(via_try.as_bytes(), &[1, 2, 3]);
        let via_vec_ref = OctetiN::<8>::try_from(&unbounded).unwrap();
        assert_eq!(via_vec_ref, bounded);
    }

    #[test]
    fn accepts_non_ascii_bytes() {
        let a = OctetiN::<4>::new(&[0x00, 0x7f, 0x80, 0xff]).unwrap();
        assert_eq!(a.as_bytes(), &[0x00, 0x7f, 0x80, 0xff]);
        let mut b = OctetiN::<2>::empty();
        b.appende(0x80).unwrap();
        b.appende(0xff).unwrap();
        assert_eq!(b.as_bytes(), &[0x80, 0xff]);
        assert_eq!(OctetiN::<1>::from_byte(0x80).to_byte(), Some(0x80));
    }

    #[test]
    fn empty_octeti1_is_copy_and_not_from_byte() {
        let a = OctetiN::<1>::empty();
        let b = a;
        assert!(a.vacua());
        assert_eq!(b.to_byte(), None);
        assert_eq!(OctetiN::<1>::from_byte(0x00).to_byte(), Some(0x00));
    }
}
