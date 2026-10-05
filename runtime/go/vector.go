package rt

// The Faber `vector` carrier for generated Go programs (codegen-readability
// T1-G7): a fixed-width register-class value.
//
// A vector's width is a compile-time Faber fact; Go has no fixed-size generic
// slice, so the carrier stores a runtime slice and `VectorFromList` enforces
// the declared width at construction. The arithmetic kernels are generic over
// the `num` union constraint shared with the tensor carrier, so `l + r`
// compiles monomorphically; the `any`-typed entry points hoist one type switch
// per op, in the `tensorMean` shape. The carrier type and constructor are
// spelled by the compiler's canonical helper table (`rt.VectorVector`,
// `rt.VectorFromList`); the operations are methods carrying the Latin names
// the call sites use.
//
// No operation here is a store: arithmetic computes in the element type, and a
// width limit applies only where a result leaves into a bounded cell.

// VectorVector is the fixed-width vector carrier.
type VectorVector[T any] struct {
	data []T
}

// VectorFromList builds a vector from a list, trapping unless the list has the
// declared width.
func VectorFromList[T any](data []T, width int) VectorVector[T] {
	if len(data) != width {
		panic("vector width mismatch")
	}
	return VectorVector[T]{data: append([]T{}, data...)}
}

// AdLista is the exported flat-data accessor, the vector's twin of
// TensorTensor.Planata: it lets the dynamic DisplayValor renderer read the
// carrier through reflection without reaching into the unexported `data`
// field.
func (v VectorVector[T]) AdLista() []T { return append([]T{}, v.data...) }

func numDiv[T num](l T, r T) T { return l / r }

func zipNumDiv[T num](dst []T, a []T, b []T) {
	for i := range dst {
		dst[i] = numDiv(a[i], b[i])
	}
}

// vectorAddWise fills dst with a+b elementwise. The switch runs once per op,
// in the tensorMean shape, and matches exact slice types, exactly as the old
// per-element switches matched exact element types, so the traps and the
// non-numeric fall-through are unchanged. An empty list returns without
// touching the switch, as the old per-element loops never reached the switch.
func vectorAddWise[T any](dst []T, a []T, b []T) {
	if len(dst) == 0 {
		return
	}
	switch any(a).(type) {
	case []int:
		zipNumAdd(any(dst).([]int), any(a).([]int), any(b).([]int))
	case []int32:
		zipNumAdd(any(dst).([]int32), any(a).([]int32), any(b).([]int32))
	case []int64:
		zipNumAdd(any(dst).([]int64), any(a).([]int64), any(b).([]int64))
	case []uint:
		zipNumAdd(any(dst).([]uint), any(a).([]uint), any(b).([]uint))
	case []uint32:
		zipNumAdd(any(dst).([]uint32), any(a).([]uint32), any(b).([]uint32))
	case []uint64:
		zipNumAdd(any(dst).([]uint64), any(a).([]uint64), any(b).([]uint64))
	case []float32:
		zipNumAdd(any(dst).([]float32), any(a).([]float32), any(b).([]float32))
	case []float64:
		zipNumAdd(any(dst).([]float64), any(a).([]float64), any(b).([]float64))
	default:
		panic("vector arithmetic requires numeric elements")
	}
}

// vectorSubWise is vectorAddWise around numSub.
func vectorSubWise[T any](dst []T, a []T, b []T) {
	if len(dst) == 0 {
		return
	}
	switch any(a).(type) {
	case []int:
		zipNumSub(any(dst).([]int), any(a).([]int), any(b).([]int))
	case []int32:
		zipNumSub(any(dst).([]int32), any(a).([]int32), any(b).([]int32))
	case []int64:
		zipNumSub(any(dst).([]int64), any(a).([]int64), any(b).([]int64))
	case []uint:
		zipNumSub(any(dst).([]uint), any(a).([]uint), any(b).([]uint))
	case []uint32:
		zipNumSub(any(dst).([]uint32), any(a).([]uint32), any(b).([]uint32))
	case []uint64:
		zipNumSub(any(dst).([]uint64), any(a).([]uint64), any(b).([]uint64))
	case []float32:
		zipNumSub(any(dst).([]float32), any(a).([]float32), any(b).([]float32))
	case []float64:
		zipNumSub(any(dst).([]float64), any(a).([]float64), any(b).([]float64))
	default:
		panic("vector arithmetic requires numeric elements")
	}
}

