package rt

import (
	"math/big"
	"reflect"
	"testing"
)

// TestTupleKeyIsComparableAndRoundTrips pins that equal tuples make equal map
// keys and that the parts come back as a `[]any`.
func TestTupleKeyIsComparableAndRoundTrips(t *testing.T) {
	m := map[any]string{}
	m[TupleKey([]any{1, "a"})] = "x"
	if m[TupleKey([]any{1, "a"})] != "x" {
		t.Fatalf("equal tuples must be the same key")
	}
	if _, ok := m[TupleKey([]any{2, "a"})]; ok {
		t.Fatalf("different tuples must be different keys")
	}
	if got := TupleParts(TupleKey([]any{1, "a", true})); !reflect.DeepEqual(got, []any{1, "a", true}) {
		t.Fatalf("TupleParts = %v", got)
	}
}

// TestTupleKeyCanonicalisesInfParts pins that two equal `inf` values with
// distinct allocations are one key, and that the part comes back as a
// `*big.Int`.
func TestTupleKeyCanonicalisesInfParts(t *testing.T) {
	a := big.NewInt(1)
	a.Lsh(a, 100)
	b := new(big.Int).Set(a)
	if TupleKey([]any{a, 1}) != TupleKey([]any{b, 1}) {
		t.Fatalf("equal inf parts must be one key")
	}
	parts := TupleParts(TupleKey([]any{a}))
	n, ok := parts[0].(*big.Int)
	if !ok || n.Cmp(a) != 0 {
		t.Fatalf("inf part came back as %v", parts[0])
	}
}
