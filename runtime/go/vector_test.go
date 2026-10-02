package rt

import (
	"reflect"
	"testing"
)

// TestVectorFromListChecksWidth pins the construction width check and the
// copy-on-the-way-in rule.
func TestVectorFromListChecksWidth(t *testing.T) {
	source := []int64{1, 2, 3}
	v := VectorFromList(source, 3)
	source[0] = 99
	if !reflect.DeepEqual(v.AdLista(), []int64{1, 2, 3}) {
		t.Fatalf("AdLista = %v", v.AdLista())
	}
	wantPanic(t, "vector width mismatch", func() { VectorFromList([]int64{1, 2}, 3) })
}

// TestVectorArithmetic pins elementwise arithmetic, dot, cross and the width
// checks.
func TestVectorArithmetic(t *testing.T) {
	a := VectorFromList([]int64{1, 2, 3}, 3)
	b := VectorFromList([]int64{4, 5, 6}, 3)
	if got := a.Addita(b).AdLista(); !reflect.DeepEqual(got, []int64{5, 7, 9}) {
		t.Fatalf("Addita = %v", got)
	}
	if got := b.Subtrahe(a).AdLista(); !reflect.DeepEqual(got, []int64{3, 3, 3}) {
		t.Fatalf("Subtrahe = %v", got)
	}
	if got := a.Multiplica(b).AdLista(); !reflect.DeepEqual(got, []int64{4, 10, 18}) {
		t.Fatalf("Multiplica = %v", got)
	}
	if got := b.Divida(a).AdLista(); !reflect.DeepEqual(got, []int64{4, 2, 2}) {
		t.Fatalf("Divida = %v", got)
	}
	if got := a.Productum(b); got != 32 {
		t.Fatalf("Productum = %d", got)
	}
	if got := a.Transversum(b).AdLista(); !reflect.DeepEqual(got, []int64{-3, 6, -3}) {
		t.Fatalf("Transversum = %v", got)
	}
	short := VectorFromList([]int64{1, 2}, 2)
	wantPanic(t, "vector elementwise arithmetic requires equal widths", func() { a.Addita(short) })
	wantPanic(t, "vector dot product requires equal widths", func() { a.Productum(short) })
	wantPanic(t, "vector cross product requires width 3", func() { short.Transversum(short) })
	wantPanic(t, "vector arithmetic requires numeric elements", func() {
		VectorFromList([]string{"a"}, 1).Addita(VectorFromList([]string{"b"}, 1))
	})
}

// TestVectorSwizzle pins the xyzw mask and its range checks.
func TestVectorSwizzle(t *testing.T) {
	v := VectorFromList([]int64{10, 20, 30}, 3)
	if got := v.Swizzle("zyx").AdLista(); !reflect.DeepEqual(got, []int64{30, 20, 10}) {
		t.Fatalf("Swizzle = %v", got)
	}
	wantPanic(t, "vector swizzle mask out of range", func() { v.Swizzle("w") })
	wantPanic(t, "vector swizzle mask out of range", func() { v.Swizzle("q") })
}
