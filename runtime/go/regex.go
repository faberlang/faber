package rt

// The Faber `regex` carrier for generated Go programs (regex-operations RX-9).
//
// The dialect is RE2 (docs/design/regex-dialect.md in radix): linear time,
// leftmost-first, no lookaround, no backreferences, `\w \d \s \b` Unicode,
// offsets counted in code points. Go's `regexp` is an RE2 engine and is the
// matcher here; nothing in this file matches text. RegexNew does two things
// before the engine sees a pattern and RegexRegex does one thing after:
//
//   - validation: the constructs the dialect rejects fail the conversion with
//     a stable construct id (`lookahead`, `lookbehind`, `backreference`,
//     `unsupported_flag`, `atomic_group`, `possessive`, `recursion`,
//     `conditional`, `syntax`); the payload is `<id>: <detail>` and only the id
//     is a contract. A pattern is never a panic.
//   - translation: a token-level rewrite (escapes, bracket classes, group
//     openers, flags; no grammar, no matcher) that gives Go's ASCII `\w \d \s`
//     the dialect's Unicode meaning, drops the `x` flag (Go has none) and
//     renames groups (Go rejects names the dialect accepts and accepts names
//     the dialect rejects, so the carrier keeps the name table itself).
//   - offsets: the engine reports byte offsets; every operation converts to
//     code points once per call with a running count, so a bulk operation over
//     a large text stays linear.
//
// Target gap G-1 (`unicode_word_boundary`): RE2 syntax has only the ASCII
// `\b`, and a Unicode word boundary needs lookaround, which Go does not have.
// A pattern containing `\b` or `\B` is a rejected pattern on this target.
// Residual: the Unicode tables are Go's (unicode.Version), not the reference
// engine's, so a code point assigned after Go's Unicode version is not a word
// character here.

import (
	"regexp"
	"sort"
	"strconv"
	"strings"
	"sync"
	"unicode"
	"unicode/utf8"
)

// RegexRegex is the compiled `regex` value. The zero value is not a usable
// regex; construct one with RegexNew.
type RegexRegex struct {
	pattern string
	re      *regexp.Regexp
	names   map[string]int
}

// RegexMatch is one successful match: the matched text, its half-open code
// point span, every group's text (none for a group that did not take part)
// and the name table of the regex it came from.
type RegexMatch struct {
	text   string
	start  int
	end    int
	groups []*string
	names  map[string]int
}

// regexReject is the failure payload of a rejected pattern.
type regexReject struct {
	id     string
	detail string
}

func (e *regexReject) Error() string { return e.id + ": " + e.detail }

func regexSyntax(detail string) error { return &regexReject{id: "syntax", detail: detail} }

// RegexNew validates, translates and compiles `pattern` once. A rejected
// pattern is the returned error (`<id>: <detail>`), never a panic.
func RegexNew(pattern string) (RegexRegex, error) {
	translated, names, err := regexTranslate(pattern)
	if err != nil {
		return RegexRegex{}, err
	}
	re, cerr := regexp.Compile(translated)
	if cerr != nil {
		return RegexRegex{}, regexSyntax(regexCompileDetail(cerr))
	}
	return RegexRegex{pattern: pattern, re: re, names: names}, nil
}

// regexCompileDetail trims Go's `error parsing regexp: ` prefix and the echoed
// translated expression, which would leak the rewrite into the payload.
func regexCompileDetail(err error) string {
	text := err.Error()
	text = strings.TrimPrefix(text, "error parsing regexp: ")
	if i := strings.Index(text, ": `"); i >= 0 {
		text = text[:i]
	}
	return text
}

// String is the pattern text the regex was built from.
func (r RegexRegex) String() string { return r.pattern }

// Consentit reports whether the pattern matches anywhere in `text`.
func (r RegexRegex) Consentit(text string) bool { return r.re.MatchString(text) }

// Inventa is the leftmost-first match in `text`, or nil.
func (r RegexRegex) Inventa(text string) *RegexMatch {
	loc := r.re.FindStringSubmatchIndex(text)
	if loc == nil {
		return nil
	}
	counter := regexCounter{text: text}
	found := r.newMatch(text, loc, &counter)
	return &found
}

