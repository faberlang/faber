package rt

import (
	"fmt"
	"strconv"
	"strings"
)

// The Faber `sparsa` carrier for generated Go programs (codegen-readability
// T1-G7): a sparse n-dimensional array of non-zero entries over a fixed shape.
//
// Entries ride a map keyed by the printed index list; a zero value is never
// stored, so `Ponde` of the zero value deletes the entry. The carrier type and
// constructor are spelled by the compiler's canonical helper table
// (`rt.SparsaSparsa`, `rt.SparsaNew`); the dense-to-sparse conversion is
// `rt.TensorToSparsa`. The operations are methods carrying the Latin names the
// call sites use.

// SparsaSparsa is the sparse tensor carrier.
type SparsaSparsa[T comparable] struct {
	shape   []int
	entries map[string]T
}

// SparsaNew is the empty sparsa of a shape: every position reads as zero.
func SparsaNew[T comparable](shape []int) SparsaSparsa[T] {
	return SparsaSparsa[T]{shape: append([]int{}, shape...), entries: make(map[string]T)}
}

func sparsaKey(indices []int) string { return fmt.Sprint(indices) }

func (s *SparsaSparsa[T]) ensure() {
	if s.entries == nil {
		s.entries = make(map[string]T)
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
	return s.entries[sparsaKey(indices)]
}

func (s *SparsaSparsa[T]) Ponde(rawIndices any, value T) {
	indices := s.validate(rawIndices)
	s.ensure()
	key := sparsaKey(indices)
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
func TensorToSparsa[T comparable](dense TensorTensor[T]) SparsaSparsa[T] {
	entries := make(map[string]T)
	var zero T
	for offset, value := range dense.data {
		if value == zero {
			continue
		}
		indices := make([]int, len(dense.shape))
		remaining := offset
		for axis := len(dense.shape) - 1; axis >= 0; axis-- {
			dim := dense.shape[axis]
			if dim == 0 {
				break
			}
			indices[axis] = remaining % dim
			remaining /= dim
		}
		entries[sparsaKey(indices)] = value
	}
	return SparsaSparsa[T]{shape: append([]int{}, dense.shape...), entries: entries}
}

func (s SparsaSparsa[T]) Densata() TensorTensor[T] {
	data := make([]T, TensorElementCount(s.shape))
	for key, value := range s.entries {
		fields := strings.Fields(strings.Trim(key, "[]"))
		indices := make([]int, len(fields))
		for i, field := range fields {
			parsed, err := strconv.Atoi(field)
			if err != nil {
				panic(err)
			}
			indices[i] = parsed
		}
		offset := tensorOffset(s.shape, indices)
		if offset != nil {
			data[*offset] = value
		}
	}
	return TensorTensor[T]{data: data, shape: append([]int{}, s.shape...)}
}
