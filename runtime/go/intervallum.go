package rt

// The Faber numeric `intervallum` carrier for generated Go programs
// (codegen-readability T1-G8): two numeric bounds plus an inclusivity tag.
//
// Generic methods rely on Go's type-set arithmetic, which is valid for the
// closed union of Faber's numeric bound types. The carrier type and the
// constructor are spelled by the compiler's canonical helper table
// (`rt.IntervallumIntervallum`, `rt.IntervallumValue`); the operations are
// methods carrying the Latin names the call sites use.
//
// No operation here is a store: arithmetic computes in the bound type, and a
// width limit applies only where a result leaves into a bounded cell.

type intervallumNum interface {
	~int | ~int32 | ~int64 | ~uint | ~uint32 | ~uint64 | ~float32 | ~float64
}

// IntervallumIntervallum is the numeric interval carrier.
type IntervallumIntervallum[T intervallumNum] struct {
	initium T
	finis   T
	kind    int
}

// IntervallumValue builds an interval from its bounds and inclusivity tag
// (0 exclusive, 1 inclusive of the end bound).
func IntervallumValue[T intervallumNum](initium T, finis T, kind int) IntervallumIntervallum[T] {
	return IntervallumIntervallum[T]{initium: initium, finis: finis, kind: kind}
}

func intervallumMin[T intervallumNum](a T, b T) T {
	if a <= b {
		return a
	}
	return b
}

func intervallumMax[T intervallumNum](a T, b T) T {
	if a >= b {
		return a
	}
	return b
}

func (i IntervallumIntervallum[T]) Continet(value T) bool {
	ascending := i.initium <= i.finis
	if i.kind == 1 {
		if ascending {
			return value >= i.initium && value <= i.finis
		}
		return value <= i.initium && value >= i.finis
	}
	if ascending {
		return value >= i.initium && value < i.finis
	}
	return value <= i.initium && value > i.finis
}

func (i IntervallumIntervallum[T]) Longitudo() int {
	var span T
	if i.initium <= i.finis {
		span = i.finis - i.initium
	} else {
		span = i.initium - i.finis
	}
	if i.kind == 1 {
		return int(span) + 1
	}
	return int(span)
}

func (i IntervallumIntervallum[T]) Coercere(value T) T {
	if i.Continet(value) {
		return value
	}
	lo := intervallumMin(i.initium, i.finis)
	hi := intervallumMax(i.initium, i.finis)
	loValid := lo
	hiValid := hi
	if i.kind == 0 && i.initium > i.finis {
		loValid = lo + 1
	}
	if i.kind == 0 && i.initium <= i.finis {
		hiValid = hi - 1
	}
	if value < loValid {
		return loValid
	}
	if value > hiValid {
		return hiValid
	}
	return value
}

func (i IntervallumIntervallum[T]) CoercereIntervallum(target IntervallumIntervallum[T]) IntervallumIntervallum[T] {
	return IntervallumIntervallum[T]{initium: target.Coercere(i.initium), finis: target.Coercere(i.finis), kind: target.kind}
}

func (i IntervallumIntervallum[T]) AdLista() []T {
	count := i.Longitudo()
	if count <= 0 {
		return []T{}
	}
	out := make([]T, 0, count)
	cursor := i.initium
	for n := 0; n < count; n++ {
		out = append(out, cursor)
		if i.initium <= i.finis {
			cursor = cursor + 1
		} else {
			cursor = cursor - 1
		}
	}
	return out
}

func (a IntervallumIntervallum[T]) Inter(b IntervallumIntervallum[T]) *IntervallumIntervallum[T] {
	lo := intervallumMax(intervallumMin(a.initium, a.finis), intervallumMin(b.initium, b.finis))
	hi := intervallumMin(intervallumMax(a.initium, a.finis), intervallumMax(b.initium, b.finis))
	if lo > hi {
		return nil
	}
	newLo := lo
	for {
		if a.Continet(newLo) && b.Continet(newLo) {
			break
		}
		if newLo >= hi {
			return nil
		}
		newLo = newLo + 1
	}
	newHi := hi
	for {
		if a.Continet(newHi) && b.Continet(newHi) {
			break
		}
		if newHi <= newLo {
			return nil
		}
		newHi = newHi - 1
	}
	if a.initium > a.finis {
		return &IntervallumIntervallum[T]{initium: newHi, finis: newLo, kind: 1}
	}
	return &IntervallumIntervallum[T]{initium: newLo, finis: newHi, kind: 1}
}

func (a IntervallumIntervallum[T]) Union(b IntervallumIntervallum[T]) *IntervallumIntervallum[T] {
	if a.Inter(b) == nil && !a.touches(b) {
		return nil
	}
	lo := intervallumMin(intervallumMin(a.initium, a.finis), intervallumMin(b.initium, b.finis))
	hi := intervallumMax(intervallumMax(a.initium, a.finis), intervallumMax(b.initium, b.finis))
	newLo := lo
	for {
		if a.Continet(newLo) || b.Continet(newLo) {
			break
		}
		newLo = newLo + 1
	}
	newHi := hi
	for {
		if a.Continet(newHi) || b.Continet(newHi) {
			break
		}
		newHi = newHi - 1
	}
	if a.initium > a.finis {
		return &IntervallumIntervallum[T]{initium: newHi, finis: newLo, kind: 1}
	}
	return &IntervallumIntervallum[T]{initium: newLo, finis: newHi, kind: 1}
}

func (a IntervallumIntervallum[T]) touches(b IntervallumIntervallum[T]) bool {
	if a.finis == b.initium && (a.Continet(b.initium) || b.Continet(a.finis)) {
		return true
	}
	if b.finis == a.initium && (b.Continet(a.initium) || a.Continet(b.finis)) {
		return true
	}
	loA := intervallumMin(a.initium, a.finis)
	hiA := intervallumMax(a.initium, a.finis)
	loB := intervallumMin(b.initium, b.finis)
	hiB := intervallumMax(b.initium, b.finis)
	actualHiA := hiA
	if !a.Continet(hiA) {
		actualHiA = hiA - 1
	}
	actualLoB := loB
	if !b.Continet(loB) {
		actualLoB = loB + 1
	}
	if actualHiA+1 == actualLoB {
		return true
	}
	actualHiB := hiB
	if !b.Continet(hiB) {
		actualHiB = hiB - 1
	}
	actualLoA := loA
	if !a.Continet(loA) {
		actualLoA = loA + 1
	}
	if actualHiB+1 == actualLoA {
		return true
	}
	return false
}
