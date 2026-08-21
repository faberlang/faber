//! Faber `textus<N>` runtime carrier: length plus `N` Unicode scalars.
//!
//! Distinct from unbounded `textus` (`String`). Storage is scalar-addressable
//! (UTF-32 equivalent): `[char; N]`, so `[i]` is O(1). Capacity `N` is
//! scalars; the byte budget is `4N`. Runtime `len` is `0..=N`; spare `len..N`
//! is unreadable. Overflow on append/grow fails closed — no truncate, no
//! silent realloc, no UTF-8 heap with a max-length sticker.

use crate::AsciiN;

/// Recoverable overflow when a payload would exceed declared `N` scalars.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextusNOverflow {
    /// Declared capacity `N` in Unicode scalars.
    pub capacity: usize,
    /// Attempted total scalar length that did not fit.
    pub attempted: usize,
}

/// Bounded Unicode cord `textus<N>`: `len` plus `N` scalar slots.
///
/// `N` is type identity. Unbounded `textus` (`String`) is a different type.
/// Indexing a stored ASCII scalar returns [`AsciiN<1>`]; a non-ASCII scalar
/// traps.
#[derive(Clone)]
pub struct TextusN<const N: usize> {
    scalars: [char; N],
    len: usize,
}

impl<const N: usize> PartialEq for TextusN<N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_scalars() == other.as_scalars()
    }
}

impl<const N: usize> Eq for TextusN<N> {}

impl<const N: usize> std::fmt::Debug for TextusN<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextusN")
            .field("n", &N)
            .field("payload", &self.to_textus())
            .finish()
    }
}

impl<const N: usize> TextusN<N> {
    /// Empty value: `len == 0`. Spare `0..N` stays unreadable.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            scalars: ['\0'; N],
            len: 0,
        }
    }

    /// Build from a UTF-8 payload of scalar length `≤ N`.
    ///
    /// Returns [`Err`] if the Unicode scalar count exceeds `N` (overflow uses
    /// declared `N`; no truncate). `N` counts scalars, not UTF-8 bytes.
    pub fn new(value: &str) -> Result<Self, TextusNOverflow> {
        let attempted = value.chars().count();
        if attempted > N {
            return Err(TextusNOverflow {
                capacity: N,
                attempted,
            });
        }
        let mut scalars = ['\0'; N];
        for (i, ch) in value.chars().enumerate() {
            scalars[i] = ch;
        }
        Ok(Self {
            scalars,
            len: attempted,
        })
    }

    /// Append one Unicode scalar. Fails closed when `len == N`.
    pub fn appende(&mut self, scalar: char) -> Result<(), TextusNOverflow> {
        if self.len >= N {
            return Err(TextusNOverflow {
                capacity: N,
                attempted: self.len.saturating_add(1),
            });
        }
        self.scalars[self.len] = scalar;
        self.len += 1;
        Ok(())
    }

    /// Runtime `len` in scalars, never `N`.
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

    /// Rust empty predicate. Same as [`Self::vacua`].
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.vacua()
    }

    /// Initialized scalar prefix. Direct slot access is O(1); spare `len..N`
    /// is not in this slice.
    #[must_use]
    pub fn as_scalars(&self) -> &[char] {
        &self.scalars[..self.len]
    }

    /// Scalar `[i]` → `ascii<1>`. Spare `len..N` is not a readable index.
    ///
    /// Direct slot, not a UTF-8 walk. Panics if the stored scalar is not ASCII
    /// (live non-ASCII trap; `ascii<1>` cannot carry it).
    #[must_use]
    pub fn get(&self, i: usize) -> Option<AsciiN<1>> {
        let ch = *self.as_scalars().get(i)?;
        let code = u32::from(ch);
        assert!(
            code <= 0x7F,
            "textus scalar index traps on non-ASCII scalar U+{code:04X}"
        );
        Some(AsciiN::<1>::from_byte(code as u8))
    }

    /// Bounded → unbounded: copy the valid prefix.
    #[must_use]
    pub fn to_textus(&self) -> String {
        self.as_scalars().iter().collect()
    }

    /// Unbounded → bounded: fail closed when scalar `len > N`.
    pub fn try_from_textus(value: &str) -> Result<Self, TextusNOverflow> {
        Self::new(value)
    }
}