// Collecta is every non-overlapping match, left to right. The engine's
// iteration skips an empty match that starts where the previous match ended,
// so `a*` over `baaac` yields (0,0) (1,4) (5,5).
func (r RegexRegex) Collecta(text string) []RegexMatch {
	locs := r.re.FindAllStringSubmatchIndex(text, -1)
	found := make([]RegexMatch, 0, len(locs))
	counter := regexCounter{text: text}
	for _, loc := range locs {
		found = append(found, r.newMatch(text, loc, &counter))
	}
	return found
}

// Divide is the pieces of `text` between the iteration's matches. Leading and
// trailing empty pieces are kept and a capture group is never spliced into the
// output (Go's own Split drops the empty pieces the dialect keeps).
func (r RegexRegex) Divide(text string) []string {
	locs := r.re.FindAllStringIndex(text, -1)
	pieces := make([]string, 0, len(locs)+1)
	last := 0
	for _, loc := range locs {
		pieces = append(pieces, text[last:loc[0]])
		last = loc[1]
	}
	return append(pieces, text[last:])
}

// Muta is `text` with every match replaced by `replacement`, taken literally:
// `$1`, `${name}` and `\1` stay text.
func (r RegexRegex) Muta(text string, replacement string) string {
	return r.re.ReplaceAllLiteralString(text, replacement)
}

// RegexReplaceEach is `text` with the i-th match of the iteration replaced by
// replacer(match i). The output is assembled from the unmatched text and the
// replacements only, so a replacement is never searched again.
func RegexReplaceEach(r RegexRegex, text string, replacer func(RegexMatch) string) string {
	locs := r.re.FindAllStringSubmatchIndex(text, -1)
	counter := regexCounter{text: text}
	var out strings.Builder
	out.Grow(len(text))
	last := 0
	for _, loc := range locs {
		found := r.newMatch(text, loc, &counter)
		out.WriteString(text[last:loc[0]])
		out.WriteString(replacer(found))
		last = loc[1]
	}
	out.WriteString(text[last:])
	return out.String()
}

// RegexEscape is a pattern text matching `text` literally: it backslash-escapes
// exactly the dialect's metacharacters `\ . + * ? ( ) | [ ] { } ^ $ # & - ~` and
// leaves every other character, including non-ASCII text, unchanged.
func RegexEscape(text string) string {
	var out strings.Builder
	out.Grow(len(text))
	for _, c := range text {
		if strings.ContainsRune(`\.+*?()|[]{}^$#&-~`, c) {
			out.WriteByte('\\')
		}
		out.WriteRune(c)
	}
	return out.String()
}

// Inventum is the matched text.
func (m RegexMatch) Inventum() string { return m.text }

// Principium is the code point offset where the match starts.
func (m RegexMatch) Principium() int { return m.start }

// Terminus is the code point offset just past the match.
func (m RegexMatch) Terminus() int { return m.end }

// Coetus is the text of group `index` (0 is the whole match), or nil when the
// group did not take part or the index is past the last group.
func (m RegexMatch) Coetus(index int) *string {
	if index < 0 || index >= len(m.groups) {
		return nil
	}
	return m.groups[index]
}

// Nominatus is the text of the named group, or nil when the group did not take
// part or no group has that name.
func (m RegexMatch) Nominatus(name string) *string {
	index, ok := m.names[name]
	if !ok {
		return nil
	}
	return m.Coetus(index)
}

// regexCounter maps byte offsets to code point offsets with one running count.
// Offsets must be asked for in ascending order, which is the order the engine
// reports matches in.
type regexCounter struct {
	text   string
	bytes  int
	points int
}

func (c *regexCounter) at(offset int) int {
	c.points += utf8.RuneCountInString(c.text[c.bytes:offset])
	c.bytes = offset
	return c.points
}

