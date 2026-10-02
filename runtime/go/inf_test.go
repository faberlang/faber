package rt

import (
	"math"
	"math/big"
	"testing"
)

// big10 reads a literal decimal carrier; the expected values below are
// literals, never computed through the helpers under test.
func big10(t *testing.T, text string) *big.Int {
	t.Helper()
	v, ok := new(big.Int).SetString(text, 10)
	if !ok {
		t.Fatalf("bad literal %q", text)
	}
	return v
}

func wantInf(t *testing.T, name string, got *big.Int, want string) {
	t.Helper()
	if got == nil || got.String() != want {
		t.Fatalf("%s = %v, want %s", name, got, want)
	}
}

// TestInfLiftsAndNilIsZero pins the lifts and the nil-reads-as-zero rule.
func TestInfLiftsAndNilIsZero(t *testing.T) {
	wantInf(t, "I(-5)", InfI(-5), "-5")
	wantInf(t, "U(max)", InfU(math.MaxUint64), "18446744073709551615")
	wantInf(t, "S", InfS("123456789012345678901234567890"), "123456789012345678901234567890")
	wantInf(t, "Z(nil)", InfZ(nil), "0")
	wantInf(t, "Add(nil, 1)", InfAdd(nil, InfI(1)), "1")
	if InfSign(nil) != 0 || InfSign(InfI(-3)) != -1 || InfSign(InfI(9)) != 1 {
		t.Fatalf("Sign wrong")
	}
	wantPanic(t, "malformed inf literal", func() { InfS("12x") })
}

// TestInfArithmetic pins the exact arithmetic well past 64 bits.
func TestInfArithmetic(t *testing.T) {
	two64 := big10(t, "18446744073709551616")
	wantInf(t, "Add", InfAdd(InfU(math.MaxUint64), InfI(1)), "18446744073709551616")
	wantInf(t, "Sub", InfSub(InfI(0), two64), "-18446744073709551616")
	wantInf(t, "Mul", InfMul(two64, two64), "340282366920938463463374607431768211456")
	wantInf(t, "Neg", InfNeg(two64), "-18446744073709551616")
	wantInf(t, "Abs", InfAbs(InfI(-7)), "7")
	wantInf(t, "Not", InfNot(InfI(5)), "-6")
	wantInf(t, "And", InfAnd(InfI(-1), InfI(12)), "12")
	wantInf(t, "Or", InfOr(InfI(8), InfI(1)), "9")
	wantInf(t, "Xor", InfXor(InfI(6), InfI(3)), "5")
	wantInf(t, "Pow", InfPow(InfI(2), InfI(100)), "1267650600228229401496703205376")
	wantInf(t, "Pow zero", InfPow(InfI(0), InfI(0)), "1")
	wantInf(t, "Sum", InfSum([]*big.Int{InfI(1), nil, two64}), "18446744073709551617")
	wantInf(t, "Signum", InfSignum(InfI(-9)), "-1")
	wantInf(t, "Max", InfMax(InfI(3), InfI(-4)), "3")
	wantInf(t, "Min", InfMin(InfI(3), InfI(-4)), "-4")
	wantPanic(t, "numerus potentia failed: negative exponent", func() { InfPow(InfI(2), InfI(-1)) })
}

// TestInfDivAndModFloor pins the floored division and the sign-of-divisor mod.
func TestInfDivAndModFloor(t *testing.T) {
	wantInf(t, "7/2", InfDiv(InfI(7), InfI(2)), "3")
	wantInf(t, "-7/2", InfDiv(InfI(-7), InfI(2)), "-4")
	wantInf(t, "7/-2", InfDiv(InfI(7), InfI(-2)), "-4")
	wantInf(t, "-7/-2", InfDiv(InfI(-7), InfI(-2)), "3")
	wantInf(t, "7%2", InfMod(InfI(7), InfI(2)), "1")
	wantInf(t, "-7%2", InfMod(InfI(-7), InfI(2)), "1")
	wantInf(t, "7%-2", InfMod(InfI(7), InfI(-2)), "-1")
	wantPanic(t, "numerus division failed", func() { InfDiv(InfI(1), InfI(0)) })
	wantPanic(t, "numerus division failed", func() { InfMod(InfI(1), nil) })
}

