package rt

import "time"

// InstansParse is the `time.Parse` three-fallback chain for the `instans`
// carrier (codegen-readability T1-G8).
//
// Semantics match the historical inline chain: `RFC3339Nano` first, then the
// nanosecond and second `-0700` layouts, returning the last parse error.
func InstansParse(raw string) (time.Time, error) {
	parsed, err := time.Parse(time.RFC3339Nano, raw)
	if err != nil {
		parsed, err = time.Parse("2006-01-02T15:04:05.999999999-0700", raw)
	}
	if err != nil {
		parsed, err = time.Parse("2006-01-02T15:04:05-0700", raw)
	}
	return parsed, err
}
