package rt

import (
	"math"
	"testing"
)

// panicText runs f and returns the text it panicked with ("" when it did not).
func panicText(f func()) (text string) {
	defer func() {
		if r := recover(); r != nil {
			if s, ok := r.(string); ok {
				text = s
			} else {
				text = "non-string panic"
			}
		}
	}()
	f()
	return ""
}

func wantPanic(t *testing.T, want string, f func()) {
	t.Helper()
	if got := panicText(f); got != want {
		t.Fatalf("panic = %q, want %q", got, want)
	}
}

func wantExact(t *testing.T, name string, got ExactX, wantNeg bool, wantM uint64) {
	t.Helper()
	if got.neg != wantNeg || got.m != wantM {
		t.Fatalf("%s = {%v %d}, want {%v %d}", name, got.neg, got.m, wantNeg, wantM)
	}
}

// TestExactLiftAndWindow pins the lifts and the [-2^63, 2^64-1] window.
func TestExactLiftAndWindow(t *testing.T) {
	wantExact(t, "I(-5)", ExactI(-5), true, 5)
	wantExact(t, "I(math.MinInt64)", ExactI(math.MinInt64), true, 1<<63)
	wantExact(t, "U(max)", ExactU(math.MaxUint64), false, math.MaxUint64)
	wantExact(t, "Mk(neg, 0)", ExactMk(true, 0), false, 0)
	wantExact(t, "Neg(5)", ExactNeg(ExactI(5)), true, 5)
	wantPanic(t, "numerus overflow", func() { ExactMk(true, 1<<63+1) })
	wantPanic(t, "numerus overflow", func() { ExactNeg(ExactU(math.MaxUint64)) })
}

// TestExactAddSubMulOverflow pins the two-limb overflow checks.
func TestExactAddSubMulOverflow(t *testing.T) {
	wantExact(t, "Add", ExactAdd(ExactI(-3), ExactI(5)), false, 2)
	wantExact(t, "Add cross sign", ExactAdd(ExactI(3), ExactI(-5)), true, 2)
	wantExact(t, "Sub", ExactSub(ExactI(3), ExactI(5)), true, 2)
	wantExact(t, "Mul", ExactMul(ExactI(-3), ExactI(5)), true, 15)
	wantPanic(t, "numerus overflow", func() { ExactAdd(ExactU(math.MaxUint64), ExactI(1)) })
	wantPanic(t, "numerus overflow", func() { ExactSub(ExactI(math.MinInt64), ExactI(1)) })
	wantPanic(t, "numerus overflow", func() { ExactMul(ExactU(1<<32), ExactU(1<<32)) })
	wantPanic(t, "numerus overflow", func() { ExactMul(ExactI(math.MinInt64), ExactI(2)) })
	wantExact(t, "Mul at the window edge", ExactMul(ExactU(1<<32), ExactU(1<<31)), false, 1<<63)
}

// TestExactDivModFloor pins floor division and the division-failed trap.
func TestExactDivModFloor(t *testing.T) {
	wantExact(t, "7/2", ExactDiv(ExactI(7), ExactI(2)), false, 3)
	wantExact(t, "-7/2", ExactDiv(ExactI(-7), ExactI(2)), true, 4)
	wantExact(t, "7/-2", ExactDiv(ExactI(7), ExactI(-2)), true, 4)
	wantExact(t, "-7/-2", ExactDiv(ExactI(-7), ExactI(-2)), false, 3)
	wantExact(t, "7%2", ExactMod(ExactI(7), ExactI(2)), false, 1)
	wantExact(t, "-7%2", ExactMod(ExactI(-7), ExactI(2)), false, 1)
	wantExact(t, "7%-2", ExactMod(ExactI(7), ExactI(-2)), true, 1)
	wantExact(t, "-7%-2", ExactMod(ExactI(-7), ExactI(-2)), true, 1)
	wantPanic(t, "numerus division failed", func() { ExactDiv(ExactI(1), ExactI(0)) })
	wantPanic(t, "numerus division failed", func() { ExactMod(ExactI(1), ExactI(0)) })
	// i64::MIN / -1 leaves the i64 range but stays inside the carrier window.
	wantExact(t, "MIN/-1", ExactDiv(ExactI(math.MinInt64), ExactI(-1)), false, 1<<63)
}

