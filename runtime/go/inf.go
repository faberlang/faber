package rt

// The Faber `inf` carrier for generated Go programs (codegen-readability
// T1-G4): unbounded integers (`inf`, F9 rulings 32-46).
//
// An `inf` is a `*big.Int`, treated as immutable: every helper builds a fresh
// value and never writes through an argument, so sharing a pointer is safe. A
// nil pointer (an unset `inf` slot) reads as zero through `InfZ`. Division
// floors (`big.Int.Div` is Euclidean and `QuoRem` truncates, so the floor is
// corrected by hand); bitwise operators and shifts are the infinite
// two's-complement semantics `math/big` already defines. A width limit (the
// Store, Wrap and Clamp families) applies only where a value leaves the
// carrier into a bounded slot; nothing clamps or traps per operation except
// the two implementation ceilings of ruling 45. Every exported name is
// `Inf<Name>`, spelled by the compiler's canonical helper table.

import (
	"math"
	"math/big"
)

// InfPart is an `inf` inside a composite (tuple) map key: Go compares a
// `*big.Int` by pointer, so the part rides its canonical decimal string.
type InfPart string

var infMask64 = new(big.Int).SetUint64(^uint64(0))

func InfZ(a *big.Int) *big.Int {
	if a == nil {
		return new(big.Int)
	}
	return a
}

func InfI(v int64) *big.Int { return big.NewInt(v) }

func InfU(v uint64) *big.Int { return new(big.Int).SetUint64(v) }

func InfS(s string) *big.Int {
	v, ok := new(big.Int).SetString(s, 10)
	if !ok {
		panic("malformed inf literal")
	}
	return v
}

func InfSign(a *big.Int) int { return InfZ(a).Sign() }

// The two implementation ceilings of ruling 45, the same pair the MIR runner
// enforces: the bit length of any single inf value, and the schoolbook limb
// products of one multiplication. They are limits of the implementation, not
// language rules, and they bound a RESULT (never an operand count): a shift
// count is checked against the bits the shifted value would reach.
const infBitCeiling = 1 << 31
const infMulWorkCeiling = 1 << 27

func infExhausted(op string) {
	panic("inf size exhausted in " + op + ": the value would exceed the ceiling of 2147483648 bits")
}

func infGrew(v *big.Int, op string) *big.Int {
	if v.BitLen() > infBitCeiling {
		infExhausted(op)
	}
	return v
}

func InfAdd(a *big.Int, b *big.Int) *big.Int {
	return infGrew(new(big.Int).Add(InfZ(a), InfZ(b)), "addition")
}

func InfSub(a *big.Int, b *big.Int) *big.Int {
	return infGrew(new(big.Int).Sub(InfZ(a), InfZ(b)), "subtraction")
}

func infLimbs(a *big.Int) uint64 { return (uint64(a.BitLen()) + 31) / 32 }

func InfMul(a *big.Int, b *big.Int) *big.Int {
	a, b = InfZ(a), InfZ(b)
	if infLimbs(a)*infLimbs(b) > infMulWorkCeiling {
		panic("inf size exhausted in multiplication: the work exceeds the ceiling of 134217728 limb products")
	}
	if a.Sign() != 0 && b.Sign() != 0 && uint64(a.BitLen())+uint64(b.BitLen())-1 > infBitCeiling {
		infExhausted("multiplication")
	}
	return infGrew(new(big.Int).Mul(a, b), "multiplication")
}

func InfNeg(a *big.Int) *big.Int { return new(big.Int).Neg(InfZ(a)) }

func InfAbs(a *big.Int) *big.Int { return new(big.Int).Abs(InfZ(a)) }

func InfNot(a *big.Int) *big.Int { return new(big.Int).Not(InfZ(a)) }

func InfAnd(a *big.Int, b *big.Int) *big.Int {
	return new(big.Int).And(InfZ(a), InfZ(b))
}

func InfOr(a *big.Int, b *big.Int) *big.Int {
	return new(big.Int).Or(InfZ(a), InfZ(b))
}

func InfXor(a *big.Int, b *big.Int) *big.Int {
	return new(big.Int).Xor(InfZ(a), InfZ(b))
}

func InfDiv(a *big.Int, b *big.Int) *big.Int {
	b = InfZ(b)
	if b.Sign() == 0 {
		panic("numerus division failed")
	}
	q, r := new(big.Int).QuoRem(InfZ(a), b, new(big.Int))
	if r.Sign() != 0 && (r.Sign() < 0) != (b.Sign() < 0) {
		q.Sub(q, big.NewInt(1))
	}
	return q
}

func InfMod(a *big.Int, b *big.Int) *big.Int {
	b = InfZ(b)
	if b.Sign() == 0 {
		panic("numerus division failed")
	}
	r := new(big.Int).Rem(InfZ(a), b)
	if r.Sign() != 0 && (r.Sign() < 0) != (b.Sign() < 0) {
		r.Add(r, b)
	}
	return r
}

