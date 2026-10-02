package rt

import (
	"reflect"
	"testing"
)

// TestSparsaStoresOnlyNonZero pins SparsaNew / Ponde / Accipe: a position reads
// zero until set, and storing the zero value removes the entry.
func TestSparsaStoresOnlyNonZero(t *testing.T) {
	s := SparsaNew[int64]([]int{3, 3})
	if s.Longitudo() != 2 || !reflect.DeepEqual(s.Magnitudines(), []int{3, 3}) || s.Nonnihil() != 0 {
		t.Fatalf("empty sparsa = rank %d shape %v nonnihil %d", s.Longitudo(), s.Magnitudines(), s.Nonnihil())
	}
	s.Ponde([]int{1, 2}, 5)
	s.Ponde([]uint32{0, 0}, 9)
	if s.Accipe([]int{1, 2}) != 5 || s.Accipe([]int{0, 0}) != 9 || s.Accipe([]int{2, 2}) != 0 {
		t.Fatalf("Accipe disagrees with Ponde")
	}
	if s.Nonnihil() != 2 {
		t.Fatalf("Nonnihil = %d", s.Nonnihil())
	}
	s.Ponde([]int{1, 2}, 0)
	if s.Nonnihil() != 1 {
		t.Fatalf("storing zero must delete the entry, Nonnihil = %d", s.Nonnihil())
	}
	wantPanic(t, "sparsa index rank does not match shape rank", func() { s.Accipe([]int{1}) })
	wantPanic(t, "sparsa index out of bounds", func() { s.Ponde([]int{3, 0}, 1) })
}

// TestSparsaZeroValueEnsuresItsMap pins that a sparsa built without SparsaNew
// (the zero value) still accepts Ponde once it has a shape.
func TestSparsaZeroValueEnsuresItsMap(t *testing.T) {
	s := SparsaSparsa[int64]{shape: []int{2}}
	s.Ponde([]int{1}, 4)
	if s.Accipe([]int{1}) != 4 {
		t.Fatalf("zero-value sparsa did not store")
	}
}

// TestSparsaDenseRoundTrip pins TensorToSparsa and Densata.
func TestSparsaDenseRoundTrip(t *testing.T) {
	dense := tensorOf([]int64{0, 2, 0, 0, 0, 7}, []int{2, 3})
	sparse := TensorToSparsa(dense)
	if sparse.Nonnihil() != 2 || sparse.Accipe([]int{0, 1}) != 2 || sparse.Accipe([]int{1, 2}) != 7 {
		t.Fatalf("TensorToSparsa lost entries: nonnihil %d", sparse.Nonnihil())
	}
	back := sparse.Densata()
	if !reflect.DeepEqual(back.Planata(), dense.Planata()) || !reflect.DeepEqual(back.Magnitudines(), []int{2, 3}) {
		t.Fatalf("Densata = %v %v", back.Planata(), back.Magnitudines())
	}
}