// TestExactShifts pins shifts, the overflow check and the negative-count trap.
func TestExactShifts(t *testing.T) {
	wantExact(t, "1<<10", ExactShl(ExactI(1), ExactI(10)), false, 1024)
	wantExact(t, "0<<100", ExactShl(ExactI(0), ExactI(100)), false, 0)
	wantExact(t, "-1<<63", ExactShl(ExactI(-1), ExactI(63)), true, 1<<63)
	wantPanic(t, "numerus overflow", func() { ExactShl(ExactI(1), ExactI(64)) })
	wantPanic(t, "numerus overflow", func() { ExactShl(ExactI(1), ExactI(65)) })
	wantPanic(t, "negative shift count", func() { ExactShl(ExactI(1), ExactI(-1)) })
	wantPanic(t, "negative shift count", func() { ExactShr(ExactI(1), ExactI(-1)) })
	wantExact(t, "5>>1", ExactShr(ExactI(5), ExactI(1)), false, 2)
	wantExact(t, "-5>>1", ExactShr(ExactI(-5), ExactI(1)), true, 3)
	wantExact(t, "-5>>64", ExactShr(ExactI(-5), ExactI(64)), true, 1)
	wantExact(t, "5>>64", ExactShr(ExactI(5), ExactI(64)), false, 0)
}

// TestExactBitwise pins the two's-complement bit operations over the window.
func TestExactBitwise(t *testing.T) {
	wantExact(t, "-1&255", ExactAnd(ExactI(-1), ExactI(255)), false, 255)
	wantExact(t, "-8|3", ExactOr(ExactI(-8), ExactI(3)), true, 5)
	wantExact(t, "5^-1", ExactXor(ExactI(5), ExactI(-1)), true, 6)
	wantExact(t, "!0", ExactNot(ExactI(0)), true, 1)
	wantExact(t, "!-1", ExactNot(ExactI(-1)), false, 0)
	wantExact(t, "Abs(-9)", ExactAbs(ExactI(-9)), false, 9)
	hi, lo := ExactW(ExactI(-1))
	if hi != math.MaxUint64 || lo != math.MaxUint64 {
		t.Fatalf("W(-1) = %d %d", hi, lo)
	}
	wantPanic(t, "numerus overflow", func() { ExactFromW(1, 0) })
	wantPanic(t, "numerus overflow", func() { ExactFromW(math.MaxUint64, 1) })
}

// TestExactPow pins exponentiation and its trap texts.
func TestExactPow(t *testing.T) {
	wantExact(t, "3^4", ExactPow(ExactI(3), ExactI(4)), false, 81)
	wantExact(t, "(-2)^3", ExactPow(ExactI(-2), ExactI(3)), true, 8)
	wantExact(t, "x^0", ExactPow(ExactI(7), ExactI(0)), false, 1)
	wantPanic(t, "numerus potentia failed: negative exponent", func() { ExactPow(ExactI(2), ExactI(-1)) })
	wantPanic(t, "numerus overflow", func() { ExactPow(ExactI(2), ExactI(64)) })
	wantExact(t, "2^63", ExactPow(ExactI(2), ExactI(63)), false, 1<<63)
}