// TestInfShifts pins the infinite two's-complement shifts and their traps.
func TestInfShifts(t *testing.T) {
	wantInf(t, "1<<100", InfShl(InfI(1), InfI(100)), "1267650600228229401496703205376")
	wantInf(t, "0<<huge", InfShl(InfI(0), big10(t, "99999999999999999999")), "0")
	wantInf(t, "-8>>1", InfShr(InfI(-8), InfI(1)), "-4")
	wantInf(t, "-1>>huge", InfShr(InfI(-1), big10(t, "99999999999999999999")), "-1")
	wantInf(t, "5>>huge", InfShr(InfI(5), big10(t, "99999999999999999999")), "0")
	wantPanic(t, "negative shift count", func() { InfShl(InfI(1), InfI(-1)) })
	wantPanic(t, "negative shift count", func() { InfShr(InfI(1), InfI(-1)) })
}

// TestInfCeilings pins the two implementation ceilings and their trap texts.
func TestInfCeilings(t *testing.T) {
	exhausted := "inf size exhausted in shift: the value would exceed the ceiling of 2147483648 bits"
	wantPanic(t, exhausted, func() { InfShl(InfI(1), InfI(1<<31)) })
	wantPanic(t, exhausted, func() { InfShl(InfI(1), big10(t, "99999999999999999999")) })
	// A multiplication whose schoolbook limb products pass 2^27 traps before it
	// allocates: 2^14 + 1 limbs by 2^14 + 1 limbs is past the ceiling.
	wide := InfShl(InfI(1), InfI(32*(1<<14)))
	wantPanic(t, "inf size exhausted in multiplication: the work exceeds the ceiling of 134217728 limb products", func() {
		InfMul(wide, wide)
	})
	wantInf(t, "Mul under the work ceiling", new(big.Int).SetInt64(int64(InfMul(InfShl(InfI(1), InfI(1000)), InfShl(InfI(1), InfI(1000))).BitLen())), "2001")
	// The power loop ends at the same multiplication ceiling.
	wantPanic(t, "inf size exhausted in multiplication: the work exceeds the ceiling of 134217728 limb products", func() {
		InfPow(wide, InfI(2))
	})
}

// TestInfCompare pins ordering, list equality and the float comparisons.
func TestInfCompare(t *testing.T) {
	two64 := big10(t, "18446744073709551616")
	if InfCmp(two64, InfI(5)) != 1 || InfCmp(InfI(5), two64) != -1 || InfCmp(nil, InfI(0)) != 0 {
		t.Fatalf("Cmp wrong")
	}
	if !InfListEq([]*big.Int{InfI(1), nil}, []*big.Int{InfI(1), InfI(0)}) {
		t.Fatalf("ListEq must compare by value")
	}
	if InfListEq([]*big.Int{InfI(1)}, []*big.Int{InfI(1), InfI(1)}) || InfListEq([]*big.Int{InfI(1)}, []*big.Int{InfI(2)}) {
		t.Fatalf("ListEq false positive")
	}
	nan, pinf, ninf := math.NaN(), math.Inf(1), math.Inf(-1)
	for name, got := range map[string]bool{
		"Lt(1, 1.5)":     InfLtF(InfI(1), 1.5),
		"Le(2, 2)":       InfLeF(InfI(2), 2),
		"Gt(2, 1.5)":     InfGtF(InfI(2), 1.5),
		"Ge(2, 2)":       InfGeF(InfI(2), 2),
		"Eq(2, 2)":       InfEqF(InfI(2), 2),
		"Lt(huge, +inf)": InfLtF(two64, pinf),
		"Gt(huge, -inf)": InfGtF(two64, ninf),
	} {
		if !got {
			t.Fatalf("%s must be true", name)
		}
	}
	for name, got := range map[string]bool{
		"Lt(nan)":        InfLtF(InfI(1), nan),
		"Le(nan)":        InfLeF(InfI(1), nan),
		"Gt(nan)":        InfGtF(InfI(1), nan),
		"Ge(nan)":        InfGeF(InfI(1), nan),
		"Eq(nan)":        InfEqF(InfI(1), nan),
		"Eq(1, 1.5)":     InfEqF(InfI(1), 1.5),
		"Gt(huge, +inf)": InfGtF(two64, pinf),
	} {
		if got {
			t.Fatalf("%s must be false", name)
		}
	}
}

