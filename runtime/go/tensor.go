package rt

// The Faber `tensor` carrier for generated Go programs (codegen-readability
// T1-G7): a dense row-major n-dimensional array with a runtime shape.
//
// A tensor is a flat data slice plus a shape. Go generics cannot bound `T` to
// the numeric operators across the full primitive set a tensor carries, so the
// arithmetic helpers dispatch on the element type. Every method copies on the
// way in and out: a tensor value never aliases the slice a caller passed or
// received. The operations are methods carrying the Latin names the call sites
// use (`Crea`, `Addita`, `Matmul`, ...); the exported free functions and the
// carrier type are spelled by the compiler's canonical helper table
// (`rt.TensorTensor`, `rt.TensorElementCount`, `rt.TensorToSparsa`).
//
// No operation here is a store: arithmetic computes in the element type, and a
// width limit applies only where a result leaves into a bounded cell.

// TensorTensor is the dense tensor carrier.
type TensorTensor[T any] struct {
	data  []T
	shape []int
}

// TensorElementCount is the number of elements a shape holds. It traps on a
// negative dimension and on an element count that overflows an int.
func TensorElementCount(shape []int) int {
	const maxInt = int(^uint(0) >> 1)
	total := 1
	for _, dim := range shape {
		if dim < 0 {
			panic("tensor shape dimension must be non-negative")
		}
		if dim > 0 && total > maxInt/dim {
			panic("tensor shape element count overflow")
		}
		total *= dim
	}
	return total
}

// indexSlice reads a numeric index list of any element width into []int.
func indexSlice(indices any) []int {
	switch values := indices.(type) {
	case []int:
		return append([]int{}, values...)
	case []uint32:
		out := make([]int, len(values))
		for i, value := range values {
			out[i] = int(value)
		}
		return out
	case []uint64:
		out := make([]int, len(values))
		for i, value := range values {
			out[i] = int(value)
		}
		return out
	case []int32:
		out := make([]int, len(values))
		for i, value := range values {
			out[i] = int(value)
		}
		return out
	case []int64:
		out := make([]int, len(values))
		for i, value := range values {
			out[i] = int(value)
		}
		return out
	default:
		panic("tensor index must be a numeric list")
	}
}

// tensorOffset is the row-major offset of an index list, or nil when the rank
// differs or an index is out of bounds.
func tensorOffset(shape []int, rawIndices any) *int {
	const maxInt = int(^uint(0) >> 1)
	indices := indexSlice(rawIndices)
	if len(indices) != len(shape) {
		return nil
	}
	offset := 0
	stride := 1
	for axis := len(shape) - 1; axis >= 0; axis-- {
		idx := indices[axis]
		dim := shape[axis]
		if dim < 0 || idx < 0 || idx >= dim {
			return nil
		}
		if idx > 0 && stride > (maxInt-offset)/idx {
			return nil
		}
		offset += idx * stride
		if dim > 0 && stride > maxInt/dim {
			return nil
		}
		stride *= dim
	}
	return &offset
}

func (t TensorTensor[T]) Crea(fill T, shape []int) TensorTensor[T] {
	data := make([]T, TensorElementCount(shape))
	for i := range data {
		data[i] = fill
	}
	return TensorTensor[T]{data: data, shape: append([]int{}, shape...)}
}

func (t TensorTensor[T]) Strue(data []T, shape []int) TensorTensor[T] {
	if TensorElementCount(shape) != len(data) {
		panic("tensor structa element count does not match shape")
	}
	return TensorTensor[T]{data: append([]T{}, data...), shape: append([]int{}, shape...)}
}

func (t TensorTensor[T]) Longitudo() int { return len(t.shape) }

func (t TensorTensor[T]) Magnitudines() []int { return append([]int{}, t.shape...) }

func (t TensorTensor[T]) Planata() []T { return append([]T{}, t.data...) }

func (t TensorTensor[T]) Materialize() TensorTensor[T] {
	return TensorTensor[T]{data: append([]T{}, t.data...), shape: append([]int{}, t.shape...)}
}

func tensorAdd[T any](left T, right T) T {
	switch value := any(left).(type) {
	case int:
		return any(value + any(right).(int)).(T)
	case int32:
		return any(value + any(right).(int32)).(T)
	case int64:
		return any(value + any(right).(int64)).(T)
	case uint:
		return any(value + any(right).(uint)).(T)
	case uint32:
		return any(value + any(right).(uint32)).(T)
	case uint64:
		return any(value + any(right).(uint64)).(T)
	case float32:
		return any(value + any(right).(float32)).(T)
	case float64:
		return any(value + any(right).(float64)).(T)
	default:
		panic("tensor arithmetic requires numeric elements")
	}
}

func tensorMul[T any](left T, right T) T {
	switch value := any(left).(type) {
	case int:
		return any(value * any(right).(int)).(T)
	case int32:
		return any(value * any(right).(int32)).(T)
	case int64:
		return any(value * any(right).(int64)).(T)
	case uint:
		return any(value * any(right).(uint)).(T)
	case uint32:
		return any(value * any(right).(uint32)).(T)
	case uint64:
		return any(value * any(right).(uint64)).(T)
	case float32:
		return any(value * any(right).(float32)).(T)
	case float64:
		return any(value * any(right).(float64)).(T)
	default:
		panic("tensor arithmetic requires numeric elements")
	}
}

