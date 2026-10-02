package rt

import (
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"
)

func mustRegex(t *testing.T, pattern string) RegexRegex {
	t.Helper()
	r, err := RegexNew(pattern)
	if err != nil {
		t.Fatalf("RegexNew(%q) = %v, want ok", pattern, err)
	}
	return r
}

func rejectID(err error) string {
	var rejected *regexReject
	if errors.As(err, &rejected) {
		return rejected.id
	}
	return "accepted"
}

func spans(r RegexRegex, text string) string {
	var out []string
	for _, m := range r.Collecta(text) {
		out = append(out, fmt.Sprintf("(%d,%d)", m.Principium(), m.Terminus()))
	}
	return strings.Join(out, " ")
}

// TestRegexRejectIds pins the stable construct ids of a rejected pattern (RD-4).
func TestRegexRejectIds(t *testing.T) {
	cases := map[string]string{
		"a(?=b)":           "lookahead",
		"a(?!b)":           "lookahead",
		"(?<=a)b":          "lookbehind",
		"(?<!a)b":          "lookbehind",
		"(a)\\1":           "backreference",
		"(?P<n>a)\\k<n>":   "backreference",
		"(?P<n>a)(?P=n)":   "backreference",
		"\\0":              "backreference",
		"\\012":            "backreference",
		"(?g)a":            "unsupported_flag",
		"(?y)a":            "unsupported_flag",
		"(?U)a+":           "unsupported_flag",
		"(?u)a":            "unsupported_flag",
		"(?-u)a":           "unsupported_flag",
		"(?>a)":            "atomic_group",
		"a++":              "possessive",
		"a*+":              "possessive",
		"(?R)":             "recursion",
		"(a)(?1)":          "recursion",
		"(?(1)a|b)":        "conditional",
		"(":                "syntax",
		"a)":               "syntax",
		"[":                "syntax",
		"a{2,1}":           "syntax",
		"*a":               "syntax",
		"a**":              "syntax",
		"\\":               "syntax",
		"\\Z":              "syntax",
		"\\Qa.b\\E":        "syntax",
		"\\p{Foo}":         "syntax",
		"a{100000}":        "syntax",
		"[]":               "syntax",
		"[z-a]":            "syntax",
		"\\u0041":          "syntax",
		"\\u{41}":          "syntax",
		"[a-z&&[^m]]":      "syntax",
		"[a[bc]]":          "syntax",
		"[a-z--m]":         "syntax",
		"\\<":              "syntax",
		"\\b{start}":       "syntax",
		"(?P<>a)":          "syntax",
		"(?#c)a":           "syntax",
		"(?'n'a)":          "syntax",
		"(?P<n>a)(?P<n>b)": "syntax",
		"(?P<1a>a)":        "syntax",
		"[[:^alpha:]]":     "accepted",
		"\\x{1F600}":       "accepted",
		"\\x41":            "accepted",
		"a+?":              "accepted",
		"a{2,3}":           "accepted",
		"a{2,}":            "accepted",
		"()":               "accepted",
		"(|a)":             "accepted",
		"#":                "accepted",
		"$":                "accepted",
		"(?i)a":            "accepted",
		"(?ims)a":          "accepted",
		"(?x:a b)":         "accepted",
		"(?P<é>a)":         "accepted",
		"(?<n>a)b":         "accepted",
		"":                 "accepted",
		// G-1: the Unicode word boundary is a named gap on the Go target.
		"\\bfoo\\b": "unicode_word_boundary",
		"a\\B":      "unicode_word_boundary",
	}
	for pattern, want := range cases {
		_, err := RegexNew(pattern)
		if got := rejectID(err); got != want {
			t.Errorf("RegexNew(%q) id = %q, want %q (%v)", pattern, got, want, err)
		}
	}
}

// TestRegexRejectPayload pins `<id>: <detail>` with a non-empty detail.
func TestRegexRejectPayload(t *testing.T) {
	_, err := RegexNew("a(?=b)")
	if err == nil || !strings.HasPrefix(err.Error(), "lookahead: ") || len(err.Error()) <= len("lookahead: ") {
		t.Fatalf("payload = %v", err)
	}
}