// TestInfApprox pins the exact relative 1e-9 tolerance.
func TestInfApprox(t *testing.T) {
	if !InfApprox(InfI(1000000000), InfI(1000000001)) {
		t.Fatalf("1e9 vs 1e9+1 is within the tolerance")
	}
	if InfApprox(InfI(1000000000), InfI(1000000002)) {
		t.Fatalf("1e9 vs 1e9+2 is outside the tolerance")
	}
	if !InfApprox(InfI(0), InfI(0)) || InfApprox(InfI(0), InfI(1)) {
		t.Fatalf("zero tolerance wrong")
	}
}

// TestInfFloatAndTextConversions pins the float, text and valor conversions.
func TestInfFloatAndTextConversions(t *testing.T) {
	if InfF64(InfU(1<<63)) != 9223372036854775808.0 || InfF32(InfI(16777216)) != float32(16777216) {
		t.Fatalf("float conversion wrong")
	}
	if v, ok := InfFromF(-2.9); !ok || v.String() != "-2" {
		t.Fatalf("FromF(-2.9) = %v %v", v, ok)
	}
	if _, ok := InfFromF(math.NaN()); ok {
		t.Fatalf("FromF(NaN) must fail")
	}
	if _, ok := InfFromF(math.Inf(1)); ok {
		t.Fatalf("FromF(+inf) must fail")
	}
	if v, ok := InfParse("ff", 16); !ok || v.String() != "255" {
		t.Fatalf("Parse(ff, 16) = %v %v", v, ok)
	}
	if _, ok := InfParse("zz", 10); ok {
		t.Fatalf("Parse(zz) must fail")
	}
	if InfStr(nil) != "0" || InfStr(InfI(-12)) != "-12" {
		t.Fatalf("Str wrong")
	}
	for _, in := range []any{int(-3), int8(-3), int16(-3), int32(-3), int64(-3), InfI(-3)} {
		if v, ok := InfFromValor(in); !ok || v.String() != "-3" {
			t.Fatalf("FromValor(%T) = %v %v", in, v, ok)
		}
	}
	for _, in := range []any{uint(3), uint8(3), uint16(3), uint32(3), uint64(3)} {
		if v, ok := InfFromValor(in); !ok || v.String() != "3" {
			t.Fatalf("FromValor(%T) = %v %v", in, v, ok)
		}
	}
	if _, ok := InfFromValor("3"); ok {
		t.Fatalf("FromValor(string) must fail")
	}
}

// TestInfBytesAreMinimalTwosComplement pins the byte conversions.
func TestInfBytesAreMinimalTwosComplement(t *testing.T) {
	cases := []struct {
		value  string
		be, le []byte
	}{
		{"0", []byte{0x00}, []byte{0x00}},
		{"127", []byte{0x7f}, []byte{0x7f}},
		{"128", []byte{0x00, 0x80}, []byte{0x80, 0x00}},
		{"-1", []byte{0xff}, []byte{0xff}},
		{"-128", []byte{0x80}, []byte{0x80}},
		{"-129", []byte{0xff, 0x7f}, []byte{0x7f, 0xff}},
		{"256", []byte{0x01, 0x00}, []byte{0x00, 0x01}},
	}
	for _, c := range cases {
		v := big10(t, c.value)
		if got := InfToBytes(v, true); string(got) != string(c.be) {
			t.Fatalf("ToBytes(%s, be) = %x, want %x", c.value, got, c.be)
		}
		if got := InfToBytes(v, false); string(got) != string(c.le) {
			t.Fatalf("ToBytes(%s, le) = %x, want %x", c.value, got, c.le)
		}
		wantInf(t, "FromBytes be "+c.value, InfFromBytes(c.be, true), c.value)
		wantInf(t, "FromBytes le "+c.value, InfFromBytes(c.le, false), c.value)
	}
	wantInf(t, "FromBytes empty", InfFromBytes(nil, true), "0")
}

