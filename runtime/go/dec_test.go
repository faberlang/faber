package rt

import (
	"math"
	"math/big"
	"testing"
)

// wantDec pins a carrier by its literal coefficient and scale.
func wantDec(t *testing.T, name string, got DecCarrier, c int64, s uint) {
	t.Helper()
	if got.c.Cmp(big.NewInt(c)) != 0 || got.s != s {
		t.Fatalf("%s = {%s %d}, want {%d %d}", name, got.c.String(), got.s, c, s)
	}
}

func decPow2(exp uint) *big.Int {
	return new(big.Int).Lsh(big.NewInt(1), exp)
}

// TestDecCarriersAndNormalization pins the lifts and the scale-8 floor of
// normalization (a result never keeps trailing zeros past scale 8).
func TestDecCarriersAndNormalization(t *testing.T) {
	wantDec(t, "Of(5)", DecOf(5), 5, 8)
	wantDec(t, "Int(7)", DecInt(big.NewInt(7)), 7, 0)
	wantDec(t, "Add", DecAdd(DecOf(150000000), DecOf(250000000)), 400000000, 8)
	wantDec(t, "Sub", DecSub(DecOf(150000000), DecOf(250000000)), -100000000, 8)
	wantDec(t, "Mul keeps scale 8 after normalization", DecMul(DecOf(100000000), DecOf(100000000)), 100000000, 8)
	wantDec(t, "Mul integers add scales", DecMul(DecInt(big.NewInt(3)), DecInt(big.NewInt(4))), 12, 0)
	wantDec(t, "Add aligns scales", DecAdd(DecInt(big.NewInt(1)), DecOf(50000000)), 150000000, 8)
	wantDec(t, "Neg", DecNeg(DecOf(150000000)), -150000000, 8)
}

// TestDecOverflowIsTheCarrierBound pins the 128-bit carrier bound and its text.
func TestDecOverflowIsTheCarrierBound(t *testing.T) {
	wantPanic(t, "numerus overflow", func() { DecAdd(DecInt(decPow2(126)), DecInt(decPow2(126))) })
	wantPanic(t, "numerus overflow", func() { DecMul(DecInt(decPow2(64)), DecInt(decPow2(64))) })
	wantPanic(t, "numerus overflow", func() { DecNeg(DecInt(new(big.Int).Neg(decPow2(127)))) })
	// Aligning to the larger scale is itself bounded.
	wantPanic(t, "numerus overflow", func() {
		DecAdd(DecInt(decPow2(120)), DecCarrier{big.NewInt(1), 20})
	})
}

// TestDecDivRoundsHalfEvenAtTheLargerScale pins the division rounding.
func TestDecDivRoundsHalfEvenAtTheLargerScale(t *testing.T) {
	one, two, three, five := DecInt(big.NewInt(1)), DecInt(big.NewInt(2)), DecInt(big.NewInt(3)), DecInt(big.NewInt(5))
	wantDec(t, "1/2 ties to even 0", DecDiv(one, two), 0, 0)
	wantDec(t, "3/2 ties to even 2", DecDiv(three, two), 2, 0)
	wantDec(t, "5/2 ties to even 2", DecDiv(five, two), 2, 0)
	wantDec(t, "-3/2 ties to even -2", DecDiv(DecInt(big.NewInt(-3)), two), -2, 0)
	wantDec(t, "1.0/4.0", DecDiv(DecOf(100000000), DecOf(400000000)), 25000000, 8)
	wantPanic(t, "numerus division failed", func() { DecDiv(one, DecInt(big.NewInt(0))) })
	// The scaled numerator is bounded; its overflow reports as a failed division.
	wantPanic(t, "numerus division failed", func() {
		DecDiv(DecInt(decPow2(100)), DecCarrier{big.NewInt(1), 30})
	})
}

// TestDecRemIsFloorRemainder pins the F9 ruling 29 remainder: the sign of the
// divisor.
func TestDecRemIsFloorRemainder(t *testing.T) {
	wantDec(t, "-7 rem 3", DecRem(DecInt(big.NewInt(-7)), DecInt(big.NewInt(3))), 2, 0)
	wantDec(t, "7 rem -3", DecRem(DecInt(big.NewInt(7)), DecInt(big.NewInt(-3))), -2, 0)
	wantDec(t, "7 rem 3", DecRem(DecInt(big.NewInt(7)), DecInt(big.NewInt(3))), 1, 0)
	wantDec(t, "7.5 rem 2", DecRem(DecOf(750000000), DecOf(200000000)), 150000000, 8)
	wantPanic(t, "numerus division failed", func() { DecRem(DecOf(1), DecInt(big.NewInt(0))) })
}