// TestRegexUnicodeClasses pins the dialect's Unicode `\w \d \s` (Go's are ASCII).
func TestRegexUnicodeClasses(t *testing.T) {
	cases := []struct {
		pattern string
		text    string
		want    bool
	}{
		{`^\w+$`, "héllo", true},
		{`^\w$`, "𝐀", true},
		{`^\w$`, "e\u0301", false},
		{`^\w+$`, "e\u0301", true},
		{`^\w$`, "\u0301", true},
		{`^\w$`, "\u200D", true},
		{`^\w$`, "‿", true},
		{`^\w$`, "²", false},
		{`^\w$`, "Ⅷ", true},
		{`^\w$`, "٣", true},
		{`^\W$`, "é", false},
		{`^\W$`, "\u00A0", true},
		{`^\d+$`, "٣", true},
		{`^\d$`, "𝟘", true},
		{`^\d$`, "²", false},
		{`^\d$`, "Ⅷ", false},
		{`^\D$`, "٣", false},
		{`^\s$`, "\u00A0", true},
		{`^\s$`, "\u0085", true},
		{`^\s$`, "\u2028", true},
		{`^\s$`, "\u200B", false},
		{`^\s$`, "\uFEFF", false},
		{`^\s$`, "\u180E", false},
		{`^\s$`, "\x1c", false},
		{`^\s$`, "\x0b", true},
		{`^\S$`, "\u00A0", false},
		{`^[\w-]+$`, "hé-llo", true},
		{`^[^\W\d]+$`, "héllo", true},
		{`^[^\W\d]+$`, "h3llo", false},
		{`^\p{Alphabetic}$`, "Ⅷ", true},
		{`^\P{Alphabetic}$`, "Ⅷ", false},
		{`^[[:alpha:]]+$`, "héllo", false},
		{`^[[:alpha:]]+$`, "hello", true},
		{`^\p{L}+$`, "héllo", true},
		{`^\pL+$`, "héllo", true},
		{`^\p{Greek}+$`, "αβγ", true},
		{`^\p{Lu}$`, "É", true},
	}
	for _, c := range cases {
		if got := mustRegex(t, c.pattern).Consentit(c.text); got != c.want {
			t.Errorf("%q over %q = %v, want %v", c.pattern, c.text, got, c.want)
		}
	}
}

// TestRegexFlags pins the flag semantics, including the dropped `x`.
func TestRegexFlags(t *testing.T) {
	cases := []struct {
		pattern string
		text    string
		want    bool
	}{
		{"(?x)a b # comment\n c", "abc", true},
		{`(?x)a\ b`, "a b", true},
		{`(?x)[a ]b`, " b", false},
		{`(?x)[a ]b`, "ab", true},
		{`(?m:^b)`, "a\nb", true},
		{`a(?i)b`, "aB", true},
		{`(a(?i)b)c`, "aBc", true},
		{`(a(?i)b)c`, "aBC", false},
		{`(?i)a(?-i)b`, "Ab", true},
		{`(?i)a(?-i)b`, "AB", false},
		{`(?i:a)b`, "Ab", true},
		{`(?i:a)b`, "AB", false},
		{`(?i)^σ$`, "ς", true},
		{`(?i)^ß$`, "ẞ", true},
		{`(?i)^ß$`, "SS", false},
		{`(?i)^k$`, "\u212A", true},
		{`(?i)^i$`, "\u0130", false},
		{`(?i)^ǆ$`, "ǅ", true},
		{`(?i)^[a-z]+$`, "\u017F", true},
		// the six pairs Go's own folding misses
		{"(?i)\u0390", "\u1FD3", true},
		{"(?i)\u1FE3", "\u03B0", true},
		{"(?i)\uFB05", "\uFB06", true},
		{"(?i)[\u0390x]", "\u1FD3", true},
		{"\u0390", "\u1FD3", false},
		{`(?s)^.$`, "\n", true},
		{`^.$`, "\n", false},
		{`^.$`, "\r", true},
		{`^.$`, "\u2028", true},
		{`^.$`, "😀", true},
		{`^.$`, "e\u0301", false},
		{`(?m)a$`, "a\r\nb", false},
		{`(?m)^b`, "a\rb", false},
		{`a$`, "a\n", false},
		{`a\z`, "a\n", false},
	}
	for _, c := range cases {
		if got := mustRegex(t, c.pattern).Consentit(c.text); got != c.want {
			t.Errorf("%q over %q = %v, want %v", c.pattern, c.text, got, c.want)
		}
	}
}

