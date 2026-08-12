package rt

import (
	"strings"
	"testing"
	"time"
)

// TestVerum pins the bivalens surface used by generated Go display calls.
func TestVerum(t *testing.T) {
	if got := Verum(true); got != "verum" {
		t.Fatalf("Verum(true) = %q, want verum", got)
	}
	if got := Verum(false); got != "falsum" {
		t.Fatalf("Verum(false) = %q, want falsum", got)
	}
}

// TestListDisplay pins the `[a, b, c]` renderer with an element renderer.
func TestListDisplay(t *testing.T) {
	got := ListDisplay([]int{1, 2, 3}, func(v int) string {
		return Verum(v%2 == 0)
	})
	if got != "[falsum, verum, falsum]" {
		t.Fatalf("ListDisplay = %q, want [falsum, verum, falsum]", got)
	}
}

// TestValorDisplayScalars pins scalar rendering (nil, bivalens, numerus,
// fractus `.0` marker, textus, byte buffer, time.Time).
func TestValorDisplayScalars(t *testing.T) {
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
			if got := ValorDisplay(tc.value); got != tc.want {
				t.Fatalf("ValorDisplay(%v) = %q, want %q", tc.value, got, tc.want)
			}
		})
	}
}

// TestValorDisplayCollections pins map rendering (sorted quoted keys) and the
// per-type slice rendering through a boxed collection.
func TestValorDisplayCollections(t *testing.T) {
	got := ValorDisplay(map[string]any{"b": 2, "a": 1})
	if got != `{"a": 1, "b": 2}` {
		t.Fatalf("ValorDisplay(map) = %q, want {\"a\": 1, \"b\": 2}", got)
	}
	slice := ValorDisplay([]any{1, "x", true})
	if slice != `[1, x, verum]` {
		t.Fatalf("ValorDisplay(slice) = %q, want [1, x, verum]", slice)
	}
}

// faberTensorLike mirrors the emitted carrier shape so the reflection renderer
// can dispatch on the `faberTensor`/`faberVector` type-name convention.
type faberTensorLike struct {
	data []int
}

func (t faberTensorLike) Planata() []int {
	return t.data
}

// TestValorDisplayCarrier pins the tensor/vector carrier flat-data dispatch.
func TestValorDisplayCarrier(t *testing.T) {
	got := ValorDisplay(faberTensorLike{data: []int{1, 2}})
	if got != "[1, 2]" {
		t.Fatalf("ValorDisplay(carrier) = %q, want [1, 2]", got)
	}
}

// TestValorDisplayStruct pins genus-record rendering with lowercased quoted
// field names.
func TestValorDisplayStruct(t *testing.T) {
	type genus struct {
		Nomen   string
		Numerus int
	}
	got := ValorDisplay(genus{Nomen: "Marcus", Numerus: 3})
	// Field names are lowercased and quoted; field values render through
	// ValorDisplay (textus unquoted) — the frozen emit surface.
	if !strings.Contains(got, `"nomen": Marcus`) || !strings.Contains(got, `"numerus": 3`) {
		t.Fatalf("ValorDisplay(struct) = %q, want lowercased quoted names", got)
	}
}
