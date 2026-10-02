package rt

import (
	"math"
	"math/big"
	"reflect"
)

// The ordering-contract helpers for generated Go programs
// (codegen-readability T1-G8, D1.4).
//
// `OrderOrder` calls a genus's own `Compare` (pointer receiver) and falls back
// to `OrderCompare` for built-ins. `OrderCompare` orders integers by value,
// floats by IEEE 754 totalOrder (operator default F2), strings by byte order
// (UTF-8 byte order is code-point order, D1.6), tuples lexicographically, an
// `inf` (`*big.Int`) by value, and any other value through its `Compare`
// method.

// OrderOrder is the generic ordering-contract entry for a bounded `T`.
func OrderOrder[T any](a T, b T) int {
	if o, ok := any(&a).(interface{ Compare(T) int }); ok {
		return o.Compare(b)
	}
	return OrderCompare(any(a), any(b))
}

func compareFloat(a float64, b float64) int {
	x := int64(math.Float64bits(a))
	y := int64(math.Float64bits(b))
	x ^= int64(uint64(x>>63) >> 1)
	y ^= int64(uint64(y>>63) >> 1)
	if x < y {
		return -1
	}
	if x > y {
		return 1
	}
	return 0
}

// OrderCompare is the built-in and tuple three-way compare.
func OrderCompare(a any, b any) int {
	switch x := a.(type) {
	case float64:
		return compareFloat(x, b.(float64))
	case float32:
		return compareFloat(float64(x), float64(b.(float32)))
	case *big.Int:
		return InfCmp(x, b.(*big.Int))
	case []any:
		y := b.([]any)
		for i := range x {
			if c := OrderCompare(x[i], y[i]); c != 0 {
				return c
			}
		}
		return 0
	}
	va, vb := reflect.ValueOf(a), reflect.ValueOf(b)
	switch va.Kind() {
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		x, y := va.Int(), vb.Int()
		if x < y {
			return -1
		}
		if x > y {
			return 1
		}
		return 0
	case reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64:
		x, y := va.Uint(), vb.Uint()
		if x < y {
			return -1
		}
		if x > y {
			return 1
		}
		return 0
	case reflect.String:
		x, y := va.String(), vb.String()
		if x < y {
			return -1
		}
		if x > y {
			return 1
		}
		return 0
	}
	pa := reflect.New(va.Type())
	pa.Elem().Set(va)
	return int(pa.MethodByName("Compare").Call([]reflect.Value{vb})[0].Int())
}
