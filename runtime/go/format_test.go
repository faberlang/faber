package rt

import (
	"math"
	"math/big"
	"testing"
)

// spec builds a layout spec the way a generated call site does; the expected
// values below are literals, never computed through the helpers under test.
func spec(fill rune, align rune, plus bool, zero bool, width int, precision int, kind rune) FormatSpec {
	return FormatSpec{Fill: fill, Align: align, Plus: plus, Zero: zero, Width: width, Precision: precision, Kind: kind}
}

func wantText(t *testing.T, name string, got string, want string) {
	t.Helper()
	if got != want {
		t.Fatalf("%s = %q, want %q", name, got, want)
	}
}

// TestFormatPadAlignsByRuneCount pins fill, the three alignments, the zero pad
// and the rune (not byte) width of the sign and body.
func TestFormatPadAlignsByRuneCount(t *testing.T) {
	right := spec(' ', '>', false, false, 6, -1, ' ')
	wantText(t, "right", FormatPad("-", "12", right, false), "   -12")
	left := spec('*', '<', false, false, 6, -1, ' ')
	wantText(t, "left", FormatPad("", "ab", left, false), "ab****")
	centre := spec('*', '^', false, false, 7, -1, ' ')
	wantText(t, "centre odd", FormatPad("", "ab", centre, false), "**ab***")
	wantText(t, "zero", FormatPad("-", "12", right, true), "-00012")
	wantText(t, "wide enough", FormatPad("", "abcdefg", right, false), "abcdefg")
	wantText(t, "runes", FormatPad("", "∞", spec('.', '>', false, false, 3, -1, ' '), false), "..∞")
}

// TestFormatSigned pins the signed integer layouts: plus, radix kinds, zero
// pad, the precision pad and the most negative value.
func TestFormatSigned(t *testing.T) {
	wantText(t, "plain", FormatSigned(42, spec(' ', '>', false, false, 0, -1, ' ')), "42")
	wantText(t, "width", FormatSigned(42, spec(' ', '>', false, false, 5, -1, ' ')), "   42")
	wantText(t, "zero", FormatSigned(42, spec(' ', '>', false, true, 5, -1, ' ')), "00042")
	wantText(t, "left", FormatSigned(42, spec(' ', '<', false, false, 5, -1, ' ')), "42   ")
	wantText(t, "plus", FormatSigned(42, spec(' ', '>', true, false, 0, -1, ' ')), "+42")
	wantText(t, "negative", FormatSigned(-42, spec(' ', '>', true, false, 0, -1, ' ')), "-42")
	wantText(t, "hex", FormatSigned(255, spec(' ', '>', false, false, 0, -1, 'x')), "ff")
	wantText(t, "hex zero", FormatSigned(255, spec(' ', '>', false, true, 8, -1, 'x')), "000000ff")
	wantText(t, "negative hex", FormatSigned(-42, spec(' ', '>', false, false, 0, -1, 'x')), "-2a")
	wantText(t, "binary", FormatSigned(5, spec(' ', '>', false, false, 0, -1, 'b')), "101")
	wantText(t, "octal", FormatSigned(8, spec(' ', '>', false, false, 0, -1, 'o')), "10")
	wantText(t, "precision", FormatSigned(7, spec(' ', '>', false, false, 0, 2, ' ')), "7.00")
	wantText(t, "sci", FormatSigned(1500, spec(' ', '>', false, false, 0, 1, 'e')), "1.5e3")
	wantText(t, "min", FormatSigned(math.MinInt64, spec(' ', '>', false, false, 0, -1, ' ')), "-9223372036854775808")
}

// TestFormatIntUnsigned pins an unsigned magnitude past the signed range.
func TestFormatIntUnsigned(t *testing.T) {
	plain := spec(' ', '>', false, false, 0, -1, ' ')
	wantText(t, "max", FormatInt(false, math.MaxUint64, plain), "18446744073709551615")
	wantText(t, "zero binary", FormatInt(false, 5, spec(' ', '>', false, true, 8, -1, 'b')), "00000101")
}

