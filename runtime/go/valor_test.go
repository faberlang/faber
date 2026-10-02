package rt

import "testing"

type valorBox struct{ n int }

func decodeValorBox(m map[string]any) valorBox {
	n, _ := m["n"].(int)
	return valorBox{n: n}
}

// TestValorStructDecoder pins the three paths: an already-typed value passes
// through, a map is decoded by the callback, anything else reports failure.
func TestValorStructDecoder(t *testing.T) {
	if got, ok := ValorStruct(any(valorBox{n: 7}), decodeValorBox); !ok || got.n != 7 {
		t.Fatalf("typed passthrough = %v, %v", got, ok)
	}
	if got, ok := ValorStruct(any(map[string]any{"n": 9}), decodeValorBox); !ok || got.n != 9 {
		t.Fatalf("map decode = %v, %v", got, ok)
	}
	if got, ok := ValorStruct(any("text"), decodeValorBox); ok || got.n != 0 {
		t.Fatalf("a non-map, non-typed value must fail, got %v, %v", got, ok)
	}
}
