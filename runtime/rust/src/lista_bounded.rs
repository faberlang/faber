//! Faber `lista<T, N>` runtime carrier: length plus `N` typed slots.
//!
//! The carrier stores only the initialized prefix. Spare slots are
//! [`MaybeUninit`] storage and are never exposed as `T`, so `T` need not
//! implement [`Default`]. This first host carrier does not narrow the source
//! element type: every `Sized` Rust `T` can occupy a typed fixed slot here.
//! That host representation choice does not imply device admission for
//! runtime-managed or dynamically allocated element types.
//!
//! Bare `lista<T>` remains the unbounded `Vec<T>` carrier. `ListaN<T, N>` is a
//! distinct bounded type; its `len` is always in `0..=N`, and an append past
//! `N` returns [`ListaNOverflow`] without changing the initialized prefix.

use std::mem::{ManuallyDrop, MaybeUninit};

/// Recoverable overflow when a bounded list operation would exceed `N`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListaNOverflow {
    /// Declared capacity `N`.
    pub capacity: usize,
    /// Attempted total length that did not fit.
    pub attempted: usize,
}

/// Bounded `lista<T, N>` with an initialized readable prefix.
///
/// `N` is part of the Rust type. The storage contains exactly `N` typed
/// slots, but only `0..len` are initialized and readable. `ListaN<T, 0>` is a
/// valid empty type. The representation does not promise stack placement;
/// placement remains a lowering choice.
pub struct ListaN<T, const N: usize> {
    slots: [MaybeUninit<T>; N],
    len: usize,
}

impl<T, const N: usize> ListaN<T, N> {
    /// Construct an empty bounded list. Spare slots remain unreadable.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            slots: std::array::from_fn(|_| MaybeUninit::uninit()),
            len: 0,
        }
    }

    /// Construct from an unbounded `Vec<T>`, failing closed when it is too
    /// long. The oversized input is not truncated or silently reallocated.
    pub fn new(values: Vec<T>) -> Result<Self, ListaNOverflow> {
        Self::try_from_vec(values)
    }

    /// Construct from an unbounded `Vec<T>`, failing closed when `len > N`.
    pub fn try_from_vec(values: Vec<T>) -> Result<Self, ListaNOverflow> {
        let attempted = values.len();
        if attempted > N {
            return Err(ListaNOverflow {
                capacity: N,
                attempted,
            });
        }

        let mut bounded = Self::empty();
        for value in values {
            // The length check above makes this append infallible. Keeping the
            // checked operation here preserves one overflow policy for all
            // insertion paths.
            bounded
                .appende(value)
                .expect("validated Vec length must fit ListaN capacity");
        }
        Ok(bounded)
    }

    /// Append one element. Returns a recoverable error when `len == N` and
    /// leaves the initialized prefix unchanged.
    pub fn appende(&mut self, value: T) -> Result<(), ListaNOverflow> {
        if self.len >= N {
            return Err(ListaNOverflow {
                capacity: N,
                attempted: self.len.saturating_add(1),
            });
        }

        self.slots[self.len].write(value);
        self.len += 1;
        Ok(())
    }

    /// Runtime length, never capacity.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// `longitudo` — the runtime length as `i64`.
    #[must_use]
    pub fn longitudo(&self) -> i64 {
        self.len as i64
    }

    /// Whether the initialized prefix is empty.
    #[must_use]
    pub fn vacua(&self) -> bool {
        self.len == 0
    }

    /// Rust empty predicate. Same as [`Self::vacua`].
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.vacua()
    }

    /// Read an initialized element. Spare slots return `None`.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&T> {
        self.as_slice().get(index)
    }

    /// Read the initialized prefix. The spare range is never represented as a
    /// slice of `T`.
    #[must_use]
    pub fn as_slice(&self) -> &[T] {
        // SAFETY: `slots[0..self.len]` are initialized by `empty` + `appende`
        // or `try_from_vec`, and `self.len <= N` is maintained by those
        // constructors and methods. `MaybeUninit<T>` has the same layout and
        // alignment as `T`; the slice is limited to the initialized prefix.
        unsafe { std::slice::from_raw_parts(self.slots.as_ptr() as *const T, self.len) }
    }

    /// Iterate over the initialized prefix only.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.as_slice().iter()
    }

    /// Bounded → unbounded: copy the initialized prefix into an unbounded
    /// `Vec<T>` (the bare `lista<T>` carrier).
    #[must_use]
    pub fn to_lista(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.as_slice().to_vec()
    }

    /// Alias for [`Self::to_lista`] using the Rust carrier name.
    #[must_use]
    pub fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.to_lista()
    }

    /// Unbounded → bounded: copy a `lista<T>` prefix and fail closed when
    /// `len > N`.
    pub fn try_from_lista(values: &[T]) -> Result<Self, ListaNOverflow>
    where
        T: Clone,
    {
        let attempted = values.len();
        if attempted > N {
            return Err(ListaNOverflow {
                capacity: N,
                attempted,
            });
        }

        let mut bounded = Self::empty();
        for value in values {
            bounded
                .appende(value.clone())
                .expect("validated lista length must fit ListaN capacity");
        }
        Ok(bounded)
    }

    /// Move the initialized prefix into an unbounded `Vec<T>`.
    #[must_use]
    pub fn into_vec(self) -> Vec<T> {
        let this = ManuallyDrop::new(self);
        let mut values = Vec::with_capacity(this.len);
        for slot in this.slots.iter().take(this.len) {
            // SAFETY: only the initialized prefix is visited, and the
            // ManuallyDrop wrapper prevents the source slots from being
            // dropped after each value is moved out.
            values.push(unsafe { slot.assume_init_read() });
        }
        values
    }
}

