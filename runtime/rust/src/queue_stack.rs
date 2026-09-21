//! Faber `queue<T[,N]>` / `stack<T[,N]>` runtime carrier family.
//!
//! One parameterized storage/live-window mechanism — [`Fenestra`] — carries
//! both access disciplines and both capacity postures. The four public shapes
//! the Rust lane names are thin aliases over it:
//!
//! | Faber type | Rust carrier | Discipline | Capacity |
//! | --- | --- | --- | --- |
//! | `queue<T>` | [`Queue`] | FIFO | unbounded |
//! | `stack<T>` | [`Stack`] | LIFO | unbounded |
//! | `queue<T, N>` | [`QueueN`] | FIFO | bound `N` |
//! | `stack<T, N>` | [`StackN`] | LIFO | bound `N` |
//!
//! The settled access asymmetry (operator need `4d3d7eaa` item C, C4) is
//! stated once in the core: the `N` forms fail closed — an append at capacity
//! leaves the live window unchanged and returns [`QueueStackOverflow`] on the
//! recoverable channel — while `pop()` on an empty carrier is `Option<T>`
//! (`T ∪ nihil`), never an alternate exit.
//!
//! Storage is a [`VecDeque`], so both removals are O(1): FIFO takes the front,
//! LIFO takes the back. The live window is `0..len`; there is no readable
//! spare storage and no capacity-sized allocation up front.
//!
//! # Examples
//!
//! ```
//! use faber::{Queue, QueueN, QueueStackOverflow, Stack, StackN};
//!
//! let mut queue: Queue<i64> = Queue::from(vec![1, 2]);
//! queue.appende(3);
//! assert_eq!(queue.decapita(), Some(1));
//!
//! let mut stack: Stack<i64> = Stack::from(vec![1, 2]);
//! assert_eq!(stack.detrahe(), Some(2));
//!
//! let sized: StackN<i64, 4> = StackN::try_from_vec(vec![9])?;
//! assert_eq!(sized.longitudo(), 1);
//!
//! let mut bounded: QueueN<i64, 2> = QueueN::empty();
//! bounded.appende(1)?;
//! bounded.appende(2)?;
//! assert_eq!(
//!     bounded.appende(3),
//!     Err(QueueStackOverflow {
//!         capacity: 2,
//!         attempted: 3,
//!     }),
//! );
//! # Ok::<(), QueueStackOverflow>(())
//! ```
//!
//! # Elementwise law
//!
//! Numeric arithmetic over these carriers is elementwise with scalar
//! broadcasting, exactly as [`crate::ListaN`]'s list law and the MIR runner's
//! `array_elementwise` do it — one shared body, not a per-shape copy:
//!
//! - [`Fenestra::combina`] pairs two carriers' live windows positionally. Unequal
//!   live-window lengths are a hard error ([`QueueStackError::Longitudo`]); the
//!   result keeps the carrier kind, and its declared capacity must be
//!   `min(N1, N2)` ([`QueueStackError::Capacitas`] otherwise). The caller names
//!   the result capacity, because stable Rust cannot compute `min` in a type
//!   position; the runtime enforces the law rather than trusting the caller.
//! - [`Fenestra::sparge`] / [`Fenestra::sparge_sinistra`] broadcast one scalar
//!   against either side and preserve the receiver's kind and capacity.
//!
//! Removing `RUST_QUEUE_STACK_UNSUPPORTED_ISSUE` from the Radix Rust emitter is
//! a separate trigger-gated follow-on; this module is the carrier it needs.

use std::collections::VecDeque;
use std::marker::PhantomData;

/// Access discipline: the end of the live window a removal takes.
pub trait Disciplina {
    /// Whether removal takes the oldest element (the front) rather than newest.
    const A_FRONTE: bool;
}

/// FIFO discipline — `queue`.
pub struct Fifo;

/// LIFO discipline — `stack`.
pub struct Lifo;

impl Disciplina for Fifo {
    const A_FRONTE: bool = true;
}

impl Disciplina for Lifo {
    const A_FRONTE: bool = false;
}

/// Capacity posture: unbounded, or a type-level bound `N`.
pub trait Capacitas {
    /// The declared capacity, or `None` for the unbounded posture.
    #[must_use]
    fn limes() -> Option<usize>;
}

/// Unbounded capacity posture — bare `queue<T>` / `stack<T>`.
pub struct Infinita;