// TestExactCompare pins the carrier comparison, the exact tolerance and the
// float comparisons (NaN is false under every ordering).
func TestExactCompare(t *testing.T) {
	if ExactCmp(ExactI(-1), ExactI(1)) != -1 || ExactCmp(ExactI(2), ExactI(2)) != 0 || ExactCmp(ExactI(3), ExactI(-3)) != 1 {
		t.Fatal("ExactCmp ordering")
	}
	if ExactCmp(ExactI(-3), ExactI(-2)) != -1 {
		t.Fatal("ExactCmp negative magnitudes order inversely")
	}
	if !ExactApprox(ExactI(1_000_000_000), ExactI(1_000_000_001)) {
		t.Fatal("ExactApprox within tolerance")
	}
	if ExactApprox(ExactI(1), ExactI(2)) {
		t.Fatal("ExactApprox outside tolerance")
	}
	if ExactApprox(ExactI(math.MinInt64), ExactU(math.MaxUint64)) {
		t.Fatal("ExactApprox across the whole window is not approximately equal")
	}
	nan := math.NaN()
	if ExactLtF(ExactI(1), nan) || ExactLeF(ExactI(1), nan) || ExactGtF(ExactI(1), nan) || ExactGeF(ExactI(1), nan) || ExactEqF(ExactI(1), nan) {
		t.Fatal("NaN must be false under every comparison")
	}
	if !ExactLtF(ExactI(1), 1.5) || !ExactGtF(ExactI(2), 1.5) || !ExactEqF(ExactI(2), 2.0) || !ExactLeF(ExactI(2), 2.0) || !ExactGeF(ExactI(2), 2.0) {
		t.Fatal("float comparison")
	}
	if !ExactLtF(ExactU(math.MaxUint64), 18446744073709551616.0) {
		t.Fatal("u64::MAX is below 2^64")
	}
	if !ExactGtF(ExactI(math.MinInt64), -9223372036854777856.0) {
		t.Fatal("i64::MIN is above a float below the window")
	}
	if c, ok := ExactCmpF(ExactI(1), math.NaN()); ok || c != 0 {
		t.Fatal("ExactCmpF NaN")
	}
}

// TestExactTextAndFloat pins the text and float views of the carrier.
func TestExactTextAndFloat(t *testing.T) {
	if got := ExactStr(ExactI(-42)); got != "-42" {
		t.Fatalf("ExactStr = %q", got)
	}
	if got := ExactStr(ExactU(math.MaxUint64)); got != "18446744073709551615" {
		t.Fatalf("ExactStr = %q", got)
	}
	if got := ExactFloat(ExactI(-3)); got != -3.0 {
		t.Fatalf("ExactFloat = %v", got)
	}
}

// TestExactStoreWidthText pins the mapping from a Go sized type to the Faber
// width text a store trap names and to its bounds: `int64` reads `i64`, never
// Go `int`, and every width traps one past its bound.
func TestExactStoreWidthText(t *testing.T) {
	check := func(ty string, lo int64, hi uint64, gotLo int64, gotHi uint64, gotTy string) {
		t.Helper()
		if gotTy != ty || gotLo != lo || gotHi != hi {
			t.Fatalf("%s: got (%d, %d, %q), want (%d, %d, %q)", ty, gotLo, gotHi, gotTy, lo, hi, ty)
		}
	}
	lo, hi, ty := exactRange[int8]()
	check("i8", math.MinInt8, math.MaxInt8, lo, hi, ty)
	lo, hi, ty = exactRange[int16]()
	check("i16", math.MinInt16, math.MaxInt16, lo, hi, ty)
	lo, hi, ty = exactRange[int32]()
	check("i32", math.MinInt32, math.MaxInt32, lo, hi, ty)
	lo, hi, ty = exactRange[int64]()
	check("i64", math.MinInt64, math.MaxInt64, lo, hi, ty)
	lo, hi, ty = exactRange[uint8]()
	check("u8", 0, math.MaxUint8, lo, hi, ty)
	lo, hi, ty = exactRange[uint16]()
	check("u16", 0, math.MaxUint16, lo, hi, ty)
	lo, hi, ty = exactRange[uint32]()
	check("u32", 0, math.MaxUint32, lo, hi, ty)
	lo, hi, ty = exactRange[uint64]()
	check("u64", 0, math.MaxUint64, lo, hi, ty)

	if got := ExactStore[int64](ExactI(math.MinInt64), "declaration"); got != math.MinInt64 {
		t.Fatalf("Store[int64](min) = %d", got)
	}
	if got := ExactStore[uint64](ExactU(math.MaxUint64), "declaration"); got != math.MaxUint64 {
		t.Fatalf("Store[uint64](max) = %d", got)
	}
	if got := ExactStore[int32](ExactI(math.MaxInt32), "declaration"); got != math.MaxInt32 {
		t.Fatalf("Store[int32](max) = %d", got)
	}
	wantPanic(t, "9223372036854775808 does not fit in `i64` (return)", func() {
		ExactStore[int64](ExactU(1<<63), "return")
	})
	wantPanic(t, "-1 does not fit in `u64` (return) (a negative value cannot be stored in an unsigned slot)", func() {
		ExactStore[uint64](ExactI(-1), "return")
	})
	wantPanic(t, "65536 does not fit in `u16` (field `f`)", func() {
		ExactStore[uint16](ExactI(65536), "field `f`")
	})
	wantPanic(t, "2147483648 does not fit in `i32` (assignment)", func() {
		ExactStore[int32](ExactI(1<<31), "assignment")
	})
}