impl<T, const N: usize> Default for ListaN<T, N> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<T, const N: usize> Drop for ListaN<T, N> {
    fn drop(&mut self) {
        for slot in self.slots.iter_mut().take(self.len) {
            // SAFETY: only the initialized prefix is dropped.
            unsafe { slot.assume_init_drop() };
        }
    }
}

impl<T: Clone, const N: usize> Clone for ListaN<T, N> {
    fn clone(&self) -> Self {
        let mut cloned = Self::empty();
        for value in self.as_slice() {
            cloned
                .appende(value.clone())
                .expect("a clone has the same length and capacity");
        }
        cloned
    }
}

impl<T: PartialEq, const N: usize> PartialEq for ListaN<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Eq, const N: usize> Eq for ListaN<T, N> {}

impl<T: std::fmt::Debug, const N: usize> std::fmt::Debug for ListaN<T, N> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListaN")
            .field("n", &N)
            .field("len", &self.len)
            .field("items", &self.as_slice())
            .finish()
    }
}

impl<T, const N: usize> AsRef<[T]> for ListaN<T, N> {
    fn as_ref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T, const N: usize> std::ops::Deref for ListaN<T, N> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T, const N: usize> From<ListaN<T, N>> for Vec<T> {
    fn from(value: ListaN<T, N>) -> Self {
        value.into_vec()
    }
}

impl<T: Clone, const N: usize> From<&ListaN<T, N>> for Vec<T> {
    fn from(value: &ListaN<T, N>) -> Self {
        value.to_vec()
    }
}

impl<T, const N: usize> TryFrom<Vec<T>> for ListaN<T, N> {
    type Error = ListaNOverflow;

    fn try_from(value: Vec<T>) -> Result<Self, Self::Error> {
        Self::try_from_vec(value)
    }
}

impl<T: Clone, const N: usize> TryFrom<&[T]> for ListaN<T, N> {
    type Error = ListaNOverflow;

    fn try_from(value: &[T]) -> Result<Self, Self::Error> {
        Self::try_from_lista(value)
    }
}

impl<T: Clone, const N: usize> TryFrom<&Vec<T>> for ListaN<T, N> {
    type Error = ListaNOverflow;

    fn try_from(value: &Vec<T>) -> Result<Self, Self::Error> {
        Self::try_from_lista(value)
    }
}

#[cfg(test)]
mod tests {
    use super::{ListaN, ListaNOverflow};

    #[test]
    fn empty_lista_has_no_readable_spare_slots() {
        let lista = ListaN::<i64, 8>::empty();
        assert!(lista.vacua());
        assert_eq!(lista.len(), 0);
        assert_eq!(lista.longitudo(), 0);
        assert_eq!(lista.get(0), None);
        assert_eq!(lista.get(7), None);
        assert!(lista.as_slice().is_empty());
    }