/// Bounded capacity posture — `queue<T, N>` / `stack<T, N>`.
pub struct Finita<const N: usize>;

impl Capacitas for Infinita {
    fn limes() -> Option<usize> {
        None
    }
}

impl<const N: usize> Capacitas for Finita<N> {
    fn limes() -> Option<usize> {
        Some(N)
    }
}

/// Recoverable overflow when a bounded append would exceed declared `N`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueStackOverflow {
    /// Declared capacity `N`.
    pub capacity: usize,
    /// Attempted total length that did not fit.
    pub attempted: usize,
}

/// Elementwise law violation over a carrier pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueStackError {
    /// Same-length law: the operands' live windows differ in length.
    Longitudo {
        /// Left operand live-window length.
        left: usize,
        /// Right operand live-window length.
        right: usize,
    },
    /// Capacity law: the declared result capacity is not `min(N1, N2)`.
    Capacitas {
        /// `min(N1, N2)`, `None` when both operands are unbounded.
        expected: Option<usize>,
        /// Capacity the caller declared on the result carrier.
        declared: Option<usize>,
    },
}

/// `min(N1, N2)` with `None` read as unbounded (the widest posture).
fn min_capacitas(left: Option<usize>, right: Option<usize>) -> Option<usize> {
    match (left, right) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
    }
}

/// Shared storage and live window behind all four public shapes.
///
/// `D` selects the removal end ([`Fifo`] / [`Lifo`]); `C` selects the capacity
/// posture ([`Infinita`] / [`Finita<N>`]). Iteration follows the access
/// discipline: a queue yields front (oldest) first, a stack yields back
/// (newest) first, so iteration order matches removal order.
pub struct Fenestra<T, D: Disciplina, C: Capacitas> {
    items: VecDeque<T>,
    marker: PhantomData<(D, C)>,
}

impl<T, D: Disciplina, C: Capacitas> Fenestra<T, D, C> {
    /// Empty carrier: live window `0..0`.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            items: VecDeque::new(),
            marker: PhantomData,
        }
    }

    /// Shared append body. The bounded posture fails closed at capacity;
    /// the unbounded posture has no capacity to exceed.
    fn push(&mut self, value: T) -> Result<(), QueueStackOverflow> {
        if let Some(limit) = C::limes()
            && self.items.len() >= limit
        {
            return Err(QueueStackOverflow {
                capacity: limit,
                attempted: self.items.len().saturating_add(1),
            });
        }
        self.items.push_back(value);
        Ok(())
    }

    /// Remove and return the element the discipline selects, or `None` when
    /// the live window is empty.
    pub fn pop(&mut self) -> Option<T> {
        if D::A_FRONTE {
            self.items.pop_front()
        } else {
            self.items.pop_back()
        }
    }

    /// Runtime live-window length, never declared capacity.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// `longitudo` — the live-window length as `i64`.
    ///
    /// # Panics
    ///
    /// Panics when the declared capacity permits a length above [`i64::MAX`].
    #[must_use]
    pub fn longitudo(&self) -> i64 {
        i64::try_from(self.items.len()).expect("queue/stack live length must fit in i64")
    }

    /// Whether the live window is empty.
    #[must_use]
    pub fn vacua(&self) -> bool {
        self.items.is_empty()
    }

    /// Rust empty predicate. Same as [`Self::vacua`].
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.vacua()
    }

    /// Declared capacity of this posture, not the live-window length.
    #[must_use]
    pub fn capacitas(&self) -> Option<usize> {
        C::limes()
    }

    /// Read a live-window element. Positions at or past `len` return `None`.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&T> {
        self.items.get(index)
    }

    /// Iterate the live window in access order (oldest first for a queue,
    /// newest first for a stack).
    #[must_use]
    pub fn iter(&self) -> Iter<'_, T> {
        if D::A_FRONTE {
            Iter::Fronte(self.items.iter())
        } else {
            Iter::Tergo(self.items.iter().rev())
        }
    }

    /// Elementwise combination of two live windows of the same kind.
    ///
    /// `R` is the declared result capacity and must be `min(C, C2)`; for equal
    /// bounded postures that is the bound itself, and for two unbounded
    /// operands it is unbounded. Unequal live-window lengths fail closed.
    /// Kind preservation is the carrier type: both operands share the same
    /// discipline `D`, so `queue ⊗ stack` has no instance.
    ///
    /// # Errors
    ///
    /// Returns [`QueueStackError::Capacitas`] when the declared result
    /// capacity is not `min(N1, N2)`, and [`QueueStackError::Longitudo`] when
    /// the live windows differ in length.
    pub fn combina<U, C2, R, F>(
        &self,
        other: &Fenestra<T, D, C2>,
        f: F,
    ) -> Result<Fenestra<U, D, R>, QueueStackError>
    where
        C2: Capacitas,
        R: Capacitas,
        F: Fn(&T, &T) -> U,
    {
        let expected = min_capacitas(C::limes(), C2::limes());
        if R::limes() != expected {
            return Err(QueueStackError::Capacitas {
                expected,
                declared: R::limes(),
            });
        }
        if self.items.len() != other.items.len() {
            return Err(QueueStackError::Longitudo {
                left: self.items.len(),
                right: other.items.len(),
            });
        }
        Ok(Fenestra {
            items: self
                .items
                .iter()
                .zip(other.items.iter())
                .map(|(left, right)| f(left, right))
                .collect(),
            marker: PhantomData,
        })
    }

    /// Broadcast one scalar against the carrier: `carrier ⊗ scalar`.
    /// The receiver's kind and capacity are preserved.
    pub fn sparge<U, F>(&self, scalar: &T, f: F) -> Fenestra<U, D, C>
    where
        F: Fn(&T, &T) -> U,
    {
        Fenestra {
            items: self.items.iter().map(|value| f(value, scalar)).collect(),
            marker: PhantomData,
        }
    }

    /// Broadcast one scalar against the carrier from the left: `scalar ⊗ carrier`.
    /// The collection's kind and capacity are preserved.
    pub fn sparge_sinistra<U, F>(scalar: &T, collection: &Self, f: F) -> Fenestra<U, D, C>
    where
        F: Fn(&T, &T) -> U,
    {
        Fenestra {
            items: collection
                .items
                .iter()
                .map(|value| f(scalar, value))
                .collect(),
            marker: PhantomData,
        }
    }

    /// Copy the live window into an unbounded `Vec<T>` (the bare `lista<T>`
    /// carrier).
    #[must_use]
    pub fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.items.iter().cloned().collect()
    }

    /// Move the live window into an unbounded `Vec<T>`.
    #[must_use]
    pub fn into_vec(self) -> Vec<T> {
        self.items.into_iter().collect()
    }
}