func (r RegexRegex) newMatch(text string, loc []int, counter *regexCounter) RegexMatch {
	found := RegexMatch{
		text:   text[loc[0]:loc[1]],
		start:  counter.at(loc[0]),
		end:    counter.at(loc[1]),
		groups: make([]*string, len(loc)/2),
		names:  r.names,
	}
	for i := range found.groups {
		if loc[2*i] >= 0 {
			group := text[loc[2*i]:loc[2*i+1]]
			found.groups[i] = &group
		}
	}
	return found
}

// ---------------------------------------------------------------------------
// Unicode class tables

type regexRange struct{ lo, hi rune }

// regexSet is a code point set as sorted, non-adjacent ranges, with the text
// of the set and of its complement as Go class members (`\x{lo}-\x{hi}...`).
type regexSet struct {
	text       string
	complement string
}

var (
	regexSetsOnce sync.Once
	regexWord     regexSet
	regexDigit    regexSet
	regexSpace    regexSet
	regexAlpha    regexSet
)

func regexSets() {
	regexSetsOnce.Do(func() {
		// The reference `\w` is Alphabetic + M + Nd + Pc + Join_Control, where
		// Alphabetic is L + Nl + Other_Alphabetic.
		alpha := regexRanges(unicode.L, unicode.Nl, unicode.Other_Alphabetic)
		word := regexRanges(unicode.L, unicode.Nl, unicode.Other_Alphabetic, unicode.M, unicode.Nd, unicode.Pc, unicode.Join_Control)
		regexAlpha = regexMakeSet(alpha)
		regexWord = regexMakeSet(word)
		regexDigit = regexMakeSet(regexRanges(unicode.Nd))
		regexSpace = regexMakeSet(regexRanges(unicode.White_Space))
	})
}

func regexRanges(tables ...*unicode.RangeTable) []regexRange {
	var ranges []regexRange
	for _, table := range tables {
		for _, r := range table.R16 {
			ranges = regexStride(ranges, rune(r.Lo), rune(r.Hi), rune(r.Stride))
		}
		for _, r := range table.R32 {
			ranges = regexStride(ranges, rune(r.Lo), rune(r.Hi), rune(r.Stride))
		}
	}
	return regexNormalize(ranges)
}

func regexStride(ranges []regexRange, lo, hi, stride rune) []regexRange {
	if stride == 1 {
		return append(ranges, regexRange{lo, hi})
	}
	for c := lo; c <= hi; c += stride {
		ranges = append(ranges, regexRange{c, c})
	}
	return ranges
}

func regexNormalize(ranges []regexRange) []regexRange {
	sort.Slice(ranges, func(i, j int) bool { return ranges[i].lo < ranges[j].lo })
	var merged []regexRange
	for _, r := range ranges {
		if n := len(merged); n > 0 && r.lo <= merged[n-1].hi+1 {
			if r.hi > merged[n-1].hi {
				merged[n-1].hi = r.hi
			}
			continue
		}
		merged = append(merged, r)
	}
	return merged
}

// regexMakeSet renders the set and its complement over the scalar values
// (the surrogate block is excluded from the complement, as in the reference).
func regexMakeSet(ranges []regexRange) regexSet {
	var complement []regexRange
	next := rune(0)
	for _, r := range ranges {
		if r.lo > next {
			complement = append(complement, regexRange{next, r.lo - 1})
		}
		next = r.hi + 1
	}
	if next <= unicode.MaxRune {
		complement = append(complement, regexRange{next, unicode.MaxRune})
	}
	var scalar []regexRange
	for _, r := range complement {
		switch {
		case r.hi < 0xD800 || r.lo > 0xDFFF:
			scalar = append(scalar, r)
		default:
			if r.lo < 0xD800 {
				scalar = append(scalar, regexRange{r.lo, 0xD7FF})
			}
			if r.hi > 0xDFFF {
				scalar = append(scalar, regexRange{0xE000, r.hi})
			}
		}
	}
	return regexSet{text: regexRangeText(ranges), complement: regexRangeText(scalar)}
}

