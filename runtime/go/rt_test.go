package rt

import (
	"math/big"
	"strconv"
	"strings"
	"testing"
	"time"
)

// TestDisplayVerum pins the bivalens surface used by generated Go display calls.
func TestDisplayVerum(t *testing.T) {
	if got := DisplayVerum(true); got != "verum" {
		t.Fatalf("DisplayVerum(true) = %q, want verum", got)
	}
	if got := DisplayVerum(false); got != "falsum" {
		t.Fatalf("DisplayVerum(false) = %q, want falsum", got)
	}
}

// TestDisplayList pins the `[a, b, c]` renderer with an element renderer.
func TestDisplayList(t *testing.T) {
	got := DisplayList([]int{1, 2, 3}, func(v int) string {
		return DisplayVerum(v%2 == 0)
	})
	if got != "[falsum, verum, falsum]" {
		t.Fatalf("DisplayList = %q, want [falsum, verum, falsum]", got)
	}
}

// TestMapDisplay pins the `{"k": v}` renderer with key and value renderers.
func TestMapDisplay(t *testing.T) {
	quote := func(k string) string { return strconv.Quote(k) }
	one := map[string]int{"a": 1}
	if got := MapDisplay(one, quote, strconv.Itoa); got != `{"a": 1}` {
		t.Fatalf("MapDisplay = %q, want {\"a\": 1}", got)
	}
	if got := MapDisplay(map[string]int{}, quote, strconv.Itoa); got != "{}" {
		t.Fatalf("MapDisplay(empty) = %q, want {}", got)
	}
	dec := map[string]int64{"a": 125000000}
	got := MapDisplay(dec, quote, func(v int64) string { return strconv.FormatFloat(float64(v)/1e8, 'f', -1, 64) })
	if got != `{"a": 1.25}` {
		t.Fatalf("MapDisplay(decimal) = %q, want {\"a\": 1.25}", got)
	}
}

// TestDisplayValorScalars pins scalar rendering (nil, bivalens, numerus,
// fractus `.0` marker, textus, byte buffer, time.Time).
func TestDisplayValorScalars(t *testing.T) {
	cases := []struct {
		name  string
		value any
		want  string
	}{
		{"nil", nil, "nihil"},
		{"bool verum", true, "verum"},
		{"bool falsum", false, "falsum"},
		{"int", 42, "42"},
		{"int64", int64(-7), "-7"},
		{"uint", uint(9), "9"},
		{"fractus integer", 3.0, "3.0"},
		{"fractus fractional", 3.5, "3.5"},
		{"textus", "salve", "salve"},
		{"byte buffer", []byte{65, 66}, "[65, 66]"},
		{"time", time.Date(2026, 8, 12, 0, 0, 0, 0, time.UTC), "2026-08-12T00:00:00Z"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := DisplayValor(tc.value); got != tc.want {
				t.Fatalf("DisplayValor(%v) = %q, want %q", tc.value, got, tc.want)
			}
		})
	}
}

// TestDisplayValorCollections pins map rendering (sorted quoted keys) and the
// per-type slice rendering through a boxed collection.
func TestDisplayValorCollections(t *testing.T) {
	got := DisplayValor(map[string]any{"b": 2, "a": 1})
	if got != `{"a": 1, "b": 2}` {
		t.Fatalf("DisplayValor(map) = %q, want {\"a\": 1, \"b\": 2}", got)
	}
	slice := DisplayValor([]any{1, "x", true})
	if slice != `[1, x, verum]` {
		t.Fatalf("DisplayValor(slice) = %q, want [1, x, verum]", slice)
	}
}

