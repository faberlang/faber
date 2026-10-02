package rt

import "testing"

// TestInstansParseLayouts pins the three accepted layouts and the error for
// anything else.
func TestInstansParseLayouts(t *testing.T) {
	for _, raw := range []string{
		"2026-10-02T13:06:44Z",
		"2026-10-02T13:06:44.123456789Z",
		"2026-10-02T13:06:44.123456789-0400",
		"2026-10-02T13:06:44-0400",
	} {
		if _, err := InstansParse(raw); err != nil {
			t.Fatalf("InstansParse(%q): %v", raw, err)
		}
	}
	parsed, err := InstansParse("2026-10-02T13:06:44+0000")
	if err != nil || parsed.Year() != 2026 || parsed.Second() != 44 {
		t.Fatalf("second-layout parse = %v, %v", parsed, err)
	}
	if _, err := InstansParse("not a time"); err == nil {
		t.Fatalf("a malformed instans must return the last parse error")
	}
}