impl<const N: usize> From<&TextusN<N>> for String {
    fn from(value: &TextusN<N>) -> Self {
        value.to_textus()
    }
}

impl<const N: usize> From<TextusN<N>> for String {
    fn from(value: TextusN<N>) -> Self {
        value.to_textus()
    }
}

impl<const N: usize> TryFrom<&str> for TextusN<N> {
    type Error = TextusNOverflow;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_from_textus(value)
    }
}

impl<const N: usize> TryFrom<&String> for TextusN<N> {
    type Error = TextusNOverflow;

    fn try_from(value: &String) -> Result<Self, Self::Error> {
        Self::try_from_textus(value)
    }
}

impl<const N: usize> std::fmt::Display for TextusN<N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(crate::display_text_payload(&self.to_textus()))
    }
}

#[cfg(test)]
mod tests {
    use super::{TextusN, TextusNOverflow};
    use crate::AsciiN;

    fn assert_clone<T: Clone>() {}

    /// Layout probe: `textus<N>` is `N` inline `char` slots plus `len`, not a
    /// heap `String` plus a max-length sticker.
    struct ScalarSlots<const N: usize> {
        _scalars: [char; N],
        _len: usize,
    }

    #[test]
    fn storage_is_n_inline_scalars_not_utf8_heap() {
        assert_eq!(
            std::mem::size_of::<TextusN<4>>(),
            std::mem::size_of::<ScalarSlots<4>>()
        );
        assert_eq!(
            std::mem::size_of::<TextusN<32>>(),
            std::mem::size_of::<ScalarSlots<32>>()
        );
        assert!(std::mem::size_of::<TextusN<32>>() >= 32 * 4);
        assert!(std::mem::size_of::<TextusN<32>>() > std::mem::size_of::<String>());
    }

    #[test]
    fn two_scalars_in_textus4_have_longitudo_2() {
        let t = TextusN::<4>::new("πΩ").expect("2 scalars fit textus<4>");
        assert_eq!(t.len(), 2);
        assert_eq!(t.longitudo(), 2);
        assert_eq!(t.as_scalars(), &['π', 'Ω']);
        assert!(!t.vacua());
        assert_eq!(t.to_textus(), "πΩ");
    }

    #[test]
    fn index_is_direct_scalar_slot_not_utf8_walk() {
        let t = TextusN::<4>::new("a🔥b").expect("3 scalars fit textus<4>");
        assert_eq!(t.longitudo(), 3);
        assert_eq!(t.as_scalars()[0], 'a');
        assert_eq!(t.as_scalars()[1], '🔥');
        assert_eq!(t.as_scalars()[2], 'b');
        assert_eq!(t.get(0).and_then(|c| c.to_byte()), Some(b'a'));
        assert_eq!(t.get(2).and_then(|c| c.to_byte()), Some(b'b'));
        assert_eq!(t.get(3), None);
    }

