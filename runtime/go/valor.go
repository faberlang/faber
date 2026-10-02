package rt

// ValorStruct is the valor-to-struct decoder shell (codegen-readability
// T1-G8); field extraction is supplied by the per-site decode callback.
func ValorStruct[T any](src any, decode func(map[string]any) T) (T, bool) {
	if v, ok := src.(T); ok {
		return v, true
	}
	m, ok := src.(map[string]any)
	if !ok {
		var zero T
		return zero, false
	}
	return decode(m), true
}
