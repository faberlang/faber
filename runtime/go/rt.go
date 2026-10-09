// Package rt is the Faber Go runtime display surface for generated programs.
//
// The display helper family (DisplayValor, DisplayVerum, DisplayList,
// MapDisplay, and the per-type display renderers) lives here as a native Go
// module. Generated programs import `faber/rt` and call the exported surface
// with `rt.` qualification; an exported name is `<Namespace><Name>`.
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
// as RFC3339. A `*big.Int` (the unbounded `inf` carrier) renders its decimal
// digits.
package rt

import (
	"fmt"
	"math/big"
	"reflect"
	"sort"
	"strconv"
	"strings"
	"time"
)

// DisplayTokens are the words a program prints for a bivalens and for the null
// value. The print locale is the code locale: Latin code prints
// verum/falsum/nihil and English code prints true/false/none.
type DisplayTokens struct {
	True  string
	False string
	None  string
}

// LatinTokens are the Latin display tokens; DisplayVerum and DisplayValor use
// them.
var LatinTokens = DisplayTokens{True: "verum", False: "falsum", None: "nihil"}

// DisplayVerum renders a Go bool as the Faber bivalens surface
// (`verum`/`falsum`).
func DisplayVerum(b bool) string {
	return DisplayVerumTokens(b, LatinTokens)
}

// DisplayVerumTokens renders a Go bool with the module's display tokens.
func DisplayVerumTokens(b bool, tokens DisplayTokens) string {
	if b {
		return tokens.True
	}
	return tokens.False
}

// DisplayList renders a Faber list as `[a, b, c]` via the element renderer.
func DisplayList[T any](values []T, render func(T) string) string {
	parts := make([]string, 0, len(values))
	for _, item := range values {
		parts = append(parts, render(item))
	}
	return "[" + strings.Join(parts, ", ") + "]"
}

// MapDisplay renders a Faber map as `{"k": v, ...}` via the key and value
// renderers. Entry order is unspecified by the language; entries are emitted
// in the order of their rendered keys so a print is repeatable.
func MapDisplay[K comparable, V any](entries map[K]V, renderKey func(K) string, renderValue func(V) string) string {
	parts := make([]string, 0, len(entries))
	for key, value := range entries {
		parts = append(parts, renderKey(key)+": "+renderValue(value))
	}
	sort.Strings(parts)
	return "{" + strings.Join(parts, ", ") + "}"
}

// DisplayValor renders a boxed Faber valor/json value with Faber display
// semantics. See the package doc for the per-type rules.
func DisplayValor(value any) string {
	return DisplayValorTokens(value, LatinTokens)
}

// DisplayValorTokens renders a boxed value with the module's display tokens.
func DisplayValorTokens(value any, tokens DisplayTokens) string {
	if value == nil {
		return tokens.None
	}
	switch v := value.(type) {
	case bool:
		return DisplayVerumTokens(v, tokens)
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
	case *big.Int:
		// An unbounded `inf` carrier: its decimal digits (a nil pointer is the
		// unset slot, which reads as zero).
		if v == nil {
			return "0"
		}
		return v.String()
	}
	rv := reflect.ValueOf(value)
	for rv.Kind() == reflect.Pointer {
		if rv.IsNil() {
			return tokens.None
		}
		rv = rv.Elem()
	}
	switch rv.Kind() {
	case reflect.Slice, reflect.Array:
		return valorSliceDisplay(rv, tokens)
	case reflect.Map:
		return valorMapDisplay(rv, tokens)
	case reflect.Struct:
		name := rv.Type().Name()
		if rv.Type().PkgPath() == "faber/rt" && (strings.HasPrefix(name, "TensorTensor") || strings.HasPrefix(name, "VectorVector")) {
			for _, methodName := range []string{"Planata", "AdLista"} {
				method := rv.MethodByName(methodName)
				if !method.IsValid() {
					continue
				}
				results := method.Call(nil)
				if len(results) == 1 && results[0].Kind() == reflect.Slice {
					return valorSliceDisplay(results[0], tokens)
				}
			}
		}
		return valorStructDisplay(rv, tokens)
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
func valorSliceDisplay(rv reflect.Value, tokens DisplayTokens) string {
	parts := make([]string, rv.Len())
	for i := 0; i < rv.Len(); i++ {
		parts[i] = DisplayValorTokens(rv.Index(i).Interface(), tokens)
	}
	return "[" + strings.Join(parts, ", ") + "]"
}

// valorMapDisplay renders a reflected map as `{"k": v, ...}` with the keys
// sorted by their display form and string keys quoted.
func valorMapDisplay(rv reflect.Value, tokens DisplayTokens) string {
	keys := rv.MapKeys()
	sort.Slice(keys, func(i, j int) bool {
		return DisplayValorTokens(keys[i].Interface(), tokens) < DisplayValorTokens(keys[j].Interface(), tokens)
	})
	parts := make([]string, 0, len(keys))
	for _, key := range keys {
		keyStr := DisplayValorTokens(key.Interface(), tokens)
		if key.Kind() == reflect.String {
			keyStr = strconv.Quote(key.String())
		}
		parts = append(parts, keyStr+": "+DisplayValorTokens(rv.MapIndex(key).Interface(), tokens))
	}
	return "{" + strings.Join(parts, ", ") + "}"
}

// valorStructDisplay renders a genus record as `{"field": value, ...}` with
// Go-exported field names lowercased and quoted.
func valorStructDisplay(rv reflect.Value, tokens DisplayTokens) string {
	t := rv.Type()
	parts := make([]string, 0, rv.NumField())
	for i := 0; i < rv.NumField(); i++ {
		field := t.Field(i)
		name := field.Name
		if len(name) > 0 && name[0] >= 'A' && name[0] <= 'Z' {
			name = string(rune(name[0])+32) + name[1:]
		}
		parts = append(parts, strconv.Quote(name)+": "+DisplayValorTokens(rv.Field(i).Interface(), tokens))
	}
	return "{" + strings.Join(parts, ", ") + "}"
}