func InfShl(a *big.Int, n *big.Int) *big.Int {
	n = InfZ(n)
	if n.Sign() < 0 {
		panic("negative shift count")
	}
	a = InfZ(a)
	if a.Sign() == 0 {
		return new(big.Int)
	}
	if !n.IsUint64() || n.Uint64() > infBitCeiling || uint64(a.BitLen())+n.Uint64() > infBitCeiling {
		infExhausted("shift")
	}
	return new(big.Int).Lsh(a, uint(n.Uint64()))
}

func InfShr(a *big.Int, n *big.Int) *big.Int {
	n = InfZ(n)
	if n.Sign() < 0 {
		panic("negative shift count")
	}
	a = InfZ(a)
	if !n.IsUint64() || n.Uint64() > 1<<31 {
		if a.Sign() < 0 {
			return big.NewInt(-1)
		}
		return new(big.Int)
	}
	return new(big.Int).Rsh(a, uint(n.Uint64()))
}

// InfPow is right-to-left binary exponentiation over InfMul, the
// runner's own lowering, so a runaway power ends at the same multiplication
// ceiling instead of allocating or spinning.
func InfPow(base *big.Int, exponent *big.Int) *big.Int {
	exponent = InfZ(exponent)
	if exponent.Sign() < 0 {
		panic("numerus potentia failed: negative exponent")
	}
	acc := big.NewInt(1)
	b := InfZ(base)
	e := new(big.Int).Set(exponent)
	for e.Sign() > 0 {
		if e.Bit(0) == 1 {
			acc = InfMul(acc, b)
		}
		e.Rsh(e, 1)
		if e.Sign() > 0 {
			b = InfMul(b, b)
		}
	}
	return acc
}

func InfSum(items []*big.Int) *big.Int {
	total := new(big.Int)
	for _, item := range items {
		total.Add(total, InfZ(item))
		if total.BitLen() > infBitCeiling {
			infExhausted("summa")
		}
	}
	return total
}

func InfSignum(a *big.Int) *big.Int { return big.NewInt(int64(InfZ(a).Sign())) }

func InfMax(a *big.Int, b *big.Int) *big.Int {
	if InfCmp(a, b) >= 0 {
		return InfZ(a)
	}
	return InfZ(b)
}

func InfMin(a *big.Int, b *big.Int) *big.Int {
	if InfCmp(a, b) <= 0 {
		return InfZ(a)
	}
	return InfZ(b)
}

// InfListEq is list equality over inf elements: value equality, never
// pointer identity.
func InfListEq(a []*big.Int, b []*big.Int) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if InfCmp(a[i], b[i]) != 0 {
			return false
		}
	}
	return true
}

func InfCmp(a *big.Int, b *big.Int) int { return InfZ(a).Cmp(InfZ(b)) }

func infCmpF(a *big.Int, f float64) (int, bool) {
	if math.IsNaN(f) {
		return 0, false
	}
	if math.IsInf(f, 1) {
		return -1, true
	}
	if math.IsInf(f, -1) {
		return 1, true
	}
	return new(big.Float).SetInt(InfZ(a)).Cmp(new(big.Float).SetFloat64(f)), true
}

func InfLtF(a *big.Int, f float64) bool { c, ok := infCmpF(a, f); return ok && c < 0 }

func InfLeF(a *big.Int, f float64) bool { c, ok := infCmpF(a, f); return ok && c <= 0 }

func InfGtF(a *big.Int, f float64) bool { c, ok := infCmpF(a, f); return ok && c > 0 }

func InfGeF(a *big.Int, f float64) bool { c, ok := infCmpF(a, f); return ok && c >= 0 }

func InfEqF(a *big.Int, f float64) bool { c, ok := infCmpF(a, f); return ok && c == 0 }

// InfApprox is `≈` on integers: 10^9 * |a - b| <= max(|a|, |b|), exactly.
func InfApprox(a *big.Int, b *big.Int) bool {
	a, b = InfZ(a), InfZ(b)
	scaled := new(big.Int).Sub(a, b)
	scaled.Abs(scaled).Mul(scaled, big.NewInt(1000000000))
	limit := new(big.Int).Abs(a)
	if other := new(big.Int).Abs(b); other.Cmp(limit) > 0 {
		limit = other
	}
	return scaled.Cmp(limit) <= 0
}

func InfF64(a *big.Int) float64 {
	f, _ := new(big.Float).SetInt(InfZ(a)).Float64()
	return f
}

func InfF32(a *big.Int) float32 {
	f, _ := new(big.Float).SetInt(InfZ(a)).Float32()
	return f
}

func InfFromF(f float64) (*big.Int, bool) {
	if math.IsNaN(f) || math.IsInf(f, 0) {
		return nil, false
	}
	z, _ := new(big.Float).SetFloat64(f).Int(nil)
	return z, true
}

func InfParse(s string, base int) (*big.Int, bool) {
	return new(big.Int).SetString(s, base)
}

func InfStr(a *big.Int) string { return InfZ(a).String() }