// TestRegexIteration pins the empty-match rule and code point offsets.
func TestRegexIteration(t *testing.T) {
	cases := []struct {
		pattern string
		text    string
		want    string
	}{
		{"a*", "baaac", "(0,0) (1,4) (5,5)"},
		{"a*", "aa", "(0,2)"},
		{"a|", "aa", "(0,1) (1,2)"},
		{"", "ab", "(0,0) (1,1) (2,2)"},
		{"x*", "", "(0,0)"},
		{"a*", "😀a", "(0,0) (1,2)"},
		{`^a`, "aa", "(0,1)"},
		{`(?m)^`, "a\nb\n", "(0,0) (2,2) (4,4)"},
		{"a", "😀a\u0301a", "(1,2) (3,4)"},
		{"😀", "x😀y", "(1,2)"},
		{"[éa]", "éaé", "(0,1) (1,2) (2,3)"},
		{`\w+`, "héllo wörld", "(0,5) (6,11)"},
	}
	for _, c := range cases {
		if got := spans(mustRegex(t, c.pattern), c.text); got != c.want {
			t.Errorf("%q over %q = %q, want %q", c.pattern, c.text, got, c.want)
		}
	}
}

// TestRegexFindAndGroups pins find, the accessors and named groups.
func TestRegexFindAndGroups(t *testing.T) {
	pair := mustRegex(t, "(?P<key>[a-z]+)=(?P<val>[0-9]+)")
	hit := pair.Inventa("x: size=42;")
	if hit == nil || hit.Inventum() != "size=42" || hit.Principium() != 3 || hit.Terminus() != 10 {
		t.Fatalf("find = %+v", hit)
	}
	if got := hit.Coetus(0); got == nil || *got != "size=42" {
		t.Fatalf("group 0 = %v", got)
	}
	if got := hit.Nominatus("key"); got == nil || *got != "size" {
		t.Fatalf("named key = %v", got)
	}
	if hit.Nominatus("nope") != nil || hit.Coetus(9) != nil || hit.Coetus(-1) != nil {
		t.Fatal("unknown name or index must be none")
	}
	if pair.Inventa("no pairs") != nil {
		t.Fatal("miss must be nil")
	}
	either := mustRegex(t, "(a)|(b)").Inventa("xb")
	if either.Coetus(1) != nil || either.Coetus(2) == nil || *either.Coetus(2) != "b" {
		t.Fatalf("alternation groups = %+v", either)
	}
	wide := mustRegex(t, "[a-z]+").Inventa("😀é abc")
	if wide.Principium() != 3 || wide.Terminus() != 6 {
		t.Fatalf("code point span = (%d,%d)", wide.Principium(), wide.Terminus())
	}
	nonpart := mustRegex(t, "(?P<n>a)|(?P<m>b)").Inventa("b")
	if nonpart.Nominatus("n") != nil || nonpart.Nominatus("m") == nil {
		t.Fatal("non-participating named group must be none")
	}
	nameFirst := mustRegex(t, "(a)(?P<n>b)(c)").Inventa("abc")
	if got := nameFirst.Nominatus("n"); got == nil || *got != "b" || *nameFirst.Coetus(3) != "c" {
		t.Fatalf("numbering = %+v", nameFirst)
	}
	if got := mustRegex(t, "(?P<é>a)").Inventa("a").Nominatus("é"); got == nil || *got != "a" {
		t.Fatal("unicode group name")
	}
	repeat := mustRegex(t, "((a)|b)*").Inventa("ab")
	if g := repeat.Coetus(2); g == nil || *g != "a" {
		t.Fatalf("group repeat keeps the last participating value, got %v", g)
	}
	star := mustRegex(t, "(a*)*").Inventa("b")
	if g := star.Coetus(1); g == nil || *g != "" {
		t.Fatalf("(a*)* over b group 1 = %v", g)
	}
}

// TestRegexDivide pins split pieces.
func TestRegexDivide(t *testing.T) {
	cases := []struct {
		pattern string
		text    string
		want    string
	}{
		{",", ",a,", "|a|"},
		{",", "a,,b", "a||b"},
		{"(,)", "a,b", "a|b"},
		{`\s+`, " a  b ", "|a|b|"},
		{"a*", "baaac", "|b|c|"},
		{"", "abc", "|a|b|c|"},
		{"x", "abc", "abc"},
		{",", "", ""},
		{",", "😀,a", "😀|a"},
		{"abc", "abc", "|"},
	}
	for _, c := range cases {
		if got := strings.Join(mustRegex(t, c.pattern).Divide(c.text), "|"); got != c.want {
			t.Errorf("%q split %q = %q, want %q", c.pattern, c.text, got, c.want)
		}
	}
	if got := mustRegex(t, ",").Divide(""); len(got) != 1 || got[0] != "" {
		t.Errorf("empty text must be one empty piece, got %q", got)
	}
}