    #[test]
    fn initialized_prefix_is_readable_and_spare_slots_are_not() {
        let mut lista = ListaN::<i64, 8>::empty();
        lista.appende(11).unwrap();
        lista.appende(22).unwrap();

        assert_eq!(lista.len(), 2);
        assert_eq!(lista.longitudo(), 2);
        assert!(!lista.vacua());
        assert_eq!(lista.as_slice(), &[11, 22]);
        assert_eq!(lista.get(0), Some(&11));
        assert_eq!(lista.get(1), Some(&22));
        assert_eq!(lista.get(2), None);
        assert_eq!(lista.get(7), None);
    }

    #[test]
    fn append_overflow_is_recoverable_and_preserves_prefix() {
        let mut lista = ListaN::<i64, 2>::empty();
        lista.appende(1).unwrap();
        lista.appende(2).unwrap();

        let error = lista
            .appende(3)
            .expect_err("third element must exceed lista<i64, 2>");
        assert_eq!(
            error,
            ListaNOverflow {
                capacity: 2,
                attempted: 3,
            }
        );
        assert_eq!(lista.as_slice(), &[1, 2]);
    }

    #[test]
    fn is_empty_matches_vacua() {
        let empty = ListaN::<i64, 8>::empty();
        assert_eq!(empty.is_empty(), empty.vacua());
        assert!(empty.is_empty());
        let mut filled = ListaN::<i64, 8>::empty();
        filled.appende(1).unwrap();
        assert_eq!(filled.is_empty(), filled.vacua());
        assert!(!filled.is_empty());
    }

    #[test]
    fn zero_capacity_is_empty_only() {
        let mut lista = ListaN::<i64, 0>::empty();
        assert!(lista.vacua());
        assert_eq!(lista.get(0), None);
        let error = lista
            .appende(1)
            .expect_err("any element must exceed lista<i64, 0>");
        assert_eq!(
            error,
            ListaNOverflow {
                capacity: 0,
                attempted: 1,
            }
        );
        assert!(lista.vacua());
    }

    #[test]
    fn vec_conversions_copy_and_move_only_the_prefix() {
        let bounded = ListaN::<i32, 4>::try_from(vec![3, 5, 8]).unwrap();
        let copied: Vec<_> = (&bounded).into();
        assert_eq!(copied, vec![3, 5, 8]);
        assert_eq!(bounded.to_lista(), vec![3, 5, 8]);
        assert_eq!(bounded.to_vec(), vec![3, 5, 8]);

        let from_slice = ListaN::<i32, 4>::try_from_lista(&copied).unwrap();
        assert_eq!(from_slice, bounded);
        let via_slice: ListaN<_, 4> = copied.as_slice().try_into().unwrap();
        assert_eq!(via_slice, bounded);
        let via_vec_ref: ListaN<_, 4> = (&copied).try_into().unwrap();
        assert_eq!(via_vec_ref, bounded);

        let moved: Vec<_> = bounded.into();
        assert_eq!(moved, vec![3, 5, 8]);

        let round_trip: ListaN<_, 4> = moved.try_into().unwrap();
        assert_eq!(round_trip.as_slice(), &[3, 5, 8]);
    }

    #[test]
    fn vec_conversion_overflow_fails_closed() {
        let error = ListaN::<i32, 2>::try_from(vec![1, 2, 3])
            .expect_err("Vec longer than N must not be truncated");
        assert_eq!(
            error,
            ListaNOverflow {
                capacity: 2,
                attempted: 3,
            }
        );
    }

    #[test]
    fn no_default_bound_is_required_for_elements() {
        struct NoDefault(i32);

        let mut lista = ListaN::<NoDefault, 2>::empty();
        lista.appende(NoDefault(7)).unwrap();
        assert_eq!(lista.get(0).map(|value| value.0), Some(7));
        assert_eq!(lista.get(1).map(|value| value.0), None);
    }

    #[test]
    fn clone_and_equality_use_only_initialized_prefix() {
        let mut lista = ListaN::<i32, 4>::empty();
        lista.appende(13).unwrap();
        let clone = lista.clone();
        assert_eq!(clone, lista);
        assert_eq!(format!("{lista:?}"), "ListaN { n: 4, len: 1, items: [13] }");
    }
}
