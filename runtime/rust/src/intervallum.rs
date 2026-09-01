//! Faber `intervallum<T>` runtime — numeric interval with glyph-encoded inclusivity.

use crate::Tensor;

/// Endpoint inclusion policy declared at construction (`‥` vs `…` / `usque`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntervallumKind {
    /// Half-open `[initium, finis)`.
    Exclusive,
    /// Closed `[initium, finis]`.
    Inclusive,
}

/// Bindable numeric interval: two bounds of `T` plus an inclusivity tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Intervallum<T> {
    pub initium: T,
    pub finis: T,
    pub kind: IntervallumKind,
}

// ---------------------------------------------------------------------------
// Internal trait: abstracts over sized-integer operations for the walk impls
// ---------------------------------------------------------------------------

/// Sealed — sized-integer numeric helpers for `Intervallum` methods.
pub trait IntervallumNumeric: PartialOrd + Copy + Sized {
    const MAX: Self;
    const MIN: Self;

    fn one() -> Self;
    fn zero() -> Self;

    fn saturating_add(self, rhs: Self) -> Self;
    fn saturating_sub(self, rhs: Self) -> Self;

    /// Step one unit forward (toward +∞).  Wraps for unsigned underflow-safe
    /// walking; callers guard against MAX beforehand.
    fn walk_ascend(self) -> Self;

    /// Step one unit backward (toward −∞).  Wraps for unsigned underflow-safe
    /// walking; callers guard against MIN beforehand.
    fn walk_descend(self) -> Self;

    /// Convert to `i64` for cardinality / length returns.
    fn to_i64(self) -> i64;
}

macro_rules! impl_intervallum_numeric_signed {
    ($($ty:ty),+ $(,)?) => {
        $(impl IntervallumNumeric for $ty {
            const MAX: Self = <$ty>::MAX;
            const MIN: Self = <$ty>::MIN;

            fn one() -> Self { 1 }
            fn zero() -> Self { 0 }

            fn saturating_add(self, rhs: Self) -> Self { self.saturating_add(rhs) }
            fn saturating_sub(self, rhs: Self) -> Self { self.saturating_sub(rhs) }

            fn walk_ascend(self) -> Self { self + 1 }
            fn walk_descend(self) -> Self { self - 1 }

            fn to_i64(self) -> i64 { self as i64 }
        })+
    }
}

macro_rules! impl_intervallum_numeric_unsigned {
    ($($ty:ty),+ $(,)?) => {
        $(impl IntervallumNumeric for $ty {
            const MAX: Self = <$ty>::MAX;
            const MIN: Self = <$ty>::MIN;

            fn one() -> Self { 1 }
            fn zero() -> Self { 0 }

            fn saturating_add(self, rhs: Self) -> Self { self.saturating_add(rhs) }
            fn saturating_sub(self, rhs: Self) -> Self { self.saturating_sub(rhs) }

            fn walk_ascend(self) -> Self { self + 1 }
            fn walk_descend(self) -> Self { self.wrapping_sub(1) }

            fn to_i64(self) -> i64 { self as i64 }
        })+
    }
}

impl_intervallum_numeric_signed!(i8, i16, i32, i64);
impl_intervallum_numeric_unsigned!(u8, u16, u32, u64);

// ---------------------------------------------------------------------------
// Generic struct-level constructors
// ---------------------------------------------------------------------------

impl<T: PartialOrd + Copy> Intervallum<T> {
    pub fn exclusive(initium: T, finis: T) -> Self {
        Self {
            initium,
            finis,
            kind: IntervallumKind::Exclusive,
        }
    }

    pub fn inclusive(initium: T, finis: T) -> Self {
        Self {
            initium,
            finis,
            kind: IntervallumKind::Inclusive,
        }
    }

    /// Point containment (`intra`): honors the interval's declared inclusivity.
    ///
    /// For a directed span, `initium` is always included and `finis` is excluded
    /// in half-open mode regardless of whether the span is ascending or descending.
    /// Membership is the set of points between initium and finis respecting those
    /// per-endpoint inclusivity rules.
    pub fn continet(&self, value: &T) -> bool {
        let ascending = self.initium <= self.finis;
        match self.kind {
            IntervallumKind::Exclusive => {
                if ascending {
                    value >= &self.initium && value < &self.finis
                } else {
                    value <= &self.initium && value > &self.finis
                }
            }
            IntervallumKind::Inclusive => {
                if ascending {
                    value >= &self.initium && value <= &self.finis
                } else {
                    value <= &self.initium && value >= &self.finis
                }
            }
        }
    }
}

fn max_bound<T: PartialOrd + Copy>(a: T, b: T) -> T {
    if a >= b { a } else { b }
}

fn min_bound<T: PartialOrd + Copy>(a: T, b: T) -> T {
    if a <= b { a } else { b }
}

// ---------------------------------------------------------------------------
// Sized-integer directed walk + set operations (no Default bound needed)
// ---------------------------------------------------------------------------