// TestFormatFloat pins the float layouts: the default display body, NaN and
// the infinities, precision, scientific and the float32 shortest digits.
func TestFormatFloat(t *testing.T) {
	plain := spec(' ', '>', false, false, 0, -1, ' ')
	wantText(t, "integral f64", FormatFloat(3.0, 64, plain), "3.0")
	wantText(t, "fraction", FormatFloat(2.5, 64, plain), "2.5")
	wantText(t, "integral f32", FormatFloat(3.0, 32, plain), "3.0")
	wantText(t, "f32 shortest", FormatFloat(float64(float32(0.1)), 32, plain), "0.1")
	wantText(t, "precision", FormatFloat(3.14159, 64, spec(' ', '>', false, false, 0, 2, ' ')), "3.14")
	wantText(t, "zero pad", FormatFloat(-1.5, 64, spec(' ', '>', false, true, 8, 2, ' ')), "-0001.50")
	wantText(t, "width", FormatFloat(3.0, 64, spec(' ', '>', false, false, 6, -1, ' ')), "   3.0")
	wantText(t, "sci", FormatFloat(1500, 64, spec(' ', '>', false, false, 0, -1, 'e')), "1.5e3")
	wantText(t, "sci precision", FormatFloat(1500, 64, spec(' ', '>', false, false, 0, 2, 'e')), "1.50e3")
	wantText(t, "sci negative exponent", FormatFloat(0.00025, 64, spec(' ', '>', false, false, 0, 1, 'e')), "2.5e-4")
	wantText(t, "nan", FormatFloat(math.NaN(), 64, spec(' ', '>', false, false, 0, 2, ' ')), "NaN")
	wantText(t, "inf", FormatFloat(math.Inf(1), 64, spec(' ', '>', false, false, 0, 2, ' ')), "∞")
	wantText(t, "-inf", FormatFloat(math.Inf(-1), 64, plain), "-∞")
	wantText(t, "inf width", FormatFloat(math.Inf(1), 64, spec(' ', '>', false, false, 4, -1, ' ')), "   ∞")
	wantText(t, "plus", FormatFloat(2.5, 64, spec(' ', '>', true, false, 0, -1, ' ')), "+2.5")
}

// TestFormatTextTruncatesByRune pins the precision cut and the left default
// the call site passes for text.
func TestFormatTextTruncatesByRune(t *testing.T) {
	wantText(t, "cut", FormatText("abcdef", spec(' ', '<', false, false, 0, 3, ' ')), "abc")
	wantText(t, "pad", FormatText("ab", spec(' ', '<', false, false, 5, -1, ' ')), "ab   ")
	wantText(t, "right", FormatText("ab", spec('*', '>', false, false, 5, -1, ' ')), "***ab")
	wantText(t, "centre cut", FormatText("abcdef", spec(' ', '^', false, false, 5, 3, ' ')), " abc ")
	wantText(t, "runes", FormatText("héllo", spec(' ', '<', false, false, 0, 2, ' ')), "hé")
}

// TestFormatInstansPresets pins the iso, date and time presets.
func TestFormatInstansPresets(t *testing.T) {
	iso := "2026-10-02T11:26:17Z"
	wantText(t, "iso", FormatInstans(iso, 'i'), iso)
	wantText(t, "date", FormatInstans(iso, 'd'), "2026-10-02")
	wantText(t, "time", FormatInstans(iso, 't'), "11:26:17")
	wantText(t, "short clock", FormatInstans("2026-10-02T11:26", 't'), "11:26")
}

// TestFormatMagnus pins the `inf` renderings: a size past 64 bits, the radix
// kinds, precision, a nil pointer as zero and scientific past float64.
func TestFormatMagnus(t *testing.T) {
	huge, _ := new(big.Int).SetString("123456789012345678901234567890", 10)
	plain := spec(' ', '>', false, false, 0, -1, ' ')
	wantText(t, "huge", FormatMagnus(huge, plain), "123456789012345678901234567890")
	wantText(t, "negative", FormatMagnus(new(big.Int).Neg(huge), plain), "-123456789012345678901234567890")
	wantText(t, "nil", FormatMagnus(nil, plain), "0")
	wantText(t, "plus nil", FormatMagnus(nil, spec(' ', '>', true, false, 0, -1, ' ')), "+0")
	wantText(t, "hex", FormatMagnus(big.NewInt(255), spec(' ', '>', false, false, 0, -1, 'x')), "ff")
	wantText(t, "negative binary", FormatMagnus(big.NewInt(-5), spec(' ', '>', false, false, 0, -1, 'b')), "-101")
	wantText(t, "octal", FormatMagnus(big.NewInt(8), spec(' ', '>', false, false, 0, -1, 'o')), "10")
	wantText(t, "precision", FormatMagnus(big.NewInt(12), spec(' ', '>', false, false, 0, 2, ' ')), "12.00")
	wantText(t, "zero pad", FormatMagnus(big.NewInt(-7), spec(' ', '>', false, true, 5, -1, ' ')), "-0007")
	wantText(t, "sci", FormatMagnus(big.NewInt(1500), spec(' ', '>', false, false, 0, 1, 'e')), "1.5e3")
}

// TestFormatMagnusSciPastFloat64 pins the digit-string path used when the
// magnitude overflows float64 (more than 308 digits).
func TestFormatMagnusSciPastFloat64(t *testing.T) {
	huge := new(big.Int).Exp(big.NewInt(10), big.NewInt(400), nil)
	huge.Mul(huge, big.NewInt(25))
	wantText(t, "default", FormatMagnusSci(huge, -1), "2.5e401")
	wantText(t, "precision pads", FormatMagnusSci(huge, 3), "2.500e401")
	wantText(t, "precision cuts", FormatMagnusSci(huge, 0), "2e401")
	pow := new(big.Int).Exp(big.NewInt(10), big.NewInt(400), nil)
	wantText(t, "power of ten", FormatMagnusSci(pow, -1), "1e400")
}
