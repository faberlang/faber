package rt

import (
	"math"
	"math/big"
	"testing"
)

type ordered struct{ n int }

func (o *ordered) Compare(other ordered) int {
	if o.n < other.n {
		return -1
	}
	if o.n > other.n {
		return 1
	}
	return 0
}

// TestOrderCompareBuiltins pins integers, strings, floats (IEEE totalOrder),
// big integers and tuples (lexicographic).
func TestOrderCompareBuiltins(t *testing.T) {
	if OrderCompare(1, 2) != -1 || OrderCompare(int8(3), int8(3)) != 0 || OrderCompare(uint(5), uint(4)) != 1 {
		t.Fatalf("integer order is wrong")
	}
	if OrderCompare("a", "b") != -1 || OrderCompare("b", "a") != 1 || OrderCompare("a", "a") != 0 {
		t.Fatalf("string order is wrong")
	}
	if OrderCompare(math.Inf(-1), -1.0) != -1 || OrderCompare(0.0, math.Copysign(0, -1)) != 1 {
		t.Fatalf("float totalOrder is wrong (negative zero sorts below positive zero)")
	}
	if OrderCompare(float32(1), float32(2)) != -1 {
		t.Fatalf("float32 order is wrong")
	}
	if OrderCompare(big.NewInt(3), big.NewInt(2)) != 1 {
		t.Fatalf("inf order is wrong")
	}
	if OrderCompare([]any{1, "b"}, []any{1, "a"}) != 1 || OrderCompare([]any{1, "a"}, []any{2, "a"}) != -1 || OrderCompare([]any{1}, []any{1}) != 0 {
		t.Fatalf("tuple order is not lexicographic")
	}
}

// TestOrderOrderPrefersTheGenusCompare pins that a genus with its own
// `Compare` is ordered by it, and a built-in falls back to OrderCompare.
func TestOrderOrderPrefersTheGenusCompare(t *testing.T) {
	if OrderOrder(ordered{1}, ordered{2}) != -1 || OrderOrder(ordered{2}, ordered{1}) != 1 || OrderOrder(ordered{2}, ordered{2}) != 0 {
		t.Fatalf("genus Compare was not used")
	}
	if OrderOrder(3, 2) != 1 || OrderOrder("a", "b") != -1 {
		t.Fatalf("built-in fallback is wrong")
	}
	// Through OrderCompare's reflective tail (a non-pointer genus value).
	if OrderCompare(ordered{1}, ordered{3}) != -1 {
		t.Fatalf("reflective Compare dispatch is wrong")
	}
}
