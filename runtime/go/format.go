package rt

// The `¶` format-operator renderers for generated Go programs
// (codegen-readability T1-G5). They reproduce the reference renderer in
// `radix_types::format_spec`: integer precision pads, radix kinds print sign
// plus magnitude, non-finite floats print `NaN`/`∞`/`-∞`, scientific uses the
// `1.5e3` shape, and the default float body is the Faber `fractus` display. An
// `inf` renders through `FormatMagnus` over a `*big.Int` of any size. Every
// exported name is `Format<Name>`, spelled by the compiler's canonical helper
// table.

import (
	"math"
	"math/big"
	"strconv"
	"strings"
	"unicode/utf8"
)

// FormatSpec is a decomposed `¶` layout spec: the fill rune, the alignment
// (`<`, `>` or `^`), the `+` sign flag, the zero-pad flag, the minimum width,
// the precision (-1 when absent) and the kind (`x`, `b`, `o`, `e` or a space
// for the default).
type FormatSpec struct {
	Fill      rune
	Align     rune
	Plus      bool
	Zero      bool
	Width     int
	Precision int
	Kind      rune
}

func FormatPad(sign string, body string, spec FormatSpec, zero bool) string {
	n := utf8.RuneCountInString(sign) + utf8.RuneCountInString(body)
	if n >= spec.Width {
		return sign + body
	}
	pad := spec.Width - n
	if zero {
		return sign + strings.Repeat("0", pad) + body
	}
	fill := string(spec.Fill)
	switch spec.Align {
	case '<':
		return sign + body + strings.Repeat(fill, pad)
	case '^':
		return strings.Repeat(fill, pad/2) + sign + body + strings.Repeat(fill, pad-pad/2)
	default:
		return strings.Repeat(fill, pad) + sign + body
	}
}

func FormatSci(mag float64, precision int) string {
	mant, exp, _ := strings.Cut(strconv.FormatFloat(mag, 'e', precision, 64), "e")
	e, _ := strconv.Atoi(exp)
	return mant + "e" + strconv.Itoa(e)
}

func FormatInt(neg bool, mag uint64, spec FormatSpec) string {
	var body string
	switch spec.Kind {
	case 'x':
		body = strconv.FormatUint(mag, 16)
	case 'b':
		body = strconv.FormatUint(mag, 2)
	case 'o':
		body = strconv.FormatUint(mag, 8)
	case 'e':
		body = FormatSci(float64(mag), spec.Precision)
	default:
		body = strconv.FormatUint(mag, 10)
		if spec.Precision > 0 {
			body += "." + strings.Repeat("0", spec.Precision)
		}
	}
	sign := ""
	if neg {
		sign = "-"
	} else if spec.Plus {
		sign = "+"
	}
	return FormatPad(sign, body, spec, spec.Zero)
}

func FormatSigned(v int64, spec FormatSpec) string {
	if v < 0 {
		return FormatInt(true, uint64(-(v+1))+1, spec)
	}
	return FormatInt(false, uint64(v), spec)
}

func FormatFloat(v float64, bits int, spec FormatSpec) string {
	if math.IsNaN(v) {
		return FormatPad("", "NaN", spec, false)
	}
	sign := ""
	if math.Signbit(v) {
		sign = "-"
	} else if spec.Plus {
		sign = "+"
	}
	mag := math.Abs(v)
	if math.IsInf(mag, 1) {
		return FormatPad(sign, "∞", spec, false)
	}
	var body string
	if spec.Kind == 'e' {
		body = FormatSci(mag, spec.Precision)
	} else if spec.Precision >= 0 {
		body = strconv.FormatFloat(mag, 'f', spec.Precision, 64)
	} else {
		body = strconv.FormatFloat(mag, 'f', -1, bits)
		if !strings.ContainsAny(body, ".eE") {
			if bits == 64 {
				// An integral float64 prints its exact expansion, like the
				// runner's `display_fractus` ({:.1}); only float32 keeps its
				// shortest digits plus ".0".
				body = strconv.FormatFloat(mag, 'f', 1, 64)
			} else {
				body += ".0"
			}
		}
	}
	return FormatPad(sign, body, spec, spec.Zero)
}

func FormatText(v string, spec FormatSpec) string {
	if spec.Precision >= 0 {
		if r := []rune(v); len(r) > spec.Precision {
			v = string(r[:spec.Precision])
		}
	}
	return FormatPad("", v, spec, false)
}

func FormatInstans(iso string, preset rune) string {
	date, clock, _ := strings.Cut(iso, "T")
	switch preset {
	case 'd':
		return date
	case 't':
		if r := []rune(clock); len(r) > 8 {
			return string(r[:8])
		}
		return clock
	}
	return iso
}

// FormatMagnus is `¶` over an `inf` (ruling 42): the integer layout rules over
// a `*big.Int` of any size, mirroring `radix_types::format_spec::render_magnus`.
// A nil pointer reads as zero.
func FormatMagnus(v *big.Int, spec FormatSpec) string {
	v = InfZ(v)
	mag := new(big.Int).Abs(v)
	var body string
	switch spec.Kind {
	case 'x':
		body = mag.Text(16)
	case 'b':
		body = mag.Text(2)
	case 'o':
		body = mag.Text(8)
	case 'e':
		body = FormatMagnusSci(mag, spec.Precision)
	default:
		body = mag.String()
		if spec.Precision > 0 {
			body += "." + strings.Repeat("0", spec.Precision)
		}
	}
	sign := ""
	if v.Sign() < 0 {
		sign = "-"
	} else if spec.Plus {
		sign = "+"
	}
	return FormatPad(sign, body, spec, spec.Zero)
}

func FormatMagnusSci(mag *big.Int, precision int) string {
	f, _ := new(big.Float).SetInt(mag).Float64()
	if !math.IsInf(f, 0) {
		return FormatSci(f, precision)
	}
	digits := mag.String()
	exponent := len(digits) - 1
	var coefficient string
	if precision >= 0 {
		take := precision + 1
		if take > len(digits) {
			take = len(digits)
		}
		coefficient = digits[:take] + strings.Repeat("0", precision+1-take)
		if precision > 0 {
			coefficient = coefficient[:1] + "." + coefficient[1:]
		}
	} else {
		trimmed := strings.TrimRight(digits, "0")
		if trimmed == "" {
			trimmed = "0"
		}
		coefficient = trimmed
		if len(trimmed) > 1 {
			coefficient = trimmed[:1] + "." + trimmed[1:]
		}
	}
	return coefficient + "e" + strconv.Itoa(exponent)
}
