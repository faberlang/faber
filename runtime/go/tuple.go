package rt

import (
	"math/big"
	"reflect"
)

// Composite map keys for generated Go programs (codegen-readability T1-G8,
// D3.2): Go tuples ride `[]any`, which is not comparable, so a tuple-keyed map
// stores the parts as a `[N]any` array (comparable when every part is) and
// hands them back as `[]any`.
//
// An `inf` part is a `*big.Int`, which Go compares by pointer, so two equal
// values with distinct allocations would be distinct keys. Inside a composite
// key it rides its canonical decimal string (`InfPart`, the runner's
// structural keying) and `TupleParts` turns it back into a `*big.Int`.

// TupleKey turns a tuple carrier (`[]any`) into a comparable map key.
func TupleKey(tuple any) any {
	parts := tuple.([]any)
	key := reflect.New(reflect.ArrayOf(len(parts), reflect.TypeOf((*any)(nil)).Elem())).Elem()
	for i, part := range parts {
		if n, ok := part.(*big.Int); ok {
			part = InfPart(InfKey(n))
		}
		key.Index(i).Set(reflect.ValueOf(&part).Elem())
	}
	return key.Interface()
}

// TupleParts turns a composite map key back into its tuple carrier.
func TupleParts(key any) []any {
	value := reflect.ValueOf(key)
	parts := make([]any, value.Len())
	for i := range parts {
		parts[i] = value.Index(i).Interface()
		if k, ok := parts[i].(InfPart); ok {
			parts[i] = InfFromKey(string(k))
		}
	}
	return parts
}
