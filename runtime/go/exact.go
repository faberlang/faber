package rt

// The Faber `exact` carrier for generated Go programs (codegen-readability
// T1-G2): the sign-magnitude exact intermediate of d11-6 rulings 9, 10, 14,
// 17, 19b, 22.
//
// Go has no 128-bit integer. An exact intermediate is a sign-magnitude pair
// `ExactX{neg, m}` whose value always lies in `[-2^63, 2^64 - 1]`; every
// operation checks that range with `math/bits` carries and panics past it.
// `math/bits` covers add/sub/mul; division, remainder and shifts work on the
// magnitude and then apply the floor correction. A width limit (the Store and
// Clamp families) applies only where a value leaves the carrier into a bounded
// slot; nothing clamps or traps per operation. Every name is `Exact<Name>`,
// spelled by the compiler's canonical helper table.

import (
	"math"
	"math/bits"
	"strconv"
)

type ExactX struct {
	neg bool
	m   uint64
}

func ExactMk(neg bool, m uint64) ExactX {
	if m == 0 {
		return ExactX{}
	}
	if neg && m > 1<<63 {
		panic("numerus overflow")
	}
	return ExactX{neg, m}
}

func ExactI(v int64) ExactX {
	if v < 0 {
		return ExactX{true, -uint64(v)}
	}
	return ExactX{false, uint64(v)}
}

func ExactU(v uint64) ExactX { return ExactX{false, v} }

func ExactNeg(a ExactX) ExactX { return ExactMk(!a.neg, a.m) }

func ExactAddS(a ExactX, bneg bool, bm uint64) ExactX {
	if a.neg == bneg {
		s, c := bits.Add64(a.m, bm, 0)
		if c != 0 {
			panic("numerus overflow")
		}
		return ExactMk(a.neg, s)
	}
	if a.m >= bm {
		return ExactMk(a.neg, a.m-bm)
	}
	return ExactMk(bneg, bm-a.m)
}

func ExactAdd(a ExactX, b ExactX) ExactX { return ExactAddS(a, b.neg, b.m) }

func ExactSub(a ExactX, b ExactX) ExactX { return ExactAddS(a, !b.neg, b.m) }

func ExactMul(a ExactX, b ExactX) ExactX {
	hi, lo := bits.Mul64(a.m, b.m)
	if hi != 0 {
		panic("numerus overflow")
	}
	return ExactMk(a.neg != b.neg, lo)
}

func ExactDiv(a ExactX, b ExactX) ExactX {
	if b.m == 0 {
		panic("numerus division failed")
	}
	q, r := a.m/b.m, a.m%b.m
	if a.neg == b.neg {
		return ExactMk(false, q)
	}
	if r != 0 {
		q++
	}
	return ExactMk(true, q)
}

func ExactMod(a ExactX, b ExactX) ExactX {
	if b.m == 0 {
		panic("numerus division failed")
	}
	r := a.m % b.m
	if r != 0 && a.neg != b.neg {
		r = b.m - r
	}
	return ExactMk(b.neg, r)
}

func ExactShl(a ExactX, n ExactX) ExactX {
	if n.neg {
		panic("negative shift count")
	}
	if a.m == 0 {
		return ExactX{}
	}
	if n.m > 64 || uint64(bits.LeadingZeros64(a.m)) < n.m {
		panic("numerus overflow")
	}
	return ExactMk(a.neg, a.m<<n.m)
}

func ExactShr(a ExactX, n ExactX) ExactX {
	if n.neg {
		panic("negative shift count")
	}
	if n.m >= 64 {
		if a.neg {
			return ExactX{true, 1}
		}
		return ExactX{}
	}
	m := a.m >> n.m
	if a.neg && a.m&(1<<n.m-1) != 0 {
		m++
	}
	return ExactMk(a.neg, m)
}

func ExactW(a ExactX) (uint64, uint64) {
	if !a.neg {
		return 0, a.m
	}
	return ^uint64(0), -a.m
}

func ExactFromW(hi uint64, lo uint64) ExactX {
	if hi == 0 {
		return ExactX{false, lo}
	}
	if hi == ^uint64(0) && lo>>63 == 1 {
		return ExactMk(true, -lo)
	}
	panic("numerus overflow")
}

func ExactAnd(a ExactX, b ExactX) ExactX {
	ah, al := ExactW(a)
	bh, bl := ExactW(b)
	return ExactFromW(ah&bh, al&bl)
}

func ExactOr(a ExactX, b ExactX) ExactX {
	ah, al := ExactW(a)
	bh, bl := ExactW(b)
	return ExactFromW(ah|bh, al|bl)
}

func ExactXor(a ExactX, b ExactX) ExactX {
	ah, al := ExactW(a)
	bh, bl := ExactW(b)
	return ExactFromW(ah^bh, al^bl)
}

func ExactNot(a ExactX) ExactX { return ExactSub(ExactNeg(a), ExactU(1)) }

func ExactAbs(a ExactX) ExactX { return ExactMk(false, a.m) }

func ExactPow(base ExactX, exponent ExactX) ExactX {
	if exponent.neg {
		panic("numerus potentia failed: negative exponent")
	}
	accumulator := ExactU(1)
	for !exponent.neg && exponent.m > 0 {
		if exponent.m%2 != 0 {
			accumulator = ExactMul(accumulator, base)
		}
		exponent = ExactU(exponent.m / 2)
		if exponent.m > 0 {
			base = ExactMul(base, base)
		}
	}
	return accumulator
}

func ExactCmp(a ExactX, b ExactX) int {
	if a.neg != b.neg {
		if a.neg {
			return -1
		}
		return 1
	}
	c := 0
	if a.m < b.m {
		c = -1
	} else if a.m > b.m {
		c = 1
	}
	if a.neg {
		return -c
	}
	return c
}