// TestDisplayValorCarrier pins the tensor/vector carrier flat-data dispatch:
// the reflection renderer reads the carriers of this package through their
// `Planata` / `AdLista` accessors.
func TestDisplayValorCarrier(t *testing.T) {
	tensor := TensorTensor[int]{}.Strue([]int{1, 2}, []int{2})
	if got := DisplayValor(tensor); got != "[1, 2]" {
		t.Fatalf("DisplayValor(tensor) = %q, want [1, 2]", got)
	}
	vector := VectorFromList([]int{3, 4, 5}, 3)
	if got := DisplayValor(vector); got != "[3, 4, 5]" {
		t.Fatalf("DisplayValor(vector) = %q, want [3, 4, 5]", got)
	}
}

// TestDisplayValorStruct pins genus-record rendering with lowercased quoted
// field names.
func TestDisplayValorStruct(t *testing.T) {
	type genus struct {
		Nomen   string
		Numerus int
	}
	got := DisplayValor(genus{Nomen: "Marcus", Numerus: 3})
	// Field names are lowercased and quoted; field values render through
	// DisplayValor (textus unquoted) — the frozen emit surface.
	if !strings.Contains(got, `"nomen": Marcus`) || !strings.Contains(got, `"numerus": 3`) {
		t.Fatalf("DisplayValor(struct) = %q, want lowercased quoted names", got)
	}
}

// TestDisplayValorBigInt pins the unbounded `inf` carrier: a `*big.Int`
// renders its decimal digits, on its own and inside a boxed collection or
// record, and an unset (nil) slot reads as zero.
func TestDisplayValorBigInt(t *testing.T) {
	huge, _ := new(big.Int).SetString("18446744073709551616", 10)
	negative, _ := new(big.Int).SetString("-340282366920938463463374607431768211456", 10)
	if got := DisplayValor(huge); got != "18446744073709551616" {
		t.Fatalf("DisplayValor(*big.Int) = %q, want 18446744073709551616", got)
	}
	if got := DisplayValor(negative); got != "-340282366920938463463374607431768211456" {
		t.Fatalf("DisplayValor(negative *big.Int) = %q", got)
	}
	if got := DisplayValor((*big.Int)(nil)); got != "0" {
		t.Fatalf("DisplayValor(nil *big.Int) = %q, want 0", got)
	}
	if got := DisplayValor([]any{huge, 1}); got != "[18446744073709551616, 1]" {
		t.Fatalf("DisplayValor(boxed list) = %q", got)
	}
	if got := DisplayValor([]*big.Int{huge}); got != "[18446744073709551616]" {
		t.Fatalf("DisplayValor([]*big.Int) = %q", got)
	}
	type genus struct{ V *big.Int }
	if got := DisplayValor(genus{V: huge}); got != `{"v": 18446744073709551616}` {
		t.Fatalf("DisplayValor(record) = %q", got)
	}
}

// TestDisplayTokensEnglish pins the English token variants: bivalens, nil and
// nested collections print true/false/none, and the Latin entry points are
// unchanged.
func TestDisplayTokensEnglish(t *testing.T) {
	en := DisplayTokens{True: "true", False: "false", None: "none"}
	if got := DisplayVerumTokens(true, en); got != "true" {
		t.Fatalf("DisplayVerumTokens(true) = %q, want true", got)
	}
	if got := DisplayVerumTokens(false, en); got != "false" {
		t.Fatalf("DisplayVerumTokens(false) = %q, want false", got)
	}
	if got := DisplayValorTokens(nil, en); got != "none" {
		t.Fatalf("DisplayValorTokens(nil) = %q, want none", got)
	}
	if got := DisplayValorTokens([]any{true, nil}, en); got != "[true, none]" {
		t.Fatalf("DisplayValorTokens(list) = %q, want [true, none]", got)
	}
	if got := DisplayValorTokens(map[string]any{"k": false}, en); got != `{"k": false}` {
		t.Fatalf("DisplayValorTokens(map) = %q", got)
	}
	if got := DisplayValor([]any{true, nil}); got != "[verum, nihil]" {
		t.Fatalf("DisplayValor(list) = %q, want [verum, nihil]", got)
	}
}
