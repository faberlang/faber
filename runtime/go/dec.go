package rt

// The Faber `dec` carrier for generated Go programs (codegen-readability
// T1-G3): exact decimal helpers (D11.7, delivery rulings 19-20).
//
// An unstored decimal is a `math/big` carrier at its own scale
// (`DecCarrier{c, s}`); `+ -` align scales, `*` adds scales, `/` rounds
// half-even at the larger operand scale, every intermediate is bounded by the
// 128-bit carrier, and only the store (`DecStore`, `DecTryStore`) rounds
// half-even to scale 8 and traps outside `int64`. Mirrors `radix-mir-runner`'s
// `decimal` module, the golden authority. Every exported name is
// `Dec<Name>`, spelled by the compiler's canonical helper table; the carrier
// internals (`decBound`, `decAlign`, `decNorm`, `decRhe`, `decPow10`,
// `decRound`) are package-private.

import (
	"math"
	"math/big"
	"strings"
)

type DecCarrier struct {
	c *big.Int
	s uint
}

var decLimit = new(big.Int).Lsh(big.NewInt(1), 127)

func decBound(c *big.Int, message string) *big.Int {
	if c.Cmp(decLimit) >= 0 || c.Cmp(new(big.Int).Neg(decLimit)) < 0 {
		panic(message)
	}
	return c
}

func decPow10(digits uint) *big.Int {
	return new(big.Int).Exp(big.NewInt(10), big.NewInt(int64(digits)), nil)
}

func DecOf(value int64) DecCarrier {
	return DecCarrier{big.NewInt(value), 8}
}

func DecInt(value *big.Int) DecCarrier {
	return DecCarrier{value, 0}
}

func decNorm(c *big.Int, s uint) DecCarrier {
	ten := big.NewInt(10)
	for s > 8 {
		q, r := new(big.Int).QuoRem(c, ten, new(big.Int))
		if r.Sign() != 0 {
			break
		}
		c = q
		s--
	}
	return DecCarrier{c, s}
}

func decAlign(a DecCarrier, b DecCarrier) (*big.Int, *big.Int, uint) {
	s := a.s
	if b.s > s {
		s = b.s
	}
	x := decBound(new(big.Int).Mul(a.c, decPow10(s-a.s)), "numerus overflow")
	y := decBound(new(big.Int).Mul(b.c, decPow10(s-b.s)), "numerus overflow")
	return x, y, s
}

func DecAdd(a DecCarrier, b DecCarrier) DecCarrier {
	x, y, s := decAlign(a, b)
	return decNorm(decBound(new(big.Int).Add(x, y), "numerus overflow"), s)
}

func DecSub(a DecCarrier, b DecCarrier) DecCarrier {
	x, y, s := decAlign(a, b)
	return decNorm(decBound(new(big.Int).Sub(x, y), "numerus overflow"), s)
}

func DecMul(a DecCarrier, b DecCarrier) DecCarrier {
	c := decBound(new(big.Int).Mul(a.c, b.c), "numerus overflow")
	return decNorm(c, a.s+b.s)
}

func decRhe(n *big.Int, d *big.Int) *big.Int {
	q, r := new(big.Int).QuoRem(n, d, new(big.Int))
	if r.Sign() == 0 {
		return q
	}
	twice := new(big.Int).Lsh(new(big.Int).Abs(r), 1)
	cmp := twice.Cmp(new(big.Int).Abs(d))
	if cmp > 0 || (cmp == 0 && q.Bit(0) != 0) {
		if (n.Sign() < 0) == (d.Sign() < 0) {
			q.Add(q, big.NewInt(1))
		} else {
			q.Sub(q, big.NewInt(1))
		}
	}
	return q
}

func DecDiv(a DecCarrier, b DecCarrier) DecCarrier {
	if b.c.Sign() == 0 {
		panic("numerus division failed")
	}
	s := a.s
	if b.s > s {
		s = b.s
	}
	n := decBound(new(big.Int).Mul(a.c, decPow10(s+b.s-a.s)), "numerus division failed")
	return decNorm(decRhe(n, b.c), s)
}

func DecRem(a DecCarrier, b DecCarrier) DecCarrier {
	x, y, s := decAlign(a, b)
	if y.Sign() == 0 {
		panic("numerus division failed")
	}
	// Floor remainder (F9 ruling 29): the sign of the divisor, exact on the carriers.
	r := new(big.Int).Rem(x, y)
	if r.Sign() != 0 && (r.Sign() < 0) != (y.Sign() < 0) {
		r.Add(r, y)
	}
	return decNorm(r, s)
}

func DecNeg(a DecCarrier) DecCarrier {
	return decNorm(decBound(new(big.Int).Neg(a.c), "numerus overflow"), a.s)
}

func DecCmp(a DecCarrier, b DecCarrier) int {
	s := a.s
	if b.s > s {
		s = b.s
	}
	x := new(big.Int).Mul(a.c, decPow10(s-a.s))
	y := new(big.Int).Mul(b.c, decPow10(s-b.s))
	return x.Cmp(y)
}

func DecFmt(a DecCarrier) string {
	digits := int(a.s)
	magnitude := new(big.Int).Abs(a.c).String()
	if len(magnitude) < digits+1 {
		magnitude = strings.Repeat("0", digits+1-len(magnitude)) + magnitude
	}
	integer := magnitude[:len(magnitude)-digits]
	fraction := strings.TrimRight(magnitude[len(magnitude)-digits:], "0")
	sign := ""
	if a.c.Sign() < 0 {
		sign = "-"
	}
	if fraction == "" {
		return sign + integer
	}
	return sign + integer + "." + fraction
}