// InfFromValor is `valor ↦ inf`: every integer carrier, including a
// `*big.Int`, converts exactly; any other dynamic value fails.
func InfFromValor(v any) (*big.Int, bool) {
	switch n := v.(type) {
	case *big.Int:
		return InfZ(n), true
	case int:
		return InfI(int64(n)), true
	case int8:
		return InfI(int64(n)), true
	case int16:
		return InfI(int64(n)), true
	case int32:
		return InfI(int64(n)), true
	case int64:
		return InfI(n), true
	case uint:
		return InfU(uint64(n)), true
	case uint8:
		return InfU(uint64(n)), true
	case uint16:
		return InfU(uint64(n)), true
	case uint32:
		return InfU(uint64(n)), true
	case uint64:
		return InfU(n), true
	}
	return nil, false
}

// InfFromBytes is `octeti ↦ inf via Be|Le`: minimal two's complement of
// any length, most significant byte first when be; empty input is zero.
func InfFromBytes(b []byte, be bool) *big.Int {
	if len(b) == 0 {
		return new(big.Int)
	}
	buf := append([]byte(nil), b...)
	if !be {
		for i, j := 0, len(buf)-1; i < j; i, j = i+1, j-1 {
			buf[i], buf[j] = buf[j], buf[i]
		}
	}
	v := new(big.Int).SetBytes(buf)
	if buf[0]&0x80 != 0 {
		v.Sub(v, new(big.Int).Lsh(big.NewInt(1), uint(8*len(buf))))
	}
	return v
}

// InfToBytes is `inf ↦ octeti via Be|Le`: minimal two's complement (zero
// is `00`; a positive value with its top bit set gets a leading `00`).
func InfToBytes(a *big.Int, be bool) []byte {
	a = InfZ(a)
	magnitude := a
	if a.Sign() < 0 {
		magnitude = new(big.Int).Not(a)
	}
	n := magnitude.BitLen()/8 + 1
	mod := new(big.Int).Lsh(big.NewInt(1), uint(8*n))
	out := new(big.Int).Mod(a, mod).FillBytes(make([]byte, n))
	if !be {
		for i, j := 0, len(out)-1; i < j; i, j = i+1, j-1 {
			out[i], out[j] = out[j], out[i]
		}
	}
	return out
}

// InfPack is `x ↦ ascii<N> via Hex|Bin|Oct`: zero-padded digits, failing
// for a negative value or one with more than width digits.
func InfPack(a *big.Int, base int, width int) (string, bool) {
	a = InfZ(a)
	if a.Sign() < 0 {
		return "", false
	}
	digits := a.Text(base)
	if len(digits) > width {
		return "", false
	}
	for len(digits) < width {
		digits = "0" + digits
	}
	return digits, true
}

func InfFitsI(a *big.Int, lo int64, hi int64) bool {
	a = InfZ(a)
	return a.IsInt64() && a.Int64() >= lo && a.Int64() <= hi
}

func InfFitsU(a *big.Int, hi uint64) bool {
	a = InfZ(a)
	return a.IsUint64() && a.Uint64() <= hi
}

func infTrap(a *big.Int, ty string, pos string, extra string, unsigned bool) {
	msg := InfStr(a) + " does not fit in `" + ty + "` (" + pos + ")" + extra
	if unsigned && InfSign(a) < 0 {
		msg += " (a negative value cannot be stored in an unsigned slot)"
	}
	panic(msg)
}

func InfStoreI(a *big.Int, lo int64, hi int64, ty string, pos string, extra string) int64 {
	if !InfFitsI(a, lo, hi) {
		infTrap(a, ty, pos, extra, false)
	}
	return InfZ(a).Int64()
}

func InfStoreU(a *big.Int, hi uint64, ty string, pos string, extra string) uint64 {
	if !InfFitsU(a, hi) {
		infTrap(a, ty, pos, extra, true)
	}
	return InfZ(a).Uint64()
}

func InfWrap(a *big.Int) uint64 {
	return new(big.Int).And(InfZ(a), infMask64).Uint64()
}

func InfClampI(a *big.Int, lo int64, hi int64) int64 {
	a = InfZ(a)
	if a.Cmp(big.NewInt(lo)) < 0 {
		return lo
	}
	if a.Cmp(big.NewInt(hi)) > 0 {
		return hi
	}
	return a.Int64()
}

func InfClampU(a *big.Int, hi uint64) uint64 {
	a = InfZ(a)
	if a.Sign() < 0 {
		return 0
	}
	if a.Cmp(InfU(hi)) > 0 {
		return hi
	}
	return a.Uint64()
}

// InfToDec is `inf ↦ d64`: the value at scale 8, which must fit int64.
func InfToDec(a *big.Int) (int64, bool) {
	scaled := new(big.Int).Mul(InfZ(a), big.NewInt(100000000))
	if !scaled.IsInt64() {
		return 0, false
	}
	return scaled.Int64(), true
}

// InfKey and InfFromKey are the map-key wrapper: a `tabula<inf, V>`
// or `copia<inf>` is a Go map keyed by the canonical decimal string, because a
// `map[*big.Int]V` would compare by pointer identity.
func InfKey(a *big.Int) string { return InfStr(a) }

func InfFromKey(key string) *big.Int { return InfS(key) }