func tensorSub[T any](left T, right T) T {
	switch value := any(left).(type) {
	case int:
		return any(value - any(right).(int)).(T)
	case int32:
		return any(value - any(right).(int32)).(T)
	case int64:
		return any(value - any(right).(int64)).(T)
	case uint:
		return any(value - any(right).(uint)).(T)
	case uint32:
		return any(value - any(right).(uint32)).(T)
	case uint64:
		return any(value - any(right).(uint64)).(T)
	case float32:
		return any(value - any(right).(float32)).(T)
	case float64:
		return any(value - any(right).(float64)).(T)
	default:
		panic("tensor arithmetic requires numeric elements")
	}
}

func tensorShapeEqual(left []int, right []int) bool {
	if len(left) != len(right) {
		return false
	}
	for i, dim := range left {
		if dim != right[i] {
			return false
		}
	}
	return true
}

func tensorMean[T any](data []T) T {
	if len(data) == 0 {
		panic("tensor media requires non-empty data")
	}
	switch any(data[0]).(type) {
	case float32:
		var total float32
		for _, value := range data {
			total += any(value).(float32)
		}
		return any(total / float32(len(data))).(T)
	case float64:
		var total float64
		for _, value := range data {
			total += any(value).(float64)
		}
		return any(total / float64(len(data))).(T)
	default:
		panic("tensor media requires floating-point elements")
	}
}

func (t TensorTensor[T]) Summa() T {
	var total T
	for _, value := range t.data {
		total = tensorAdd(total, value)
	}
	return total
}

func (t TensorTensor[T]) Media() T { return tensorMean(t.data) }

func (a TensorTensor[T]) Addita(b TensorTensor[T]) TensorTensor[T] {
	if !tensorShapeEqual(a.shape, b.shape) {
		panic("tensor elementwise arithmetic requires equal shapes")
	}
	data := make([]T, len(a.data))
	for i := range data {
		data[i] = tensorAdd(a.data[i], b.data[i])
	}
	return TensorTensor[T]{data: data, shape: append([]int{}, a.shape...)}
}

func (a TensorTensor[T]) Subtrahe(b TensorTensor[T]) TensorTensor[T] {
	if !tensorShapeEqual(a.shape, b.shape) {
		panic("tensor elementwise arithmetic requires equal shapes")
	}
	data := make([]T, len(a.data))
	for i := range data {
		data[i] = tensorSub(a.data[i], b.data[i])
	}
	return TensorTensor[T]{data: data, shape: append([]int{}, a.shape...)}
}

func (a TensorTensor[T]) Multiplica(b TensorTensor[T]) TensorTensor[T] {
	if !tensorShapeEqual(a.shape, b.shape) {
		panic("tensor elementwise arithmetic requires equal shapes")
	}
	data := make([]T, len(a.data))
	for i := range data {
		data[i] = tensorMul(a.data[i], b.data[i])
	}
	return TensorTensor[T]{data: data, shape: append([]int{}, a.shape...)}
}

func (a TensorTensor[T]) Matmul(b TensorTensor[T]) TensorTensor[T] {
	if len(a.shape) != 2 || len(b.shape) != 2 || a.shape[1] != b.shape[0] {
		panic("tensor matmul requires compatible rank-2 shapes")
	}
	rows, inner, cols := a.shape[0], a.shape[1], b.shape[1]
	data := make([]T, rows*cols)
	for row := 0; row < rows; row++ {
		for col := 0; col < cols; col++ {
			var sum T
			for k := 0; k < inner; k++ {
				sum = tensorAdd(sum, tensorMul(a.data[row*inner+k], b.data[k*cols+col]))
			}
			data[row*cols+col] = sum
		}
	}
	return TensorTensor[T]{data: data, shape: []int{rows, cols}}
}

func (t TensorTensor[T]) Forma(shape []int) TensorTensor[T] {
	if TensorElementCount(shape) != len(t.data) {
		panic("tensor forma (reshape) element count mismatch")
	}
	return TensorTensor[T]{data: append([]T{}, t.data...), shape: append([]int{}, shape...)}
}

func (t TensorTensor[T]) Accipe(indices any) *T {
	offset := tensorOffset(t.shape, indices)
	if offset == nil || *offset < 0 || *offset >= len(t.data) {
		return nil
	}
	return &t.data[*offset]
}

// ReadAt is the bracket read: `T` or a trap (like a lista index); `Accipe` stays
// the optional `*T` method form.
func (t TensorTensor[T]) ReadAt(indices any) T {
	p := t.Accipe(indices)
	if p == nil {
		panic("tensor accipe invalid index")
	}
	return *p
}

func (t *TensorTensor[T]) Ponde(indices any, value T) {
	offset := tensorOffset(t.shape, indices)
	if offset == nil || *offset < 0 || *offset >= len(t.data) {
		panic("tensor ponde invalid index")
	}
	t.data[*offset] = value
}

func (t *TensorTensor[T]) Reple(value T) {
	for i := range t.data {
		t.data[i] = value
	}
}

func (t TensorTensor[T]) Sectio(start int, end int) TensorTensor[T] {
	if len(t.shape) == 0 || start < 0 || end < start || end > t.shape[0] {
		panic("tensor sectio invalid slice bounds")
	}
	inner := TensorElementCount(t.shape[1:])
	shape := append([]int{end - start}, t.shape[1:]...)
	return TensorTensor[T]{data: append([]T{}, t.data[start*inner:end*inner]...), shape: shape}
}