impl<T, D: Disciplina> Fenestra<T, D, Infinita> {
    /// Append one element. The unbounded posture has no capacity, so the
    /// language's `appende` is infallible here.
    ///
    /// # Panics
    ///
    /// Unreachable: [`Self::push`] cannot overflow without a declared limit.
    pub fn appende(&mut self, value: T) {
        self.push(value)
            .expect("unbounded queue/stack append cannot overflow");
    }
}

impl<T, D: Disciplina, const N: usize> Fenestra<T, D, Finita<N>> {
    /// Append one element, failing closed at declared capacity `N`.
    ///
    /// # Errors
    ///
    /// Returns [`QueueStackOverflow`] when the live window is already `N`
    /// wide. The live window is left unchanged; nothing is truncated.
    pub fn appende(&mut self, value: T) -> Result<(), QueueStackOverflow> {
        self.push(value)
    }

    /// Construct from a `Vec<T>`, failing closed when it is longer than `N`.
    /// The oversized input is not truncated.
    ///
    /// # Errors
    ///
    /// Returns [`QueueStackOverflow`] when `values.len() > N`.
    pub fn try_from_vec(values: Vec<T>) -> Result<Self, QueueStackOverflow> {
        let attempted = values.len();
        if attempted > N {
            return Err(QueueStackOverflow {
                capacity: N,
                attempted,
            });
        }
        Ok(Self {
            items: values.into_iter().collect(),
            marker: PhantomData,
        })
    }
}

impl<T, C: Capacitas> Fenestra<T, Fifo, C> {
    /// `decapita` — remove and return the oldest element (FIFO).
    #[must_use]
    pub fn decapita(&mut self) -> Option<T> {
        self.pop()
    }
}

impl<T, C: Capacitas> Fenestra<T, Lifo, C> {
    /// `detrahe` — remove and return the newest element (LIFO).
    #[must_use]
    pub fn detrahe(&mut self) -> Option<T> {
        self.pop()
    }
}