impl<T: IntervallumNumeric> Intervallum<T> {
    /// Clamp `value` into this interval (refinement-target conversio).
    ///
    /// For descending spans, the valid range runs from `initium` (high) down to
    /// `finis` (low); clamp maps out-of-range values to the nearest valid endpoint.
    #[must_use]
    pub fn coercere(&self, value: T) -> T {
        if self.continet(&value) {
            return value;
        }
        let ascending = self.initium <= self.finis;
        let lo = min_bound(self.initium, self.finis);
        let hi = max_bound(self.initium, self.finis);

        // When the excluded `finis` is the outward bound of the valid range,
        // the nearest valid point is one step inside from it.
        let lo_valid = match self.kind {
            IntervallumKind::Exclusive if !ascending => lo.saturating_add(T::one()),
            _ => lo,
        };
        let hi_valid = match self.kind {
            IntervallumKind::Exclusive if ascending => hi.saturating_sub(T::one()),
            _ => hi,
        };

        if value < lo_valid {
            lo_valid
        } else if value > hi_valid {
            hi_valid
        } else {
            value
        }
    }

    /// Range-to-range clamp: each bound coerced into `target`; result inherits `target.kind`.
    #[must_use]
    pub fn coercere_intervallum(&self, target: &Self) -> Self {
        Self {
            initium: target.coercere(self.initium),
            finis: target.coercere(self.finis),
            kind: target.kind,
        }
    }

    /// Materialize interval values into an eager list (honors declared inclusivity).
    #[must_use]
    pub fn ad_lista(&self) -> Vec<T> {
        let ascending = self.initium <= self.finis;
        let mut out = Vec::new();
        let mut cursor = self.initium;
        if ascending {
            while match self.kind {
                IntervallumKind::Exclusive => cursor < self.finis,
                IntervallumKind::Inclusive => cursor <= self.finis,
            } {
                out.push(cursor);
                if cursor >= T::MAX {
                    break;
                }
                cursor = cursor.walk_ascend();
            }
        } else {
            while match self.kind {
                IntervallumKind::Exclusive => cursor > self.finis,
                IntervallumKind::Inclusive => cursor >= self.finis,
            } {
                out.push(cursor);
                if cursor <= T::MIN {
                    break;
                }
                cursor = cursor.walk_descend();
            }
        }
        out
    }

    /// Lazy interval walk — returns an iterator over the values in this interval.
    ///
    /// Honors the declared inclusivity and direction.  Use instead of `ad_lista`
    /// when you do not need the full materialized `Vec`.
    #[must_use]
    pub fn ambula(&self) -> IntervallumWalk<T> {
        IntervallumWalk {
            interval: *self,
            cursor: self.initium,
            exhausted: false,
        }
    }

    /// Discrete span count (same cardinality as `ad_lista()`).
    #[must_use]
    pub fn longitudo(&self) -> i64 {
        let span = if self.initium <= self.finis {
            self.finis.saturating_sub(self.initium)
        } else {
            self.initium.saturating_sub(self.finis)
        };
        let span_i64 = span.to_i64();
        match self.kind {
            IntervallumKind::Exclusive => span_i64,
            IntervallumKind::Inclusive => span_i64.saturating_add(1),
        }
    }

    /// Interval intersection; `None` when disjoint (distinct from range clamp).
    ///
    /// Operates on point sets: the result is the intersection of the two point
    /// sets, returned as an inclusive interval in the direction of the left operand.
    #[must_use]
    pub fn inter(self, other: Self) -> Option<Self> {
        let lo_a = min_bound(self.initium, self.finis);
        let hi_a = max_bound(self.initium, self.finis);
        let lo_b = min_bound(other.initium, other.finis);
        let hi_b = max_bound(other.initium, other.finis);

        let lo = max_bound(lo_a, lo_b);
        let hi = min_bound(hi_a, hi_b);

        if lo > hi {
            return None;
        }

        // Find first point in the overlapping region that belongs to both intervals.
        let mut new_lo = lo;
        loop {
            if self.continet(&new_lo) && other.continet(&new_lo) {
                break;
            }
            if new_lo >= hi {
                return None;
            }
            new_lo = new_lo.saturating_add(T::one());
        }

        // Find last point in the overlapping region that belongs to both intervals.
        let mut new_hi = hi;
        loop {
            if self.continet(&new_hi) && other.continet(&new_hi) {
                break;
            }
            if new_hi <= new_lo {
                return None;
            }
            new_hi = new_hi.saturating_sub(T::one());
        }

        // Result direction follows left operand.
        let left_descending = self.initium > self.finis;
        let (initium, finis) = if left_descending {
            (new_hi, new_lo)
        } else {
            (new_lo, new_hi)
        };

        Some(Self {
            initium,
            finis,
            kind: IntervallumKind::Inclusive,
        })
    }