// TestExactStoreAndClamp pins the store family: the width limit applies only at
// the store, with the trap texts of the runner.
func TestExactStoreAndClamp(t *testing.T) {
	if !ExactFitsI(ExactI(127), -128, 127) || ExactFitsI(ExactI(128), -128, 127) || !ExactFitsI(ExactI(-128), -128, 127) || ExactFitsI(ExactI(-129), -128, 127) {
		t.Fatal("ExactFitsI")
	}
	if !ExactFitsU(ExactI(255), 255) || ExactFitsU(ExactI(256), 255) || ExactFitsU(ExactI(-1), 255) {
		t.Fatal("ExactFitsU")
	}
	if got := ExactStore[int8](ExactI(-128), "declaration of `x`"); got != -128 {
		t.Fatalf("Store[int8] = %d", got)
	}
	if got := ExactStore[uint8](ExactU(255), "declaration of `x`"); got != 255 {
		t.Fatalf("Store[uint8] = %d", got)
	}
	wantPanic(t, "128 does not fit in `i8` (assignment to `x`)", func() {
		ExactStore[int8](ExactI(128), "assignment to `x`")
	})
	wantPanic(t, "-129 does not fit in `i8` (return); `x` was inferred as `i8` from `f()`, declare its type", func() {
		ExactStore[int8](ExactI(-129), "return", "; `x` was inferred as `i8` from `f()`, declare its type")
	})
	wantPanic(t, "256 does not fit in `u8` (argument `a`)", func() {
		ExactStore[uint8](ExactI(256), "argument `a`")
	})
	wantPanic(t, "-1 does not fit in `u8` (field `f`) (a negative value cannot be stored in an unsigned slot)", func() {
		ExactStore[uint8](ExactI(-1), "field `f`")
	})
	if got := ExactWrap(ExactI(-1)); got != math.MaxUint64 {
		t.Fatalf("Wrap(-1) = %d", got)
	}
	if got := ExactWrap(ExactU(300)); got != 300 {
		t.Fatalf("Wrap(300) = %d", got)
	}
	if ExactClampI(ExactI(300), -128, 127) != 127 || ExactClampI(ExactI(-300), -128, 127) != -128 || ExactClampI(ExactI(-5), -128, 127) != -5 {
		t.Fatal("ExactClampI")
	}
	if ExactClampU(ExactI(-1), 255) != 0 || ExactClampU(ExactI(300), 255) != 255 || ExactClampU(ExactI(7), 255) != 7 {
		t.Fatal("ExactClampU")
	}
}