impl<T, D: Disciplina> From<Vec<T>> for Fenestra<T, D, Infinita> {
    fn from(values: Vec<T>) -> Self {
        Self {
            items: values.into_iter().collect(),
            marker: PhantomData,
        }
    }
}

impl<T: Clone, D: Disciplina, C: Capacitas> Clone for Fenestra<T, D, C> {
    fn clone(&self) -> Self {
        Self {
            items: self.items.clone(),
            marker: PhantomData,
        }
    }
}

impl<T: PartialEq, D: Disciplina, C: Capacitas> PartialEq for Fenestra<T, D, C> {
    fn eq(&self, other: &Self) -> bool {
        self.items == other.items
    }
}

impl<T: Eq, D: Disciplina, C: Capacitas> Eq for Fenestra<T, D, C> {}

impl<T, D: Disciplina, C: Capacitas> Default for Fenestra<T, D, C> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<T: std::fmt::Debug, D: Disciplina, C: Capacitas> std::fmt::Debug for Fenestra<T, D, C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Fenestra")
            .field("disciplina", &if D::A_FRONTE { "fifo" } else { "lifo" })
            .field("capacitas", &C::limes())
            .field("len", &self.items.len())
            .field("items", &self.items)
            .finish()
    }
}

impl<'a, T, D: Disciplina, C: Capacitas> IntoIterator for &'a Fenestra<T, D, C> {
    type Item = &'a T;
    type IntoIter = Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Borrowed iterator over a live window in access order.
pub enum Iter<'a, T> {
    /// FIFO access order: front (oldest) to back (newest).
    Fronte(std::collections::vec_deque::Iter<'a, T>),
    /// LIFO access order: back (newest) to front (oldest).
    Tergo(std::iter::Rev<std::collections::vec_deque::Iter<'a, T>>),
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Fronte(inner) => inner.next(),
            Self::Tergo(inner) => inner.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Fronte(inner) => inner.size_hint(),
            Self::Tergo(inner) => inner.size_hint(),
        }
    }
}

impl<T> ExactSizeIterator for Iter<'_, T> {}

/// Unbounded FIFO carrier — Faber `queue<T>`.
pub type Queue<T> = Fenestra<T, Fifo, Infinita>;

/// Unbounded LIFO carrier — Faber `stack<T>`.
pub type Stack<T> = Fenestra<T, Lifo, Infinita>;

/// Bounded FIFO carrier — Faber `queue<T, N>`.
pub type QueueN<T, const N: usize> = Fenestra<T, Fifo, Finita<N>>;

/// Bounded LIFO carrier — Faber `stack<T, N>`.
pub type StackN<T, const N: usize> = Fenestra<T, Lifo, Finita<N>>;

#[cfg(test)]
mod tests {
    use super::{
        Disciplina, Fenestra, Fifo, Infinita, Lifo, Queue, QueueN, QueueStackError,
        QueueStackOverflow, Stack, StackN,
    };

    /// Both disciplines run this one body; only the removal end differs.
    fn drain<D: Disciplina>() -> Vec<i64> {
        let mut carrier: Fenestra<i64, D, Infinita> = Fenestra::empty();
        for value in [1, 2, 3] {
            carrier.appende(value);
        }
        let mut popped = Vec::new();
        while let Some(value) = carrier.pop() {
            popped.push(value);
        }
        popped
    }

    #[test]
    fn one_shared_body_serves_fifo_and_lifo() {
        assert_eq!(drain::<Fifo>(), vec![1, 2, 3]);
        assert_eq!(drain::<Lifo>(), vec![3, 2, 1]);
    }