func regexRangeText(ranges []regexRange) string {
	var out strings.Builder
	for _, r := range ranges {
		out.WriteString(`\x{`)
		out.WriteString(strconv.FormatInt(int64(r.lo), 16))
		out.WriteByte('}')
		if r.hi != r.lo {
			out.WriteString(`-\x{`)
			out.WriteString(strconv.FormatInt(int64(r.hi), 16))
			out.WriteByte('}')
		}
	}
	return out.String()
}

// regexFoldPartner holds the simple case folding pairs Go's `(?i)` misses
// (the exhaustive probe of docs/design/regex-dialect.md found exactly these
// six code points): a literal in a case-insensitive region also matches its
// partner.
var regexFoldPartner = map[rune]rune{
	0x0390: 0x1FD3, 0x1FD3: 0x0390,
	0x03B0: 0x1FE3, 0x1FE3: 0x03B0,
	0xFB05: 0xFB06, 0xFB06: 0xFB05,
}

// ---------------------------------------------------------------------------
// Pattern translation

type regexFrame struct {
	ignoreSpace bool
	fold        bool
}

type regexTranslator struct {
	src    []rune
	pos    int
	out    strings.Builder
	frames []regexFrame
	groups int
	names  map[string]int
}

// regexTranslate validates `pattern` against the dialect and rewrites it into
// the Go expression with the same meaning. It also returns the name table.
func regexTranslate(pattern string) (string, map[string]int, error) {
	regexSets()
	t := &regexTranslator{
		src:    []rune(pattern),
		frames: []regexFrame{{}},
		names:  map[string]int{},
	}
	if err := t.run(); err != nil {
		return "", nil, err
	}
	return t.out.String(), t.names, nil
}

func (t *regexTranslator) frame() *regexFrame { return &t.frames[len(t.frames)-1] }

func (t *regexTranslator) more() bool { return t.pos < len(t.src) }

func (t *regexTranslator) peek() rune {
	if t.pos < len(t.src) {
		return t.src[t.pos]
	}
	return -1
}

// skipSpace drops whitespace and `#` comments while the `x` flag is on.
func (t *regexTranslator) skipSpace() {
	if !t.frame().ignoreSpace {
		return
	}
	for t.more() {
		c := t.src[t.pos]
		switch {
		case unicode.IsSpace(c):
			t.pos++
		case c == '#':
			for t.more() && t.src[t.pos] != '\n' {
				t.pos++
			}
		default:
			return
		}
	}
}

func (t *regexTranslator) run() error {
	for {
		t.skipSpace()
		if !t.more() {
			return nil
		}
		c := t.src[t.pos]
		t.pos++
		var err error
		switch c {
		case '\\':
			err = t.escape(false)
		case '[':
			err = t.class()
		case '(':
			err = t.group()
		case ')':
			if len(t.frames) > 1 {
				t.frames = t.frames[:len(t.frames)-1]
			}
			t.out.WriteByte(')')
		case '*', '+', '?', '{':
			t.pos--
			err = t.quantifier()
		case '|', '^', '$', '.':
			t.out.WriteRune(c)
		default:
			t.literal(c)
		}
		if err != nil {
			return err
		}
	}
}

func (t *regexTranslator) literal(c rune) {
	if partner, ok := regexFoldPartner[c]; ok && t.frame().fold {
		t.out.WriteString("[" + string(c) + string(partner) + "]")
		return
	}
	t.out.WriteRune(c)
}

// quantifier copies one repetition operator, rejecting a possessive or
// repeated one by name.
func (t *regexTranslator) quantifier() error {
	op, err := t.quantOp()
	if err != nil {
		return err
	}
	t.out.WriteString(op)
	t.skipSpace()
	if t.more() && strings.ContainsRune("*+?{", t.src[t.pos]) {
		outer, err := t.quantOp()
		if err != nil {
			return err
		}
		if outer == "+" {
			return &regexReject{id: "possessive", detail: "possessive quantifiers are not supported"}
		}
		return regexSyntax("repetition of a repetition")
	}
	return nil
}

