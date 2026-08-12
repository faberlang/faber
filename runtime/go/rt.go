// Package rt is the Faber Go runtime display surface for generated programs.
//
// S5-U1 extraction (faber-target-runtime): the display helper family that the
// HIR-Go emitter previously hoisted into every generated file
// (faberValorDisplay, faberVerum, faberListDisplay, and the per-type display
// renderers) lives here as a native Go module. Generated programs import
// `faber/rt` and call the exported surface with `rt.` qualification.
//
// Package identity (inventory §7): `faber/runtime/go/` — Go module
// `faber/rt`, materialized offline by the Faber build tool (core-support
// root / module proxy optional). The module depends on Go stdlib only.
//
// Display semantics are frozen from the compiler emit surface: scalars render
// as their Faber surface (`verum`/`falsum`, fractus with a `.0` marker), byte
// buffers render as comma-separated numeric lists, collections render
// `[a, b, c]` / `{"k": v}` with sorted quoted keys, genus records render
// `{"field": value}` with Go-exported field names lowercased and quoted, and
// the tensor/vector carriers render their flat data list. `time.Time` renders
// as RFC3339.
package rt

import (
	"fmt"
	"reflect"
	"sort"
	"strconv"
	"strings"
	"time"
)

// Verum renders a Go bool as the Faber bivalens surface (`verum`/`falsum`).
func Verum(b bool) string {
	if b {
		return "verum"
	}
	return "falsum"
}

// ListDisplay renders a Faber list as `[a, b, c]` via the element renderer.
func ListDisplay[T any](values []T, render func(T) string) string {
	parts := make([]string, 0, len(values))
	for _, item := range values {
		parts = append(parts, render(item))
	}
	return "[" + strings.Join(parts, ", ") + "]"
}

// ValorDisplay renders a boxed Faber valor/json value with Faber display
// semantics. See the package doc for the per-type rules.
func ValorDisplay(value any) string {
	if value == nil {
		return "nihil"
	}
	switch v := value.(type) {
	case bool:
		if v {
			return "verum"
		}
		return "falsum"
	case string:
		return v
	case []byte:
		return valorByteListDisplay(v)
	case int:
		return strconv.Itoa(v)
	case int8:
		return strconv.FormatInt(int64(v), 10)
	case int16:
		return strconv.FormatInt(int64(v), 10)
	case int32:
		return strconv.FormatInt(int64(v), 10)
	case int64:
		return strconv.FormatInt(v, 10)
	case uint:
		return strconv.FormatUint(uint64(v), 10)
	case uint8:
		return strconv.FormatUint(uint64(v), 10)
	case uint16:
		return strconv.FormatUint(uint64(v), 10)
	case uint32:
		return strconv.FormatUint(uint64(v), 10)
	case uint64:
		return strconv.FormatUint(v, 10)
	case float32:
		return valorFractusDisplay(float64(v))
	case float64:
		return valorFractusDisplay(v)
	case time.Time:
		return v.UTC().Format(time.RFC3339)
	}
	rv := reflect.ValueOf(value)
	for rv.Kind() == reflect.Pointer {
		if rv.IsNil() {
			return "nihil"
		}
		rv = rv.Elem()
	}
	switch rv.Kind() {
	case reflect.Slice, reflect.Array:
		return valorSliceDisplay(rv)
	case reflect.Map:
		return valorMapDisplay(rv)
	case reflect.Struct:
		name := rv.Type().Name()
		if strings.HasPrefix(name, "faberTensor") || strings.HasPrefix(name, "faberVector") {
			for _, methodName := range []string{"Planata", "AdLista"} {
				method := rv.MethodByName(methodName)
				if !method.IsValid() {
					continue
				}
				results := method.Call(nil)
				if len(results) == 1 && results[0].Kind() == reflect.Slice {
					return valorSliceDisplay(results[0])
				}
			}
		}
		return valorStructDisplay(rv)
	}
	return fmt.Sprint(value)
}

// valorFractusDisplay renders a fractus with a `.0` marker when the integer
// format has no fractional/exponent separator.
func valorFractusDisplay(v float64) string {
	s := strconv.FormatFloat(v, 'f', -1, 64)
	if !strings.ContainsAny(s, ".eE") {
		return s + ".0"
	}
	return s
}

// valorByteListDisplay renders a byte buffer as a comma-separated numeric list.
func valorByteListDisplay(values []byte) string {
	parts := make([]string, len(values))
	for i, b := range values {
		parts[i] = strconv.Itoa(int(b))
	}
	return "[" + strings.Join(parts, ", ") + "]"
}

// valorSliceDisplay renders a reflected slice/array as `[a, b, c]`.
func valorSliceDisplay(rv reflect.Value) string {
	parts := make([]string, rv.Len())
	for i := 0; i < rv.Len(); i++ {
		parts[i] = ValorDisplay(rv.Index(i).Interface())
	}
	return "[" + strings.Join(parts, ", ") + "]"
}

// valorMapDisplay renders a reflected map as `{"k": v, ...}` with the keys
// sorted by their display form and string keys quoted.
func valorMapDisplay(rv reflect.Value) string {
	keys := rv.MapKeys()
	sort.Slice(keys, func(i, j int) bool {
		return ValorDisplay(keys[i].Interface()) < ValorDisplay(keys[j].Interface())
	})
	parts := make([]string, 0, len(keys))
	for _, key := range keys {
		keyStr := ValorDisplay(key.Interface())
		if key.Kind() == reflect.String {
			keyStr = strconv.Quote(key.String())
		}
		parts = append(parts, keyStr+": "+ValorDisplay(rv.MapIndex(key).Interface()))
	}
	return "{" + strings.Join(parts, ", ") + "}"
}

// valorStructDisplay renders a genus record as `{"field": value, ...}` with
// Go-exported field names lowercased and quoted.
func valorStructDisplay(rv reflect.Value) string {
	t := rv.Type()
	parts := make([]string, 0, rv.NumField())
	for i := 0; i < rv.NumField(); i++ {
		field := t.Field(i)
		name := field.Name
		if len(name) > 0 && name[0] >= 'A' && name[0] <= 'Z' {
			name = string(rune(name[0])+32) + name[1:]
		}
		parts = append(parts, strconv.Quote(name)+": "+ValorDisplay(rv.Field(i).Interface()))
	}
	return "{" + strings.Join(parts, ", ") + "}"
}