    #[test]
    fn append_past_n_fails_closed() {
        let mut t = TextusN::<2>::new("ab").expect("length 2 fits");
        let err = t
            .appende('c')
            .expect_err("3rd scalar must fail against declared N=2");
        assert_eq!(
            err,
            TextusNOverflow {
                capacity: 2,
                attempted: 3,
            }
        );
        assert_eq!(t.to_textus(), "ab");
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn is_empty_matches_vacua() {
        let empty = TextusN::<4>::empty();
        assert_eq!(empty.is_empty(), empty.vacua());
        assert!(empty.is_empty());
        let filled = TextusN::<4>::new("πΩ").unwrap();
        assert_eq!(filled.is_empty(), filled.vacua());
        assert!(!filled.is_empty());
    }

    #[test]
    fn n0_is_empty_only() {
        let t = TextusN::<0>::new("").expect("empty fits textus<0>");
        assert!(t.vacua());
        assert_eq!(t.len(), 0);
        assert_eq!(t.as_scalars(), &[] as &[char]);
        let err = TextusN::<0>::new("x").expect_err("any scalar overflows N=0");
        assert_eq!(
            err,
            TextusNOverflow {
                capacity: 0,
                attempted: 1,
            }
        );
        let mut empty = TextusN::<0>::empty();
        assert!(empty.appende('x').is_err());
        assert!(empty.vacua());
    }

    #[test]
    fn empty_value() {
        let t = TextusN::<4>::empty();
        assert!(t.vacua());
        assert_eq!(t.longitudo(), 0);
        assert_eq!(t.get(0), None);
        assert_eq!(t.to_textus(), "");
    }

    #[test]
    fn spare_slots_are_unreadable() {
        let t = TextusN::<8>::new("ab").unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t.as_scalars(), &['a', 'b']);
        assert_eq!(t.get(0).map(|c| c.to_byte()), Some(Some(b'a')));
        assert_eq!(t.get(1).map(|c| c.to_byte()), Some(Some(b'b')));
        assert_eq!(t.get(2), None);
        assert_eq!(t.get(7), None);
        assert_eq!(format!("{t:?}"), r#"TextusN { n: 8, payload: "ab" }"#);
    }

    #[test]
    fn equality_ignores_spare_slots() {
        let a = TextusN::<4>::new("xy").unwrap();
        let mut b = TextusN::<4>::empty();
        b.appende('x').unwrap();
        b.appende('y').unwrap();
        assert_eq!(a, b);
        assert_ne!(a, TextusN::<4>::new("x").unwrap());
    }

    #[test]
    fn nul_in_prefix_is_a_scalar_not_a_terminator() {
        let t = TextusN::<4>::new("\0a").unwrap();
        assert_eq!(t.len(), 2);
        assert_eq!(t.as_scalars(), &['\0', 'a']);
        assert_eq!(t.get(0).and_then(|c| c.to_byte()), Some(0));
        assert_eq!(t.get(2), None);
    }

    #[test]
    fn new_rejects_payload_longer_than_n_scalars() {
        let err = TextusN::<2>::new("🔥🔥🔥").expect_err("3 scalars vs N=2");
        assert_eq!(err.capacity, 2);
        assert_eq!(err.attempted, 3);
    }

    #[test]
    fn conversions_to_from_bare_textus() {
        let bounded = TextusN::<8>::new("hello").unwrap();
        let unbounded: String = bounded.to_textus();
        assert_eq!(unbounded, "hello");
        let round = TextusN::<8>::try_from_textus(&unbounded).unwrap();
        assert_eq!(round, bounded);
        let too_long = "abcdefghijk".to_string();
        assert!(TextusN::<8>::try_from_textus(&too_long).is_err());
        let from_ref = String::from(&bounded);
        assert_eq!(from_ref, unbounded);
        let via_try = TextusN::<8>::try_from(&unbounded).unwrap();
        assert_eq!(via_try.to_textus(), "hello");
    }

    #[test]
    fn get_returns_ascii1_for_ascii_scalar() {
        assert_clone::<TextusN<8>>();
        let t = TextusN::<4>::new("Hi").unwrap();
        let ch: AsciiN<1> = t.get(1).expect("index 1 in len 2");
        assert_eq!(ch.to_byte(), Some(b'i'));
        assert_eq!(ch.as_str(), "i");
    }

    #[test]
    fn get_traps_on_non_ascii_scalar() {
        let t = TextusN::<4>::new("aπ").unwrap();
        let result = std::panic::catch_unwind(|| t.get(1));
        assert!(result.is_err());
        assert_eq!(t.get(0).and_then(|c| c.to_byte()), Some(b'a'));
    }

    #[test]
    fn textus8_is_clone() {
        let a = TextusN::<8>::new("hi").unwrap();
        let b = a.clone();
        assert_eq!(a, b);
    }
}