// quantOp reads one operator with its optional lazy marker and returns the
// text (`*`, `+?`, `{2,3}`, ...). A lazy operator never reads as possessive:
// the returned text ends in `?`, so only a bare `+` is possessive.
func (t *regexTranslator) quantOp() (string, error) {
	start := t.pos
	switch t.src[t.pos] {
	case '*', '+', '?':
		t.pos++
	case '{':
		t.pos++
		low, ok := t.number()
		if !ok {
			return "", regexSyntax("invalid repetition count")
		}
		high := low
		if t.peek() == ',' {
			t.pos++
			if t.peek() == '}' {
				high = -1
			} else if high, ok = t.number(); !ok {
				return "", regexSyntax("invalid repetition count")
			}
		}
		if t.peek() != '}' {
			return "", regexSyntax("invalid repetition count")
		}
		t.pos++
		if low > 1000 || high > 1000 || (high >= 0 && low > high) {
			return "", regexSyntax("invalid repetition count")
		}
	}
	if t.peek() == '?' {
		t.pos++
	}
	return string(t.src[start:t.pos]), nil
}

func (t *regexTranslator) number() (int, bool) {
	start := t.pos
	for t.more() && t.src[t.pos] >= '0' && t.src[t.pos] <= '9' {
		t.pos++
	}
	if start == t.pos || t.pos-start > 6 {
		return 0, false
	}
	n, err := strconv.Atoi(string(t.src[start:t.pos]))
	return n, err == nil
}