// vectorMulWise is vectorAddWise around numMul.
func vectorMulWise[T any](dst []T, a []T, b []T) {
	if len(dst) == 0 {
		return
	}
	switch any(a).(type) {
	case []int:
		zipNumMul(any(dst).([]int), any(a).([]int), any(b).([]int))
	case []int32:
		zipNumMul(any(dst).([]int32), any(a).([]int32), any(b).([]int32))
	case []int64:
		zipNumMul(any(dst).([]int64), any(a).([]int64), any(b).([]int64))
	case []uint:
		zipNumMul(any(dst).([]uint), any(a).([]uint), any(b).([]uint))
	case []uint32:
		zipNumMul(any(dst).([]uint32), any(a).([]uint32), any(b).([]uint32))
	case []uint64:
		zipNumMul(any(dst).([]uint64), any(a).([]uint64), any(b).([]uint64))
	case []float32:
		zipNumMul(any(dst).([]float32), any(a).([]float32), any(b).([]float32))
	case []float64:
		zipNumMul(any(dst).([]float64), any(a).([]float64), any(b).([]float64))
	default:
		panic("vector arithmetic requires numeric elements")
	}
}

// vectorDivWise is vectorAddWise around numDiv.
func vectorDivWise[T any](dst []T, a []T, b []T) {
	if len(dst) == 0 {
		return
	}
	switch any(a).(type) {
	case []int:
		zipNumDiv(any(dst).([]int), any(a).([]int), any(b).([]int))
	case []int32:
		zipNumDiv(any(dst).([]int32), any(a).([]int32), any(b).([]int32))
	case []int64:
		zipNumDiv(any(dst).([]int64), any(a).([]int64), any(b).([]int64))
	case []uint:
		zipNumDiv(any(dst).([]uint), any(a).([]uint), any(b).([]uint))
	case []uint32:
		zipNumDiv(any(dst).([]uint32), any(a).([]uint32), any(b).([]uint32))
	case []uint64:
		zipNumDiv(any(dst).([]uint64), any(a).([]uint64), any(b).([]uint64))
	case []float32:
		zipNumDiv(any(dst).([]float32), any(a).([]float32), any(b).([]float32))
	case []float64:
		zipNumDiv(any(dst).([]float64), any(a).([]float64), any(b).([]float64))
	default:
		panic("vector arithmetic requires numeric elements")
	}
}

func (a VectorVector[T]) Addita(b VectorVector[T]) VectorVector[T] {
	if len(a.data) != len(b.data) {
		panic("vector elementwise arithmetic requires equal widths")
	}
	data := make([]T, len(a.data))
	vectorAddWise(data, a.data, b.data)
	return VectorVector[T]{data: data}
}

func (a VectorVector[T]) Subtrahe(b VectorVector[T]) VectorVector[T] {
	if len(a.data) != len(b.data) {
		panic("vector elementwise arithmetic requires equal widths")
	}
	data := make([]T, len(a.data))
	vectorSubWise(data, a.data, b.data)
	return VectorVector[T]{data: data}
}

func (a VectorVector[T]) Multiplica(b VectorVector[T]) VectorVector[T] {
	if len(a.data) != len(b.data) {
		panic("vector elementwise arithmetic requires equal widths")
	}
	data := make([]T, len(a.data))
	vectorMulWise(data, a.data, b.data)
	return VectorVector[T]{data: data}
}

func (a VectorVector[T]) Divida(b VectorVector[T]) VectorVector[T] {
	if len(a.data) != len(b.data) {
		panic("vector elementwise arithmetic requires equal widths")
	}
	data := make([]T, len(a.data))
	vectorDivWise(data, a.data, b.data)
	return VectorVector[T]{data: data}
}

func dotNum[N num](a []N, b []N) N {
	var total N
	for i := range a {
		total = numAdd(total, numMul(a[i], b[i]))
	}
	return total
}

// vectorDotWise folds the dot product left to right on the typed slices,
// switching on the element type once.
func vectorDotWise[T any](a []T, b []T) T {
	switch any(a).(type) {
	case []int:
		return any(dotNum(any(a).([]int), any(b).([]int))).(T)
	case []int32:
		return any(dotNum(any(a).([]int32), any(b).([]int32))).(T)
	case []int64:
		return any(dotNum(any(a).([]int64), any(b).([]int64))).(T)
	case []uint:
		return any(dotNum(any(a).([]uint), any(b).([]uint))).(T)
	case []uint32:
		return any(dotNum(any(a).([]uint32), any(b).([]uint32))).(T)
	case []uint64:
		return any(dotNum(any(a).([]uint64), any(b).([]uint64))).(T)
	case []float32:
		return any(dotNum(any(a).([]float32), any(b).([]float32))).(T)
	case []float64:
		return any(dotNum(any(a).([]float64), any(b).([]float64))).(T)
	default:
		panic("vector arithmetic requires numeric elements")
	}
}