func DecTryStore(a DecCarrier) (int64, bool) {
	var rounded *big.Int
	if a.s > 8 {
		rounded = decRhe(a.c, decPow10(a.s-8))
	} else {
		rounded = new(big.Int).Mul(a.c, decPow10(8-a.s))
	}
	if !rounded.IsInt64() {
		return 0, false
	}
	return rounded.Int64(), true
}

func DecStore(a DecCarrier, at string, inferred string) int64 {
	value, ok := DecTryStore(a)
	if !ok {
		panic(DecFmt(a) + " does not fit in `d64` (" + at + ")" + inferred)
	}
	return value
}

func decRound(a DecCarrier) *big.Int {
	return decRhe(a.c, decPow10(a.s))
}

func DecToInt(a DecCarrier) int64 {
	rounded := decRound(a)
	if !rounded.IsInt64() {
		panic("decimal to numerus conversion out of range")
	}
	return rounded.Int64()
}

func DecF64(a DecCarrier) float64 {
	c, _ := new(big.Float).SetInt(a.c).Float64()
	p, _ := new(big.Float).SetInt(decPow10(a.s)).Float64()
	return c / p
}

func DecFloatCmp(a DecCarrier, f float64) int {
	if math.IsNaN(f) {
		return 2
	}
	if math.IsInf(f, 1) {
		return -1
	}
	if math.IsInf(f, -1) {
		return 1
	}
	return new(big.Rat).SetFrac(a.c, decPow10(a.s)).Cmp(new(big.Rat).SetFloat64(f))
}

// Decimal text parse (decimal-widths D5, B7+B8). Mirrors the MIR runner's
// `decimal_text_to_carrier`: the mantissa is bounded by the 128-bit carrier
// (beyond it the text is malformed), the shifted result must fit the `int64`
// carrier (else out of range), and arithmetic is never bounded per operation:
// only the conversion's result is the store. `DecTryParse` returns the
// runner's failure message (empty on success) so a `⊥` arm can recover from
// either failure; `DecParse` is the arm without `⊥` and panics with that
// message.
func DecTryParse(text string, scaleDigits int) (int64, string) {
	const malformed = "textus to numerus conversion failed"
	const outOfRange = "textus to numerus conversion out of range"
	negative := false
	digits := text
	if len(digits) > 0 && digits[0] == '-' {
		negative = true
		digits = digits[1:]
	} else if len(digits) > 0 && digits[0] == '+' {
		digits = digits[1:]
	}
	mantissa := new(big.Int)
	ten := big.NewInt(10)
	exp10 := int64(0)
	seenDigit := false
	rest := digits
	for len(rest) > 0 && rest[0] >= '0' && rest[0] <= '9' {
		mantissa.Add(mantissa.Mul(mantissa, ten), big.NewInt(int64(rest[0]-'0')))
		if mantissa.BitLen() > 127 {
			return 0, malformed
		}
		seenDigit = true
		rest = rest[1:]
	}
	if len(rest) > 0 && rest[0] == '.' {
		rest = rest[1:]
		for len(rest) > 0 && rest[0] >= '0' && rest[0] <= '9' {
			mantissa.Add(mantissa.Mul(mantissa, ten), big.NewInt(int64(rest[0]-'0')))
			if mantissa.BitLen() > 127 {
				return 0, malformed
			}
			exp10 -= 1
			seenDigit = true
			rest = rest[1:]
		}
	}
	if len(rest) > 0 && (rest[0] == 'e' || rest[0] == 'E') {
		expText := rest[1:]
		expNegative := false
		if len(expText) > 0 && expText[0] == '-' {
			expNegative = true
			expText = expText[1:]
		} else if len(expText) > 0 && expText[0] == '+' {
			expText = expText[1:]
		}
		if len(expText) == 0 {
			return 0, malformed
		}
		e := int64(0)
		for i := 0; i < len(expText); i++ {
			if expText[i] < '0' || expText[i] > '9' {
				return 0, malformed
			}
			e = e*10 + int64(expText[i]-'0')
			if e > 2147483648 {
				return 0, malformed
			}
		}
		if expNegative {
			e = -e
		} else if e > 2147483647 {
			return 0, malformed
		}
		exp10 += e
		if exp10 > 2147483647 || exp10 < -2147483648 {
			return 0, malformed
		}
	} else if len(rest) > 0 {
		return 0, malformed
	}
	if !seenDigit {
		return 0, malformed
	}
	if negative {
		mantissa.Neg(mantissa)
	}
	shift := exp10 + int64(scaleDigits)
	if shift > 2147483647 || shift < -2147483648 {
		return 0, outOfRange
	}
	var carrier *big.Int
	if shift >= 0 {
		if shift > 38 {
			return 0, outOfRange
		}
		carrier = mantissa.Mul(mantissa, decPow10(uint(shift)))
	} else {
		if -shift > 38 {
			return 0, ""
		}
		carrier = decRhe(mantissa, decPow10(uint(-shift)))
	}
	if !carrier.IsInt64() {
		return 0, outOfRange
	}
	return carrier.Int64(), ""
}

func DecParse(text string, scaleDigits int) int64 {
	value, failure := DecTryParse(text, scaleDigits)
	if failure != "" {
		panic(failure)
	}
	return value
}