    /// Interval union when overlap or adjacent; `None` when a gap separates them.
    ///
    /// Operates on point sets: the result is the union of the two point sets,
    /// returned as an inclusive interval in the direction of the left operand.
    #[must_use]
    pub fn union(self, other: Self) -> Option<Self> {
        if self.inter(other).is_none() && !self.touches(other) {
            return None;
        }

        let lo_a = min_bound(self.initium, self.finis);
        let hi_a = max_bound(self.initium, self.finis);
        let lo_b = min_bound(other.initium, other.finis);
        let hi_b = max_bound(other.initium, other.finis);

        let lo = min_bound(lo_a, lo_b);
        let hi = max_bound(hi_a, hi_b);

        // Find first point from the union that is in at least one interval.
        let mut new_lo = lo;
        loop {
            if self.continet(&new_lo) || other.continet(&new_lo) {
                break;
            }
            new_lo = new_lo.saturating_add(T::one());
        }

        // Find last point from the union that is in at least one interval.
        let mut new_hi = hi;
        loop {
            if self.continet(&new_hi) || other.continet(&new_hi) {
                break;
            }
            new_hi = new_hi.saturating_sub(T::one());
        }

        let left_descending = self.initium > self.finis;
        let (initium, finis) = if left_descending {
            (new_hi, new_lo)
        } else {
            (new_lo, new_hi)
        };

        Some(Self {
            initium,
            finis,
            kind: IntervallumKind::Inclusive,
        })
    }

    /// Whether two intervals touch (adjacent without gap), so their union is contiguous.
    fn touches(self, other: Self) -> bool {
        // Same-direction endpoint equality (e.g. self.finis meets other.initium).
        if self.finis == other.initium
            && (self.continet(&other.initium) || other.continet(&self.finis))
        {
            return true;
        }
        if other.finis == self.initium
            && (other.continet(&self.initium) || self.continet(&other.finis))
        {
            return true;
        }

        // Diagonal adjacency: a descending span's high end may be consecutive
        // with an ascending span's low end, or vice versa.
        //
        // Compute the actual valid bounds (the outermost point each interval
        // includes) before checking whether they're adjacent.
        let lo_a = min_bound(self.initium, self.finis);
        let hi_a = max_bound(self.initium, self.finis);
        let lo_b = min_bound(other.initium, other.finis);
        let hi_b = max_bound(other.initium, other.finis);

        let actual_hi_a = if self.continet(&hi_a) {
            hi_a
        } else {
            // hi_a is the excluded finis endpoint.
            hi_a.saturating_sub(T::one())
        };
        let actual_lo_b = if other.continet(&lo_b) {
            lo_b
        } else {
            lo_b.saturating_add(T::one())
        };
        if actual_hi_a.saturating_add(T::one()) == actual_lo_b {
            return true;
        }

        let actual_hi_b = if other.continet(&hi_b) {
            hi_b
        } else {
            hi_b.saturating_sub(T::one())
        };
        let actual_lo_a = if self.continet(&lo_a) {
            lo_a
        } else {
            lo_a.saturating_add(T::one())
        };
        if actual_hi_b.saturating_add(T::one()) == actual_lo_a {
            return true;
        }

        false
    }
}

// ---------------------------------------------------------------------------
// Methods requiring Default (Tensor::linea needs T: Default)
// ---------------------------------------------------------------------------

impl<T: IntervallumNumeric + Default> Intervallum<T> {
    /// Materialize interval values into a 1-d tensor (honors declared inclusivity).
    #[must_use]
    pub fn ad_tensor(&self) -> Tensor<T> {
        Tensor::linea(self.ad_lista())
    }
}

// ---------------------------------------------------------------------------
// Expand helpers: lazy iterator walk over interval values
// ---------------------------------------------------------------------------

/// Iterator returned by [`Intervallum::ambula`].
///
/// Yields each discrete value in the interval, honoring direction and inclusivity.
#[derive(Debug, Clone)]
pub struct IntervallumWalk<T> {
    interval: Intervallum<T>,
    cursor: T,
    /// Set after the last value is yielded. Required when the endpoint is
    /// `T::MAX`/`T::MIN` so inclusive walks cannot compare "past" the bound.
    exhausted: bool,
}

impl<T: IntervallumNumeric> Iterator for IntervallumWalk<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        if self.exhausted {
            return None;
        }

        let ascending = self.interval.initium <= self.interval.finis;
        let done = if ascending {
            match self.interval.kind {
                IntervallumKind::Exclusive => self.cursor >= self.interval.finis,
                IntervallumKind::Inclusive => self.cursor > self.interval.finis,
            }
        } else {
            match self.interval.kind {
                IntervallumKind::Exclusive => self.cursor <= self.interval.finis,
                IntervallumKind::Inclusive => self.cursor < self.interval.finis,
            }
        };

        if done {
            self.exhausted = true;
            return None;
        }

        let current = self.cursor;
        if ascending {
            if current >= T::MAX || current >= self.interval.finis {
                // Last yield — no representable successor (or endpoint reached).
                self.exhausted = true;
            } else {
                self.cursor = self.cursor.walk_ascend();
            }
        } else if current <= T::MIN || current <= self.interval.finis {
            self.exhausted = true;
        } else {
            self.cursor = self.cursor.walk_descend();
        }
        Some(current)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // Approximate: at most what longitudo reports.
        let len: i64 = self.interval.longitudo();
        if len <= 0_i64 {
            return (0, Some(0));
        }
        // Bound to usize::MAX for safety.
        let len_usize: usize = len as usize;
        (0, Some(len_usize))
    }
}