// TestInfPackPadsOrFails pins the radix text pack.
func TestInfPackPadsOrFails(t *testing.T) {
	if got, ok := InfPack(InfI(255), 16, 4); !ok || got != "00ff" {
		t.Fatalf("Pack(255, 16, 4) = %q %v", got, ok)
	}
	if _, ok := InfPack(InfI(-1), 16, 4); ok {
		t.Fatalf("Pack(negative) must fail")
	}
	if _, ok := InfPack(InfI(65536), 16, 4); ok {
		t.Fatalf("Pack(too wide) must fail")
	}
}

// TestInfStoreChecksAreTheOnlyWidthLimit pins the store family and its trap text.
func TestInfStoreChecksAreTheOnlyWidthLimit(t *testing.T) {
	if !InfFitsI(InfI(127), -128, 127) || InfFitsI(InfI(128), -128, 127) || !InfFitsU(InfI(255), 255) || InfFitsU(InfI(-1), 255) {
		t.Fatalf("Fits wrong")
	}
	if InfStoreI(InfI(-128), -128, 127, "i8", "declaration", "") != -128 {
		t.Fatalf("StoreI in range")
	}
	if InfStoreU(InfI(255), 255, "u8", "declaration", "") != 255 {
		t.Fatalf("StoreU in range")
	}
	wantPanic(t, "128 does not fit in `i8` (assignment)", func() { InfStoreI(InfI(128), -128, 127, "i8", "assignment", "") })
	wantPanic(t, "300 does not fit in `u8` (return) hint", func() { InfStoreU(InfI(300), 255, "u8", "return", " hint") })
	wantPanic(t,
		"-1 does not fit in `u8` (argument) (a negative value cannot be stored in an unsigned slot)",
		func() { InfStoreU(InfI(-1), 255, "u8", "argument", "") })
	wantPanic(t,
		"18446744073709551616 does not fit in `i64` (declaration)",
		func() {
			InfStoreI(big10(t, "18446744073709551616"), math.MinInt64, math.MaxInt64, "i64", "declaration", "")
		})
}

// TestInfWrapAndClamp pins the wrapping and saturating policies.
func TestInfWrapAndClamp(t *testing.T) {
	if InfWrap(InfI(-1)) != math.MaxUint64 || InfWrap(big10(t, "18446744073709551617")) != 1 || InfWrap(nil) != 0 {
		t.Fatalf("Wrap wrong")
	}
	if InfClampI(InfI(500), -128, 127) != 127 || InfClampI(InfI(-500), -128, 127) != -128 || InfClampI(InfI(7), -128, 127) != 7 {
		t.Fatalf("ClampI wrong")
	}
	if InfClampU(InfI(500), 255) != 255 || InfClampU(InfI(-5), 255) != 0 || InfClampU(InfI(7), 255) != 7 {
		t.Fatalf("ClampU wrong")
	}
}

// TestInfToDecIsScaleEight pins the decimal conversion bound.
func TestInfToDecIsScaleEight(t *testing.T) {
	if v, ok := InfToDec(InfI(3)); !ok || v != 300000000 {
		t.Fatalf("ToDec(3) = %d %v", v, ok)
	}
	if _, ok := InfToDec(big10(t, "92233720368547758")); ok {
		t.Fatalf("ToDec past int64 at scale 8 must fail")
	}
	if v, ok := InfToDec(big10(t, "92233720368")); !ok || v != 9223372036800000000 {
		t.Fatalf("ToDec(92233720368) = %d %v", v, ok)
	}
}

// TestInfKeysRoundTripByValue pins the map-key wrapper.
func TestInfKeysRoundTripByValue(t *testing.T) {
	if InfKey(InfI(-42)) != "-42" || InfKey(nil) != "0" {
		t.Fatalf("Key wrong")
	}
	wantInf(t, "FromKey", InfFromKey("123456789012345678901234567890"), "123456789012345678901234567890")
	if InfPart(InfKey(InfI(1))) != InfPart("1") {
		t.Fatalf("InfPart carries the key string")
	}
}