    #[test]
    fn queue_pops_oldest_first_and_tracks_the_live_window() {
        let mut queue: Queue<i64> = Queue::from(vec![1, 2, 3]);
        assert_eq!(queue.decapita(), Some(1));
        assert_eq!(queue.decapita(), Some(2));
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.longitudo(), 1);
        assert_eq!(queue.decapita(), Some(3));
        assert!(queue.vacua());
        assert_eq!(queue.capacitas(), None);
    }

    #[test]
    fn stack_pops_newest_first() {
        let mut stack: Stack<i64> = Stack::from(vec![1, 2, 3]);
        assert_eq!(stack.detrahe(), Some(3));
        assert_eq!(stack.detrahe(), Some(2));
        assert_eq!(stack.detrahe(), Some(1));
        assert_eq!(stack.detrahe(), None);
    }

    #[test]
    fn empty_pop_is_none_on_every_shape() {
        assert_eq!(Queue::<i64>::empty().decapita(), None);
        assert_eq!(Stack::<i64>::empty().detrahe(), None);
        assert_eq!(QueueN::<i64, 4>::empty().decapita(), None);
        assert_eq!(StackN::<i64, 4>::empty().detrahe(), None);
    }

    #[test]
    fn bounded_append_overflow_uses_the_error_channel() {
        let mut queue: QueueN<i64, 2> = QueueN::empty();
        queue.appende(7).unwrap();
        queue.appende(8).unwrap();
        let error = queue
            .appende(9)
            .expect_err("third element must exceed queue<i64, 2>");
        assert_eq!(
            error,
            QueueStackOverflow {
                capacity: 2,
                attempted: 3,
            }
        );
        assert_eq!(queue.to_vec(), vec![7, 8]);

        let mut stack: StackN<i64, 2> = StackN::empty();
        stack.appende(1).unwrap();
        stack.appende(2).unwrap();
        assert_eq!(
            stack.appende(3),
            Err(QueueStackOverflow {
                capacity: 2,
                attempted: 3,
            })
        );
        assert_eq!(stack.to_vec(), vec![1, 2]);

        let mut zero: StackN<i64, 0> = StackN::empty();
        assert_eq!(
            zero.appende(1),
            Err(QueueStackOverflow {
                capacity: 0,
                attempted: 1,
            })
        );
        assert!(zero.vacua());
    }

    #[test]
    fn unbounded_append_has_no_overflow_channel() {
        let mut queue: Queue<u8> = Queue::empty();
        let mut stack: Stack<u8> = Stack::empty();
        for value in 0..=u8::MAX {
            queue.appende(value);
            stack.appende(value);
        }
        assert_eq!(queue.longitudo(), 256);
        assert_eq!(stack.longitudo(), 256);
        assert_eq!(queue.capacitas(), None);
    }

    #[test]
    fn bounded_construction_fails_closed_past_n() {
        assert_eq!(
            QueueN::<i64, 2>::try_from_vec(vec![1, 2, 3]),
            Err(QueueStackOverflow {
                capacity: 2,
                attempted: 3,
            })
        );
        let sized = QueueN::<i64, 2>::try_from_vec(vec![1, 2]).unwrap();
        assert_eq!(sized.to_vec(), vec![1, 2]);
        assert_eq!(sized.capacitas(), Some(2));
        assert_eq!(StackN::<i64, 0>::try_from_vec(Vec::new()).unwrap().len(), 0);
    }

    #[test]
    fn broadcast_applies_the_scalar_on_either_side() {
        let queue: Queue<i64> = Queue::from(vec![1, 2, 3]);
        let scaled: Queue<i64> = queue.sparge(&10, |value, scalar| value * scalar);
        assert_eq!(scaled.to_vec(), vec![10, 20, 30]);
        let from_left: Queue<i64> =
            Queue::sparge_sinistra(&10, &queue, |scalar, value| scalar - value);
        assert_eq!(from_left.to_vec(), vec![9, 8, 7]);

        let stack: Stack<i64> = Stack::from(vec![1, 2, 3]);
        let scaled: Stack<i64> = stack.sparge(&2, |value, scalar| value + scalar);
        assert_eq!(scaled.to_vec(), vec![3, 4, 5]);

        let bounded: QueueN<i64, 4> = QueueN::try_from_vec(vec![1, 2, 3]).unwrap();
        let scaled: QueueN<i64, 4> = bounded.sparge(&3, |value, scalar| value * scalar);
        assert_eq!(scaled.capacitas(), Some(4));
        assert_eq!(scaled.to_vec(), vec![3, 6, 9]);

        let bounded: StackN<i64, 4> = StackN::try_from_vec(vec![1, 2, 3]).unwrap();
        let scaled: StackN<i64, 4> =
            StackN::sparge_sinistra(&3, &bounded, |scalar, value| scalar * value);
        assert_eq!(scaled.capacitas(), Some(4));
        assert_eq!(scaled.to_vec(), vec![3, 6, 9]);
    }

    #[test]
    fn same_length_arithmetic_pairs_the_live_window() {
        let left: Queue<i64> = Queue::from(vec![1, 2, 3]);
        let right: Queue<i64> = Queue::from(vec![10, 20, 30]);
        let sum: Queue<i64> = left
            .combina(&right, |a, b| a + b)
            .expect("equal live windows");
        assert_eq!(sum.to_vec(), vec![11, 22, 33]);

        let left: StackN<i64, 4> = StackN::try_from_vec(vec![4, 5]).unwrap();
        let right: StackN<i64, 4> = StackN::try_from_vec(vec![6, 7]).unwrap();
        let difference: StackN<i64, 4> = left
            .combina(&right, |a, b| a - b)
            .expect("equal live windows");
        assert_eq!(difference.to_vec(), vec![-2, -2]);
    }

    #[test]
    fn elementwise_length_mismatch_is_a_law_error() {
        let left: Queue<i64> = Queue::from(vec![1, 2, 3]);
        let shorter: Queue<i64> = Queue::from(vec![1, 2]);
        let mismatch: Result<Queue<i64>, _> = left.combina(&shorter, |a, b| a + b);
        assert_eq!(
            mismatch,
            Err(QueueStackError::Longitudo { left: 3, right: 2 })
        );
    }

    #[test]
    fn elementwise_preserves_kind_and_narrows_to_min_capacity() {
        let wide: QueueN<i64, 8> = QueueN::try_from_vec(vec![1, 2, 3]).unwrap();
        let narrow: QueueN<i64, 3> = QueueN::try_from_vec(vec![10, 20, 30]).unwrap();

        let sum: QueueN<i64, 3> = wide
            .combina(&narrow, |a, b| a + b)
            .expect("min(8, 3) is the result capacity");
        assert_eq!(sum.capacitas(), Some(3));
        assert_eq!(sum.longitudo(), 3);
        assert_eq!(sum.to_vec(), vec![11, 22, 33]);

        // A declared result capacity that is not `min(N1, N2)` is a law
        // violation, even though the values would fit.
        let widened: Result<QueueN<i64, 8>, _> = wide.combina(&narrow, |a, b| a + b);
        assert_eq!(
            widened,
            Err(QueueStackError::Capacitas {
                expected: Some(3),
                declared: Some(8),
            })
        );

        // Mixed bounded/unbounded takes the bounded posture as the minimum.
        let unbounded: Queue<i64> = Queue::from(vec![4, 5, 6]);
        let mixed: QueueN<i64, 3> = narrow
            .combina(&unbounded, |a, b| a + b)
            .expect("min(3, unbounded) is 3");
        assert_eq!(mixed.capacitas(), Some(3));

        // Kind preservation is the carrier type: a queue result stays a queue,
        // a stack result stays a stack.
        let wide: StackN<i64, 8> = StackN::try_from_vec(vec![1, 2, 3]).unwrap();
        let narrow: StackN<i64, 3> = StackN::try_from_vec(vec![10, 20, 30]).unwrap();
        let product: StackN<i64, 3> = wide
            .combina(&narrow, |a, b| a * b)
            .expect("min(8, 3) is the result capacity");
        assert_eq!(product.to_vec(), vec![10, 40, 90]);
    }

    #[test]
    fn iteration_follows_the_access_discipline() {
        let queue: Queue<i64> = Queue::from(vec![1, 2, 3]);
        assert_eq!(queue.iter().copied().collect::<Vec<_>>(), vec![1, 2, 3]);
        assert_eq!(
            (&queue).into_iter().copied().collect::<Vec<_>>(),
            vec![1, 2, 3]
        );

        let stack: Stack<i64> = Stack::from(vec![1, 2, 3]);
        assert_eq!(stack.iter().copied().collect::<Vec<_>>(), vec![3, 2, 1]);
        assert_eq!(stack.get(0), Some(&1));
    }

    #[test]
    fn carriers_compare_clone_and_render() {
        let queue: QueueN<i64, 4> = QueueN::try_from_vec(vec![1, 2]).unwrap();
        assert_eq!(queue, queue.clone());
        assert_ne!(queue, QueueN::<i64, 4>::try_from_vec(vec![1]).unwrap());
        assert_eq!(
            format!("{queue:?}"),
            "Fenestra { disciplina: \"fifo\", capacitas: Some(4), len: 2, items: [1, 2] }"
        );
        assert_eq!(queue.iter().len(), 2);
    }
}
