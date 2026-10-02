package rt

// The Faber `vector` carrier for generated Go programs (codegen-readability
// T1-G7): a fixed-width register-class value.
//
// A vector's width is a compile-time Faber fact; Go has no fixed-size generic
// slice, so the carrier stores a runtime slice and `VectorFromList` enforces
// the declared width at construction. The arithmetic helpers dispatch on the
// element type, as the tensor helpers do, because Go generics cannot bound `T`
// to the numeric operators across the full primitive set a vector carries.
// The carrier type and constructor are spelled by the compiler's canonical
// helper table (`rt.VectorVector`, `rt.VectorFromList`); the operations are
// methods carrying the Latin names the call sites use.
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

func vectorAdd[T any](left T, right T) T {
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
		panic("vector arithmetic requires numeric elements")
	}
}

func vectorSub[T any](left T, right T) T {
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
		panic("vector arithmetic requires numeric elements")
	}
}

func vectorMul[T any](left T, right T) T {
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
		panic("vector arithmetic requires numeric elements")
	}
}

func vectorDiv[T any](left T, right T) T {
	switch value := any(left).(type) {
	case int:
		return any(value / any(right).(int)).(T)
	case int32:
		return any(value / any(right).(int32)).(T)
	case int64:
		return any(value / any(right).(int64)).(T)
	case uint:
		return any(value / any(right).(uint)).(T)
	case uint32:
		return any(value / any(right).(uint32)).(T)
	case uint64:
		return any(value / any(right).(uint64)).(T)
	case float32:
		return any(value / any(right).(float32)).(T)
	case float64:
		return any(value / any(right).(float64)).(T)
	default:
		panic("vector arithmetic requires numeric elements")
	}
}

func (a VectorVector[T]) Addita(b VectorVector[T]) VectorVector[T] {
	if len(a.data) != len(b.data) {
		panic("vector elementwise arithmetic requires equal widths")
	}
	data := make([]T, len(a.data))
	for i := range data {
		data[i] = vectorAdd(a.data[i], b.data[i])
	}
	return VectorVector[T]{data: data}
}

func (a VectorVector[T]) Subtrahe(b VectorVector[T]) VectorVector[T] {
	if len(a.data) != len(b.data) {
		panic("vector elementwise arithmetic requires equal widths")
	}
	data := make([]T, len(a.data))
	for i := range data {
		data[i] = vectorSub(a.data[i], b.data[i])
	}
	return VectorVector[T]{data: data}
}

func (a VectorVector[T]) Multiplica(b VectorVector[T]) VectorVector[T] {
	if len(a.data) != len(b.data) {
		panic("vector elementwise arithmetic requires equal widths")
	}
	data := make([]T, len(a.data))
	for i := range data {
		data[i] = vectorMul(a.data[i], b.data[i])
	}
	return VectorVector[T]{data: data}
}

func (a VectorVector[T]) Divida(b VectorVector[T]) VectorVector[T] {
	if len(a.data) != len(b.data) {
		panic("vector elementwise arithmetic requires equal widths")
	}
	data := make([]T, len(a.data))
	for i := range data {
		data[i] = vectorDiv(a.data[i], b.data[i])
	}
	return VectorVector[T]{data: data}
}

func (a VectorVector[T]) Productum(b VectorVector[T]) T {
	if len(a.data) != len(b.data) {
		panic("vector dot product requires equal widths")
	}
	var total T
	for i := range a.data {
		total = vectorAdd(total, vectorMul(a.data[i], b.data[i]))
	}
	return total
}

func (a VectorVector[T]) Transversum(b VectorVector[T]) VectorVector[T] {
	if len(a.data) != 3 || len(b.data) != 3 {
		panic("vector cross product requires width 3")
	}
	return VectorVector[T]{data: []T{
		vectorSub(vectorMul(a.data[1], b.data[2]), vectorMul(a.data[2], b.data[1])),
		vectorSub(vectorMul(a.data[2], b.data[0]), vectorMul(a.data[0], b.data[2])),
		vectorSub(vectorMul(a.data[0], b.data[1]), vectorMul(a.data[1], b.data[0])),
	}}
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