func (a VectorVector[T]) Productum(b VectorVector[T]) T {
	if len(a.data) != len(b.data) {
		panic("vector dot product requires equal widths")
	}
	if len(a.data) == 0 {
		var zero T
		return zero
	}
	return vectorDotWise(a.data, b.data)
}

// vectorCrossWise computes the width-3 cross product on the typed slices, in
// the original component order, switching on the element type once.
func vectorCrossWise[T any](a []T, b []T) VectorVector[T] {
	switch x := any(a).(type) {
	case []int:
		y := any(b).([]int)
		return VectorVector[T]{data: any([]int{
			numSub(numMul(x[1], y[2]), numMul(x[2], y[1])),
			numSub(numMul(x[2], y[0]), numMul(x[0], y[2])),
			numSub(numMul(x[0], y[1]), numMul(x[1], y[0])),
		}).([]T)}
	case []int32:
		y := any(b).([]int32)
		return VectorVector[T]{data: any([]int32{
			numSub(numMul(x[1], y[2]), numMul(x[2], y[1])),
			numSub(numMul(x[2], y[0]), numMul(x[0], y[2])),
			numSub(numMul(x[0], y[1]), numMul(x[1], y[0])),
		}).([]T)}
	case []int64:
		y := any(b).([]int64)
		return VectorVector[T]{data: any([]int64{
			numSub(numMul(x[1], y[2]), numMul(x[2], y[1])),
			numSub(numMul(x[2], y[0]), numMul(x[0], y[2])),
			numSub(numMul(x[0], y[1]), numMul(x[1], y[0])),
		}).([]T)}
	case []uint:
		y := any(b).([]uint)
		return VectorVector[T]{data: any([]uint{
			numSub(numMul(x[1], y[2]), numMul(x[2], y[1])),
			numSub(numMul(x[2], y[0]), numMul(x[0], y[2])),
			numSub(numMul(x[0], y[1]), numMul(x[1], y[0])),
		}).([]T)}
	case []uint32:
		y := any(b).([]uint32)
		return VectorVector[T]{data: any([]uint32{
			numSub(numMul(x[1], y[2]), numMul(x[2], y[1])),
			numSub(numMul(x[2], y[0]), numMul(x[0], y[2])),
			numSub(numMul(x[0], y[1]), numMul(x[1], y[0])),
		}).([]T)}
	case []uint64:
		y := any(b).([]uint64)
		return VectorVector[T]{data: any([]uint64{
			numSub(numMul(x[1], y[2]), numMul(x[2], y[1])),
			numSub(numMul(x[2], y[0]), numMul(x[0], y[2])),
			numSub(numMul(x[0], y[1]), numMul(x[1], y[0])),
		}).([]T)}
	case []float32:
		y := any(b).([]float32)
		return VectorVector[T]{data: any([]float32{
			numSub(numMul(x[1], y[2]), numMul(x[2], y[1])),
			numSub(numMul(x[2], y[0]), numMul(x[0], y[2])),
			numSub(numMul(x[0], y[1]), numMul(x[1], y[0])),
		}).([]T)}
	case []float64:
		y := any(b).([]float64)
		return VectorVector[T]{data: any([]float64{
			numSub(numMul(x[1], y[2]), numMul(x[2], y[1])),
			numSub(numMul(x[2], y[0]), numMul(x[0], y[2])),
			numSub(numMul(x[0], y[1]), numMul(x[1], y[0])),
		}).([]T)}
	default:
		panic("vector arithmetic requires numeric elements")
	}
}

func (a VectorVector[T]) Transversum(b VectorVector[T]) VectorVector[T] {
	if len(a.data) != 3 || len(b.data) != 3 {
		panic("vector cross product requires width 3")
	}
	return vectorCrossWise(a.data, b.data)
}

func (v VectorVector[T]) Swizzle(mask string) VectorVector[T] {
	out := make([]T, len(mask))
	for i, ch := range mask {
		var idx int
		switch ch {
		case 'x':
			idx = 0
		case 'y':
			idx = 1
		case 'z':
			idx = 2
		case 'w':
			idx = 3
		default:
			panic("vector swizzle mask out of range")
		}
		if idx >= len(v.data) {
			panic("vector swizzle mask out of range")
		}
		out[i] = v.data[idx]
	}
	return VectorVector[T]{data: out}
}
