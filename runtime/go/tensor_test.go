package rt

import (
	"reflect"
	"testing"
)

func tensorOf[T any](data []T, shape []int) TensorTensor[T] {
	return TensorTensor[T]{}.Strue(data, shape)
}

// TestTensorConstruction pins Crea / Strue / the shape accessors and the
// copy-on-the-way-in rule.
func TestTensorConstruction(t *testing.T) {
	filled := TensorTensor[int64]{}.Crea(7, []int{2, 3})
	if !reflect.DeepEqual(filled.Planata(), []int64{7, 7, 7, 7, 7, 7}) {
		t.Fatalf("Crea data = %v", filled.Planata())
	}
	if !reflect.DeepEqual(filled.Magnitudines(), []int{2, 3}) || filled.Longitudo() != 2 {
		t.Fatalf("Crea shape = %v rank %d", filled.Magnitudines(), filled.Longitudo())
	}
	source := []int64{1, 2, 3, 4}
	built := tensorOf(source, []int{2, 2})
	source[0] = 99
	if built.Planata()[0] != 1 {
		t.Fatalf("Strue aliased the caller slice")
	}
	wantPanic(t, "tensor structa element count does not match shape", func() {
		tensorOf([]int64{1, 2, 3}, []int{2, 2})
	})
	wantPanic(t, "tensor shape dimension must be non-negative", func() {
		TensorElementCount([]int{2, -1})
	})
	wantPanic(t, "tensor shape element count overflow", func() {
		TensorElementCount([]int{1 << 62, 1 << 62})
	})
	if TensorElementCount([]int{}) != 1 || TensorElementCount([]int{3, 0}) != 0 {
		t.Fatalf("element counts of the empty and zero shapes are wrong")
	}
}

// TestTensorArithmetic pins elementwise arithmetic, matmul and the reductions.
func TestTensorArithmetic(t *testing.T) {
	a := tensorOf([]int64{1, 2, 3, 4}, []int{2, 2})
	b := tensorOf([]int64{5, 6, 7, 8}, []int{2, 2})
	if got := a.Addita(b).Planata(); !reflect.DeepEqual(got, []int64{6, 8, 10, 12}) {
		t.Fatalf("Addita = %v", got)
	}
	if got := b.Subtrahe(a).Planata(); !reflect.DeepEqual(got, []int64{4, 4, 4, 4}) {
		t.Fatalf("Subtrahe = %v", got)
	}
	if got := a.Multiplica(b).Planata(); !reflect.DeepEqual(got, []int64{5, 12, 21, 32}) {
		t.Fatalf("Multiplica = %v", got)
	}
	if got := a.Matmul(b).Planata(); !reflect.DeepEqual(got, []int64{19, 22, 43, 50}) {
		t.Fatalf("Matmul = %v", got)
	}
	if got := a.Summa(); got != 10 {
		t.Fatalf("Summa = %d", got)
	}
	if got := tensorOf([]float64{1, 2, 3, 6}, []int{4}).Media(); got != 3 {
		t.Fatalf("Media = %v", got)
	}
	wantPanic(t, "tensor elementwise arithmetic requires equal shapes", func() {
		a.Addita(tensorOf([]int64{1, 2, 3, 4}, []int{4}))
	})
	wantPanic(t, "tensor matmul requires compatible rank-2 shapes", func() {
		a.Matmul(tensorOf([]int64{1, 2, 3}, []int{3}))
	})
	wantPanic(t, "tensor media requires non-empty data", func() {
		tensorOf([]float64{}, []int{0}).Media()
	})
	wantPanic(t, "tensor media requires floating-point elements", func() {
		a.Media()
	})
	wantPanic(t, "tensor arithmetic requires numeric elements", func() {
		tensorOf([]string{"a"}, []int{1}).Summa()
	})
}

// TestTensorAccess pins Accipe / Ponde / Reple / Sectio / Forma / Materialize
// and every accepted index-list width.
func TestTensorAccess(t *testing.T) {
	m := tensorOf([]int64{1, 2, 3, 4, 5, 6}, []int{3, 2})
	if p := m.Accipe([]int{2, 1}); p == nil || *p != 6 {
		t.Fatalf("Accipe([2 1]) = %v", p)
	}
	for _, indices := range []any{[]uint32{1, 0}, []uint64{1, 0}, []int32{1, 0}, []int64{1, 0}} {
		if p := m.Accipe(indices); p == nil || *p != 3 {
			t.Fatalf("Accipe(%T) = %v", indices, p)
		}
	}
	if m.Accipe([]int{3, 0}) != nil || m.Accipe([]int{0}) != nil {
		t.Fatalf("an out-of-bounds or wrong-rank index must read nil")
	}
	wantPanic(t, "tensor index must be a numeric list", func() { m.Accipe("no") })
	if m.ReadAt([]int{2, 1}) != 6 {
		t.Fatalf("ReadAt([2 1]) must read the element")
	}
	wantPanic(t, "tensor accipe invalid index", func() { m.ReadAt([]int{3, 0}) })
	m.Ponde([]int{0, 1}, 20)
	if m.Planata()[1] != 20 {
		t.Fatalf("Ponde did not store")
	}
	wantPanic(t, "tensor ponde invalid index", func() { m.Ponde([]int{9, 9}, 1) })
	m.Reple(1)
	if !reflect.DeepEqual(m.Planata(), []int64{1, 1, 1, 1, 1, 1}) {
		t.Fatalf("Reple = %v", m.Planata())
	}
	grid := tensorOf([]int64{1, 2, 3, 4, 5, 6}, []int{3, 2})
	cut := grid.Sectio(1, 3)
	if !reflect.DeepEqual(cut.Planata(), []int64{3, 4, 5, 6}) || !reflect.DeepEqual(cut.Magnitudines(), []int{2, 2}) {
		t.Fatalf("Sectio = %v %v", cut.Planata(), cut.Magnitudines())
	}
	wantPanic(t, "tensor sectio invalid slice bounds", func() { grid.Sectio(2, 1) })
	if got := grid.Forma([]int{2, 3}).Magnitudines(); !reflect.DeepEqual(got, []int{2, 3}) {
		t.Fatalf("Forma shape = %v", got)
	}
	wantPanic(t, "tensor forma (reshape) element count mismatch", func() { grid.Forma([]int{4, 2}) })
	copyOf := grid.Materialize()
	copyOf.Reple(0)
	if grid.Planata()[0] != 1 {
		t.Fatalf("Materialize aliased the source")
	}
}

// TestTensorCheckedMethodForms pins the failable method forms: the element or
// the runner's text error; a store that fails leaves the tensor unchanged.
func TestTensorCheckedMethodForms(t *testing.T) {
	grid := tensorOf([]float32{1, 2, 3, 4}, []int{2, 2})
	if v, err := grid.AccipeChecked([]int{1, 0}); err != nil || v != 3 {
		t.Fatalf("AccipeChecked in range = %v, %v", v, err)
	}
	if _, err := grid.AccipeChecked([]int{2, 0}); err == nil || err.Error() != "tensor accipe invalid index" {
		t.Fatalf("AccipeChecked out of range error = %v", err)
	}
	if err := grid.PondeChecked([]int{0, 1}, 9); err != nil {
		t.Fatalf("PondeChecked in range = %v", err)
	}
	if v, _ := grid.AccipeChecked([]int{0, 1}); v != 9 {
		t.Fatalf("PondeChecked did not store: %v", v)
	}
	if err := grid.PondeChecked([]int{0, 5}, 1); err == nil || err.Error() != "tensor ponde invalid index" {
		t.Fatalf("PondeChecked out of range error = %v", err)
	}
}
