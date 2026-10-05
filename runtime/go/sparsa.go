package rt

// The Faber `sparsa` carrier for generated Go programs (codegen-readability
// T1-G7): a sparse n-dimensional array of non-zero entries over a fixed shape.
//
// Entries ride a map keyed by the flat row-major offset of the index list,
// encoded through the shape's strides (tensorOffset) and decoded by the
// identity scatter into the dense buffer (Densata); a zero value is never
// stored, so `Ponde` of the zero value deletes the entry. The carrier type and
// constructor are spelled by the compiler's canonical helper table
// (`rt.SparsaSparsa`, `rt.SparsaNew`); the dense-to-sparse conversion is
// `rt.TensorToSparsa`. The operations are methods carrying the Latin names the
// call sites use.

// SparsaSparsa is the sparse tensor carrier.
type SparsaSparsa[T comparable] struct {
	shape   []int
	entries map[int]T
}

// SparsaNew is the empty sparsa of a shape: every position reads as zero.
func SparsaNew[T comparable](shape []int) SparsaSparsa[T] {
	return SparsaSparsa[T]{shape: append([]int{}, shape...), entries: make(map[int]T)}
}

// sparsaKey is the flat row-major offset of an already-validated index list.
// Offsets are never negative by tensorOffset's construction; for in-bounds
// indices the only failure left is a shape whose element count does not fit an
// int, the trap Densata raises for such shapes anyway.
func (s SparsaSparsa[T]) sparsaKey(indices []int) int {
	offset, ok := tensorOffset(s.shape, indices)
	if !ok {
		panic("tensor shape element count overflow")
	}
	return offset
}

func (s *SparsaSparsa[T]) ensure() {
	if s.entries == nil {
		s.entries = make(map[int]T)
	}
}

func (s SparsaSparsa[T]) validate(rawIndices any) []int {
	indices := indexSlice(rawIndices)
	if len(indices) != len(s.shape) {
		panic("sparsa index rank does not match shape rank")
	}
	for axis, idx := range indices {
		if idx < 0 || idx >= s.shape[axis] {
			panic("sparsa index out of bounds")
		}
	}
	return indices
}

func (s SparsaSparsa[T]) Accipe(rawIndices any) T {
	indices := s.validate(rawIndices)
	return s.entries[s.sparsaKey(indices)]
}

func (s *SparsaSparsa[T]) Ponde(rawIndices any, value T) {
	indices := s.validate(rawIndices)
	s.ensure()
	key := s.sparsaKey(indices)
	var zero T
	if value == zero {
		delete(s.entries, key)
	} else {
		s.entries[key] = value
	}
}

func (s SparsaSparsa[T]) Longitudo() int { return len(s.shape) }

func (s SparsaSparsa[T]) Magnitudines() []int { return append([]int{}, s.shape...) }

func (s SparsaSparsa[T]) Nonnihil() int { return len(s.entries) }

// TensorToSparsa is the sparse form of a dense tensor: its non-zero elements.
// The dense walk's ordinal is already the flat row-major offset, so it keys
// the map directly.
func TensorToSparsa[T comparable](dense TensorTensor[T]) SparsaSparsa[T] {
	entries := make(map[int]T)
	var zero T
	for offset, value := range dense.data {
		if value == zero {
			continue
		}
		entries[offset] = value
	}
	return SparsaSparsa[T]{shape: append([]int{}, dense.shape...), entries: entries}
}

func (s SparsaSparsa[T]) Densata() TensorTensor[T] {
	data := make([]T, TensorElementCount(s.shape))
	// Entry offsets were validated in bounds against this shape, which never
	// mutates, so the decode is a direct scatter instead of a key parse.
	for offset, value := range s.entries {
		data[offset] = value
	}
	return TensorTensor[T]{data: data, shape: append([]int{}, s.shape...)}
}
