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

// TestExactStoreAndClamp pins the store family: the width limit applies only at
// the store, with the trap texts of the runner.
func TestExactStoreAndClamp(t *testing.T) {
	if !ExactFitsI(ExactI(127), -128, 127) || ExactFitsI(ExactI(128), -128, 127) || !ExactFitsI(ExactI(-128), -128, 127) || ExactFitsI(ExactI(-129), -128, 127) {
		t.Fatal("ExactFitsI")
	}
	if !ExactFitsU(ExactI(255), 255) || ExactFitsU(ExactI(256), 255) || ExactFitsU(ExactI(-1), 255) {
		t.Fatal("ExactFitsU")
	}
	if got := ExactStoreI(ExactI(-128), -128, 127, "i8", "declaration of `x`", ""); got != -128 {
		t.Fatalf("StoreI = %d", got)
	}
	if got := ExactStoreU(ExactU(255), 255, "u8", "declaration of `x`", ""); got != 255 {
		t.Fatalf("StoreU = %d", got)
	}
	wantPanic(t, "128 does not fit in `i8` (assignment to `x`)", func() {
		ExactStoreI(ExactI(128), -128, 127, "i8", "assignment to `x`", "")
	})
	wantPanic(t, "-129 does not fit in `i8` (return); `x` was inferred as `i8` from `f()`, declare its type", func() {
		ExactStoreI(ExactI(-129), -128, 127, "i8", "return", "; `x` was inferred as `i8` from `f()`, declare its type")
	})
	wantPanic(t, "256 does not fit in `u8` (argument `a`)", func() {
		ExactStoreU(ExactI(256), 255, "u8", "argument `a`", "")
	})
	wantPanic(t, "-1 does not fit in `u8` (field `f`) (a negative value cannot be stored in an unsigned slot)", func() {
		ExactStoreU(ExactI(-1), 255, "u8", "field `f`", "")
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