// TestDecCmpFmt pins the ordering and the shortest-text display.
func TestDecCmpFmt(t *testing.T) {
	if got := DecCmp(DecOf(100000000), DecInt(big.NewInt(1))); got != 0 {
		t.Fatalf("Cmp(1.0, 1) = %d, want 0", got)
	}
	if got := DecCmp(DecOf(1), DecOf(2)); got != -1 {
		t.Fatalf("Cmp(1, 2) = %d, want -1", got)
	}
	if got := DecCmp(DecOf(3), DecOf(2)); got != 1 {
		t.Fatalf("Cmp(3, 2) = %d, want 1", got)
	}
	for _, c := range []struct {
		a    DecCarrier
		want string
	}{
		{DecOf(150000000), "1.5"},
		{DecOf(-5000000), "-0.05"},
		{DecOf(0), "0"},
		{DecInt(big.NewInt(7)), "7"},
		{DecOf(100000000), "1"},
		{DecOf(1), "0.00000001"},
	} {
		if got := DecFmt(c.a); got != c.want {
			t.Fatalf("Fmt = %q, want %q", got, c.want)
		}
	}
}

// TestDecStoreRoundsHalfEvenAndTraps pins the store: half-even to scale 8,
// outside int64 the stored width trap text.
func TestDecStoreRoundsHalfEvenAndTraps(t *testing.T) {
	for _, c := range []struct {
		a    DecCarrier
		want int64
	}{
		{DecCarrier{big.NewInt(125), 10}, 1},
		{DecCarrier{big.NewInt(150), 10}, 2},
		{DecCarrier{big.NewInt(250), 10}, 2},
		{DecCarrier{big.NewInt(-150), 10}, -2},
		{DecInt(big.NewInt(3)), 300000000},
	} {
		got, ok := DecTryStore(c.a)
		if !ok || got != c.want {
			t.Fatalf("TryStore = (%d, %v), want (%d, true)", got, ok, c.want)
		}
	}
	if _, ok := DecTryStore(DecInt(big.NewInt(1000000000000))); ok {
		t.Fatalf("TryStore(1e12) fit; want out of range")
	}
	if got := DecStore(DecOf(150000000), "x", ""); got != 150000000 {
		t.Fatalf("Store = %d, want 150000000", got)
	}
	wantPanic(t, "1000000000000 does not fit in `d64` (x:1) (inferred)", func() {
		DecStore(DecInt(big.NewInt(1000000000000)), "x:1", " (inferred)")
	})
}

// TestDecToIntF64AndFloatCmp pins the conversions out of the carrier.
func TestDecToIntF64AndFloatCmp(t *testing.T) {
	if got := DecToInt(DecOf(250000000)); got != 2 {
		t.Fatalf("ToInt(2.5) = %d, want 2", got)
	}
	if got := DecToInt(DecOf(350000000)); got != 4 {
		t.Fatalf("ToInt(3.5) = %d, want 4", got)
	}
	if got := DecToInt(DecOf(-250000000)); got != -2 {
		t.Fatalf("ToInt(-2.5) = %d, want -2", got)
	}
	wantPanic(t, "decimal to numerus conversion out of range", func() { DecToInt(DecInt(decPow2(63))) })
	if got := DecF64(DecOf(150000000)); got != 1.5 {
		t.Fatalf("F64(1.5) = %v, want 1.5", got)
	}
	d := DecOf(150000000)
	for _, c := range []struct {
		f    float64
		want int
	}{
		{1.5, 0},
		{1.25, 1},
		{2.0, -1},
		{math.NaN(), 2},
		{math.Inf(1), -1},
		{math.Inf(-1), 1},
	} {
		if got := DecFloatCmp(d, c.f); got != c.want {
			t.Fatalf("FloatCmp(1.5, %v) = %d, want %d", c.f, got, c.want)
		}
	}
}

// TestDecParse pins the decimal text parse: the carrier result, the two
// failure texts and the half-even rounding of extra digits.
func TestDecParse(t *testing.T) {
	const malformed = "textus to numerus conversion failed"
	const outOfRange = "textus to numerus conversion out of range"
	for _, c := range []struct {
		text    string
		want    int64
		failure string
	}{
		{"1.5", 150000000, ""},
		{"-2.25e1", -2250000000, ""},
		{"+3", 300000000, ""},
		{"0.000000001", 0, ""},
		{"0.000000005", 0, ""},
		{"0.000000015", 2, ""},
		{"abc", 0, malformed},
		{"", 0, malformed},
		{"1e", 0, malformed},
		{"1.5x", 0, malformed},
		{"1111111111111111111111111111111111111111", 0, malformed},
		{"1e40", 0, outOfRange},
		{"92233720368.54775808", 0, outOfRange},
	} {
		got, failure := DecTryParse(c.text, 8)
		if got != c.want || failure != c.failure {
			t.Fatalf("TryParse(%q) = (%d, %q), want (%d, %q)", c.text, got, failure, c.want, c.failure)
		}
	}
	if got := DecParse("1.5", 8); got != 150000000 {
		t.Fatalf("Parse(1.5) = %d, want 150000000", got)
	}
	wantPanic(t, malformed, func() { DecParse("x", 8) })
	wantPanic(t, outOfRange, func() { DecParse("1e40", 8) })
}