// TestRegexMutaAndEach pins literal replace and the closure form.
func TestRegexMutaAndEach(t *testing.T) {
	if got := mustRegex(t, "a+").Muta("caab aa", "x"); got != "cxb x" {
		t.Errorf("a+ = %q", got)
	}
	if got := mustRegex(t, "(?P<n>[a-z])([0-9])").Muta("a1 b2", "${n}-$2"); got != "${n}-$2 ${n}-$2" {
		t.Errorf("literal replacement = %q", got)
	}
	if got := mustRegex(t, "a*").Muta("baaac", "-"); got != "-b-c-" {
		t.Errorf("a* = %q", got)
	}
	if got := mustRegex(t, "").Muta("ab", "-"); got != "-a-b-" {
		t.Errorf("empty pattern = %q", got)
	}
	upper := func(m RegexMatch) string { return strings.ToUpper(m.Inventum()) }
	if got := RegexReplaceEach(mustRegex(t, "[a-z]+"), "ab cd e", upper); got != "AB CD E" {
		t.Errorf("each = %q", got)
	}
	again := func(m RegexMatch) string { return "aa" }
	if got := RegexReplaceEach(mustRegex(t, "a"), "banana", again); got != "baanaanaa" {
		t.Errorf("a result is never searched again, got %q", got)
	}
	stars := func(m RegexMatch) string { return fmt.Sprintf("<%d-%d>", m.Principium(), m.Terminus()) }
	if got := RegexReplaceEach(mustRegex(t, "a*"), "baaac", stars); got != "<0-0>b<1-4>c<5-5>" {
		t.Errorf("empty-match rule = %q", got)
	}
	at := func(m RegexMatch) string { return fmt.Sprintf("%s@%d", m.Inventum(), m.Principium()) }
	if got := RegexReplaceEach(mustRegex(t, "[a-z]+"), "😀é abc", at); got != "😀é abc@3" {
		t.Errorf("offsets = %q", got)
	}
}

// TestRegexEscape pins the escaped set and the literal round trip.
func TestRegexEscape(t *testing.T) {
	if got := RegexEscape("a.b*c"); got != `a\.b\*c` {
		t.Errorf("escape = %q", got)
	}
	if got := RegexEscape(`\#&-~ é😀 x`); got != `\\\#\&\-\~ é😀 x` {
		t.Errorf("escape = %q", got)
	}
	probes := []string{`\`, ".", "+", "*", "?", "(", ")", "|", "[", "]", "{", "}", "^", "$", "#", "&", "-", "~",
		"a.b", "x+y*z", "(a|b)", "[a-z]{2}", "^start$", `a\d`, "tab\there", "line\nbreak", "é", "😀",
		"naïve café 😀", "日本語", "", "plain"}
	for _, probe := range probes {
		hit := mustRegex(t, "^"+RegexEscape(probe)+"$").Inventa(probe)
		if hit == nil || hit.Inventum() != probe {
			t.Errorf("round trip of %q failed", probe)
		}
	}
}

// TestRegexString pins the display of a regex as its pattern text.
func TestRegexString(t *testing.T) {
	if got := fmt.Sprint(mustRegex(t, `\d+`)); got != `\d+` {
		t.Errorf("display = %q", got)
	}
}

// TestRegexLinear checks the 1 MB bulk operations stay linear: a doubled
// input costs no more than three times the time, and the nested-repetition
// worst case of backtracking engines is immediate.
func TestRegexLinear(t *testing.T) {
	build := func(n int) string { return strings.Repeat("ab é😀 cd ", n) }
	run := func(text string) time.Duration {
		r := mustRegex(t, `[a-z]+`)
		start := time.Now()
		found := r.Collecta(text)
		_ = r.Divide(text)
		_ = r.Muta(text, "x")
		_ = RegexReplaceEach(r, text, func(m RegexMatch) string { return m.Inventum() })
		if len(found) == 0 {
			t.Fatal("no matches")
		}
		return time.Since(start)
	}
	one := run(build(100000)) // about 1 MB
	two := run(build(200000))
	t.Logf("find_all + split + replace + replace_each: 1 MB %v, 2 MB %v", one, two)
	if two > 3*one+50*time.Millisecond {
		t.Errorf("2 MB took %v, 1 MB took %v: not linear", two, one)
	}
	nested := mustRegex(t, `^(a+)+$`)
	start := time.Now()
	if nested.Consentit(strings.Repeat("a", 5000) + "b") {
		t.Error("nested repetition matched")
	}
	if time.Since(start) > 2*time.Second {
		t.Error("nested repetition is not linear")
	}
}