// escape translates the escape after a backslash that is already consumed.
func (t *regexTranslator) escape(inClass bool) error {
	if !t.more() {
		return regexSyntax("trailing backslash")
	}
	c := t.src[t.pos]
	t.pos++
	switch c {
	case '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'k':
		return &regexReject{id: "backreference", detail: "backreferences are not supported"}
	case 'd', 'D':
		t.set(&regexDigit, c == 'D', inClass)
	case 'w', 'W':
		t.set(&regexWord, c == 'W', inClass)
	case 's', 'S':
		t.set(&regexSpace, c == 'S', inClass)
	case 'b', 'B':
		if inClass || t.peek() == '{' {
			return regexSyntax("unsupported assertion")
		}
		return &regexReject{id: "unicode_word_boundary", detail: `\b and \B are not supported on the Go target`}
	case 'A', 'z':
		if inClass {
			return regexSyntax("assertion inside a character class")
		}
		t.out.WriteString(`\` + string(c))
	case 'n', 'r', 't', 'f', 'v', 'a':
		t.out.WriteString(`\` + string(c))
	case 'x':
		return t.hex()
	case 'p', 'P':
		return t.property(c == 'P', inClass)
	default:
		if c < utf8.RuneSelf && !regexAlnum(c) && c != '<' && c != '>' {
			t.out.WriteString(`\` + string(c))
			return nil
		}
		return regexSyntax("unrecognized escape sequence")
	}
	return nil
}

func regexAlnum(c rune) bool {
	return c >= '0' && c <= '9' || c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z'
}

// set writes a Unicode class: its members inside a bracket expression, a
// bracket expression of its own outside one.
func (t *regexTranslator) set(set *regexSet, negate bool, inClass bool) {
	members := set.text
	if negate {
		members = set.complement
	}
	if inClass {
		t.out.WriteString(members)
		return
	}
	t.out.WriteString("[" + members + "]")
}

func (t *regexTranslator) hex() error {
	var digits string
	if t.peek() == '{' {
		end := t.pos + 1
		for end < len(t.src) && t.src[end] != '}' {
			end++
		}
		if end >= len(t.src) {
			return regexSyntax("invalid hexadecimal escape")
		}
		digits = string(t.src[t.pos+1 : end])
		t.pos = end + 1
	} else {
		if t.pos+2 > len(t.src) {
			return regexSyntax("invalid hexadecimal escape")
		}
		digits = string(t.src[t.pos : t.pos+2])
		t.pos += 2
	}
	value, err := strconv.ParseUint(digits, 16, 32)
	if err != nil || digits == "" || len(digits) > 8 || value > unicode.MaxRune || (value >= 0xD800 && value <= 0xDFFF) {
		return regexSyntax("invalid hexadecimal escape")
	}
	t.out.WriteString(`\x{` + strconv.FormatUint(value, 16) + `}`)
	return nil
}

// property reads `\pL`, `\p{Name}` or `\p{^Name}`. `Alphabetic` is a Unicode
// binary property that Go's syntax does not have, so it is written as its
// ranges; every other name is Go's own (categories and scripts) and an unknown
// one fails in the engine's compile as `syntax`.
func (t *regexTranslator) property(negate bool, inClass bool) error {
	var name string
	if t.peek() == '{' {
		end := t.pos + 1
		for end < len(t.src) && t.src[end] != '}' {
			end++
		}
		if end >= len(t.src) {
			return regexSyntax("invalid property escape")
		}
		name = string(t.src[t.pos+1 : end])
		t.pos = end + 1
		if strings.HasPrefix(name, "^") {
			negate = !negate
			name = name[1:]
		}
	} else if t.more() {
		name = string(t.src[t.pos])
		t.pos++
	}
	if name == "" {
		return regexSyntax("invalid property escape")
	}
	if name == "Alphabetic" {
		t.set(&regexAlpha, negate, inClass)
		return nil
	}
	if negate {
		t.out.WriteString(`\P{` + name + `}`)
	} else {
		t.out.WriteString(`\p{` + name + `}`)
	}
	return nil
}

// class copies one bracket expression (the `[` is consumed), rewriting the
// escapes inside it and rejecting the extensions of the reference engine.
func (t *regexTranslator) class() error {
	t.out.WriteByte('[')
	if t.peek() == '^' {
		t.pos++
		t.out.WriteByte('^')
	}
	first := true
	for {
		t.skipSpace()
		if !t.more() {
			return regexSyntax("unclosed character class")
		}
		c := t.src[t.pos]
		t.pos++
		switch {
		case c == ']' && !first:
			t.out.WriteByte(']')
			return nil
		case c == ']':
			t.out.WriteString(`\]`)
		case c == '\\':
			if err := t.escape(true); err != nil {
				return err
			}
		case c == '[':
			if err := t.posix(); err != nil {
				return err
			}
		case (c == '&' || c == '-' || c == '~') && t.peek() == c:
			return regexSyntax("character class set operation")
		default:
			t.out.WriteRune(c)
			if partner, ok := regexFoldPartner[c]; ok && t.frame().fold {
				t.out.WriteRune(partner)
			}
		}
		first = false
	}
}

// posix copies `[:name:]` or `[:^name:]` inside a bracket expression; any
// other `[` there is a nested class, which the dialect rejects.
func (t *regexTranslator) posix() error {
	if t.peek() != ':' {
		return regexSyntax("nested character class")
	}
	end := t.pos + 1
	for end < len(t.src) && (t.src[end] >= 'a' && t.src[end] <= 'z' || t.src[end] == '^') {
		end++
	}
	if end+1 >= len(t.src) || t.src[end] != ':' || t.src[end+1] != ']' {
		return regexSyntax("nested character class")
	}
	t.out.WriteString("[" + string(t.src[t.pos:end+2]))
	t.pos = end + 2
	return nil
}

// group handles what follows an opening parenthesis (already consumed).
func (t *regexTranslator) group() error {
	if t.peek() != '?' {
		t.groups++
		t.open(*t.frame())
		t.out.WriteByte('(')
		return nil
	}
	t.pos++
	if !t.more() {
		return regexSyntax("unclosed group")
	}
	c := t.src[t.pos]
	switch c {
	case 'P':
		t.pos++
		switch t.peek() {
		case '<':
			t.pos++
			return t.named()
		case '=':
			return &regexReject{id: "backreference", detail: "backreferences are not supported"}
		case '>':
			return &regexReject{id: "recursion", detail: "recursion is not supported"}
		}
		return regexSyntax("unrecognized group")
	case '<':
		t.pos++
		if p := t.peek(); p == '=' || p == '!' {
			return &regexReject{id: "lookbehind", detail: "lookbehind is not supported"}
		}
		return t.named()
	case '=', '!':
		return &regexReject{id: "lookahead", detail: "lookahead is not supported"}
	case '>':
		return &regexReject{id: "atomic_group", detail: "atomic groups are not supported"}
	case '(':
		return &regexReject{id: "conditional", detail: "conditional groups are not supported"}
	case '&', '+', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9':
		return &regexReject{id: "recursion", detail: "recursion is not supported"}
	case '#', '\'':
		return regexSyntax("unrecognized group")
	case ':':
		t.pos++
		t.open(*t.frame())
		t.out.WriteString("(?:")
		return nil
	}
	return t.flags()
}

func (t *regexTranslator) open(frame regexFrame) { t.frames = append(t.frames, frame) }

// named reads `name>` (the opener is consumed) and starts a capture group. The
// group is written unnamed: the carrier resolves names itself.
func (t *regexTranslator) named() error {
	end := t.pos
	for end < len(t.src) && t.src[end] != '>' {
		end++
	}
	if end >= len(t.src) {
		return regexSyntax("unclosed group name")
	}
	name := string(t.src[t.pos:end])
	if !regexValidName(name) {
		return regexSyntax("invalid group name")
	}
	if _, dup := t.names[name]; dup {
		return regexSyntax("duplicate group name")
	}
	t.pos = end + 1
	t.groups++
	t.names[name] = t.groups
	t.open(*t.frame())
	t.out.WriteByte('(')
	return nil
}

func regexValidName(name string) bool {
	if name == "" {
		return false
	}
	for i, c := range name {
		switch {
		case c == '_':
		case i > 0 && (c == '.' || c == '[' || c == ']'):
		case unicode.IsLetter(c) || (i > 0 && (unicode.IsNumber(c))):
		default:
			return false
		}
	}
	return true
}

// flags reads `imsx` with an optional `-`, ended by `)` (sets the flags for the
// rest of the group) or `:` (scopes them to a new group). `x` is dropped from
// the Go expression: the translator applies it itself.
func (t *regexTranslator) flags() error {
	frame := *t.frame()
	var on, off []rune
	negating := false
	seen := 0
	for {
		if !t.more() {
			return regexSyntax("unclosed group")
		}
		c := t.src[t.pos]
		t.pos++
		switch {
		case c == 'i' || c == 'm' || c == 's' || c == 'x':
			seen++
			if negating {
				off = append(off, c)
			} else {
				on = append(on, c)
			}
			switch c {
			case 'x':
				frame.ignoreSpace = !negating
			case 'i':
				frame.fold = !negating
			}
		case c == 'R':
			return &regexReject{id: "recursion", detail: "recursion is not supported"}
		case c == '-':
			if negating {
				return regexSyntax("repeated flag negation")
			}
			negating = true
			seen++
		case c == ')' || c == ':':
			if seen == 0 || (negating && len(off) == 0) {
				return regexSyntax("missing flags")
			}
			return t.finishFlags(c == ':', on, off, frame)
		case c < utf8.RuneSelf && (c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z'):
			return &regexReject{id: "unsupported_flag", detail: "flag " + string(c) + " is not supported"}
		default:
			return regexSyntax("unrecognized group")
		}
	}
}

func (t *regexTranslator) finishFlags(scoped bool, on, off []rune, frame regexFrame) error {
	text := regexGoFlags(on, off)
	if scoped {
		t.open(frame)
		t.out.WriteString("(?" + text + ":")
		return nil
	}
	*t.frame() = frame
	if text != "" {
		t.out.WriteString("(?" + text + ")")
	}
	return nil
}

// regexGoFlags is the Go spelling of a flag set without `x`: `i`, `i-s`, `-s`
// or empty.
func regexGoFlags(on, off []rune) string {
	var text strings.Builder
	for _, c := range on {
		if c != 'x' {
			text.WriteRune(c)
		}
	}
	var negative strings.Builder
	for _, c := range off {
		if c != 'x' {
			negative.WriteRune(c)
		}
	}
	if negative.Len() > 0 {
		text.WriteString("-" + negative.String())
	}
	return text.String()
}