// ExactApprox is `≈` on integers: 10^9 * |a - b| <= max(|a|, |b|), exactly.
// |a - b| reaches 2^64 + 2^63, so the scaled distance is a 128-bit value.
func ExactApprox(a ExactX, b ExactX) bool {
	var hi, lo uint64
	if a.neg == b.neg {
		lo = a.m - b.m
		if a.m < b.m {
			lo = b.m - a.m
		}
	} else {
		lo, hi = bits.Add64(a.m, b.m, 0)
	}
	mulHi, mulLo := bits.Mul64(lo, 1000000000)
	if hi*1000000000+mulHi != 0 {
		return false
	}
	limit := a.m
	if b.m > limit {
		limit = b.m
	}
	return mulLo <= limit
}

func ExactCmpF(a ExactX, f float64) (int, bool) {
	if f != f {
		return 0, false
	}
	if f >= 18446744073709551616.0 {
		return -1, true
	}
	if f < -9223372036854775808.0 {
		return 1, true
	}
	w := math.Trunc(f)
	wx := ExactX{false, uint64(w)}
	if w < 0 {
		wx = ExactX{true, uint64(-w)}
	}
	if c := ExactCmp(a, wx); c != 0 {
		return c, true
	}
	fr := f - w
	if fr > 0 {
		return -1, true
	}
	if fr < 0 {
		return 1, true
	}
	return 0, true
}

func ExactLtF(a ExactX, f float64) bool { c, ok := ExactCmpF(a, f); return ok && c < 0 }

func ExactLeF(a ExactX, f float64) bool { c, ok := ExactCmpF(a, f); return ok && c <= 0 }

func ExactGtF(a ExactX, f float64) bool { c, ok := ExactCmpF(a, f); return ok && c > 0 }

func ExactGeF(a ExactX, f float64) bool { c, ok := ExactCmpF(a, f); return ok && c >= 0 }

func ExactEqF(a ExactX, f float64) bool { c, ok := ExactCmpF(a, f); return ok && c == 0 }

func ExactStr(a ExactX) string {
	s := strconv.FormatUint(a.m, 10)
	if a.neg {
		return "-" + s
	}
	return s
}

func ExactTrap(a ExactX, ty string, pos string, extra string, unsigned bool) {
	msg := ExactStr(a) + " does not fit in `" + ty + "` (" + pos + ")" + extra
	if unsigned && a.neg {
		msg += " (a negative value cannot be stored in an unsigned slot)"
	}
	panic(msg)
}

func ExactFloat(a ExactX) float64 {
	f := float64(a.m)
	if a.neg {
		return -f
	}
	return f
}

func ExactFitsI(a ExactX, lo int64, hi int64) bool {
	return (!a.neg && a.m <= uint64(hi)) || (a.neg && a.m <= -uint64(lo))
}

func ExactFitsU(a ExactX, hi uint64) bool { return !a.neg && a.m <= hi }

// ExactInt is the set of Go sized integer types a store lands in. The set holds
// the exact types only (no `~`): the instantiation names the Faber width, and
// Go's own `int` is never one of them.
type ExactInt interface {
	int8 | int16 | int32 | int64 | uint8 | uint16 | uint32 | uint64
}

// exactRange is the width of a Go sized integer type: its inclusive bounds and
// the Faber width text a trap names (`int64` is `i64`, never Go `int`). The
// bounds of a signed type are `[lo, hi]`, of an unsigned type `[0, hi]`.
func exactRange[T ExactInt]() (lo int64, hi uint64, ty string) {
	var zero T
	switch any(zero).(type) {
	case int8:
		return math.MinInt8, math.MaxInt8, "i8"
	case int16:
		return math.MinInt16, math.MaxInt16, "i16"
	case int32:
		return math.MinInt32, math.MaxInt32, "i32"
	case int64:
		return math.MinInt64, math.MaxInt64, "i64"
	case uint8:
		return 0, math.MaxUint8, "u8"
	case uint16:
		return 0, math.MaxUint16, "u16"
	case uint32:
		return 0, math.MaxUint32, "u32"
	default:
		return 0, math.MaxUint64, "u64"
	}
}

// ExactStore leaves the carrier into a slot of Go type T under the trapping
// policy: the value must fit T's width, otherwise it panics naming the value,
// the Faber width text of T, the position and an optional inferred-slot note.
func ExactStore[T ExactInt](a ExactX, pos string, extra ...string) T {
	lo, hi, ty := exactRange[T]()
	note := ""
	if len(extra) > 0 {
		note = extra[0]
	}
	if lo < 0 {
		if !a.neg && a.m <= hi {
			return T(int64(a.m))
		}
		if a.neg && a.m <= -uint64(lo) {
			return T(-int64(a.m))
		}
		ExactTrap(a, ty, pos, note, false)
		return 0
	}
	if !a.neg && a.m <= hi {
		return T(a.m)
	}
	ExactTrap(a, ty, pos, note, true)
	return 0
}

func ExactWrap(a ExactX) uint64 {
	if a.neg {
		return -a.m
	}
	return a.m
}

func ExactClampI(a ExactX, lo int64, hi int64) int64 {
	if a.neg {
		if a.m > -uint64(lo) {
			return lo
		}
		return -int64(a.m)
	}
	if a.m > uint64(hi) {
		return hi
	}
	return int64(a.m)
}

func ExactClampU(a ExactX, hi uint64) uint64 {
	if a.neg {
		return 0
	}
	if a.m > hi {
		return hi
	}
	return a.m
}
