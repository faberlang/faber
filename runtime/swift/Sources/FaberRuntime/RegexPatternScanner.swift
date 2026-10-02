/// Lexical dialect check and shorthand rewrite for a Faber `regex` pattern
/// (RE2 dialect, RD-1, RD-8).
///
/// Swift's native `Regex` accepts constructs the dialect does not have
/// (lookaround, backreferences, atomic groups, possessive quantifiers, class
/// set operations, `\Z`, `\Q…\E`, backtracking verbs, ...), so a pattern is
/// checked here, token by token, before the engine sees it. The scanner walks
/// escapes, bracket classes, group openers and quantifiers only; it never
/// matches text and never builds a syntax tree. A construct the dialect
/// rejects is named by a stable id (`lookahead`, `lookbehind`, `backreference`,
/// `unsupported_flag`, `atomic_group`, `possessive`, `recursion`,
/// `conditional`, `syntax`); everything the scanner lets through is compiled
/// by the engine, whose own error is `syntax`.
///
/// The same pass rewrites the few shorthands whose native meaning differs from
/// the dialect's, so the engine reads the pattern the dialect's way: `\w` and
/// `\d` become the Unicode property sets the reference uses, `.` becomes "any
/// scalar but `\n`" unless `(?s)` is on, the POSIX bracket names become their
/// ASCII ranges, `\pL` becomes `\p{L}` and `\v` becomes U+000B. It rewrites
/// tokens; it does not interpret the pattern's structure and owns no matcher.
///
/// Parse errors (an unclosed group, a lookaround, an unknown escape, ...) are
/// reported in source order; the reference extras the runner's parser accepts
/// and its second pass rejects (nested repeats, `(?u)`, `(?R)`, class set
/// operations, ...) are reported only when the pattern has no parse error, as
/// the runner does.
///
/// The ids and the reference extras that are rejected as `syntax` (class set
/// operations, nested classes, `\<`, `\b{start}`, `A`, `a**`, repeat
/// counts above 1000, `(?U)`, `(?R)`) follow the runner's validator
/// (`radix-mir-runner/src/regex_program.rs`) and `faber::Regex` in the Rust
/// runtime.
enum RegexPatternScanner {
    /// The largest `{n}` / `{n,m}` count the dialect accepts.
    static let repeatLimit = 1000

    /// What the scanner decided about a pattern.
    enum Outcome: Equatable {
        /// The id of the first construct the dialect rejects.
        case rejected(String)
        /// The pattern to hand to the engine.
        case translated(String)
    }

    /// Check `pattern` and rewrite its shorthands for the engine.
    static func scan(_ pattern: String) -> Outcome {
        var scan = Scan(Array(pattern.unicodeScalars))
        if let id = scan.run() {
            return .rejected(id)
        }
        var text = String.UnicodeScalarView()
        text.append(contentsOf: scan.out)
        return .translated(String(text))
    }

    /// The id of the first construct the dialect rejects, or `nil`.
    static func rejection(_ pattern: String) -> String? {
        if case .rejected(let id) = scan(pattern) {
            return id
        }
        return nil
    }

    /// `\w` of the dialect: Alphabetic, marks, decimal digits, connector
    /// punctuation and join controls.
    private static let wordProperties = #"\p{Alphabetic}\p{M}\p{Nd}\p{Pc}\p{Join_Control}"#

    /// The ASCII ranges of a POSIX bracket name, as hex escapes.
    private static func posixRanges(_ name: String) -> String? {
        switch name {
        case "alnum": return #"\x{30}-\x{39}\x{41}-\x{5A}\x{61}-\x{7A}"#
        case "alpha": return #"\x{41}-\x{5A}\x{61}-\x{7A}"#
        case "ascii": return #"\x{0}-\x{7F}"#
        case "blank": return #"\x{9}\x{20}"#
        case "cntrl": return #"\x{0}-\x{1F}\x{7F}"#
        case "digit": return #"\x{30}-\x{39}"#
        case "graph": return #"\x{21}-\x{7E}"#
        case "lower": return #"\x{61}-\x{7A}"#
        case "print": return #"\x{20}-\x{7E}"#
        case "punct": return #"\x{21}-\x{2F}\x{3A}-\x{40}\x{5B}-\x{60}\x{7B}-\x{7E}"#
        case "space": return #"\x{9}-\x{D}\x{20}"#
        case "upper": return #"\x{41}-\x{5A}"#
        case "word": return #"\x{30}-\x{39}\x{41}-\x{5A}\x{61}-\x{7A}\x{5F}"#
        case "xdigit": return #"\x{30}-\x{39}\x{41}-\x{46}\x{61}-\x{66}"#
        default: return nil
        }
    }

    /// The flags that change how a token is read, per group.
    private struct Flags {
        /// `(?x)`: whitespace and `#` comments are not part of the pattern.
        var extended = false
        /// `(?s)`: `.` matches `\n` too.
        var dotAll = false
    }

    /// One left-to-right pass over the pattern's scalars.
    private struct Scan {
        let p: [Unicode.Scalar]
        var i = 0
        /// The rewritten pattern.
        var out: [Unicode.Scalar] = []
        /// The flags of the current group and of each enclosing one.
        var flags: [Flags] = [Flags()]
        /// Whether an operand precedes the cursor, so a quantifier has
        /// something to repeat.
        var hasOperand = false
        /// The run of quantifiers that ends at the cursor, for `a+?`, `a++` and
        /// `a**`: how many repetitions it nests and whether the outermost is a
        /// greedy `+` (possessive in Python and Swift syntax).
        var quantifier = Quantifiers.none
        /// The first reference extra found; reported only when the whole
        /// pattern parses.
        var deferred: String?
        /// Where in `out` the assertion token (`^`, `\b`, ...) just before the
        /// cursor starts, so a quantifier after it can wrap it as `(?:…)`,
        /// which the engine accepts and the dialect allows.
        var assertionStart: Int?
        /// Whether the token being consumed is an assertion.
        var isAssertion = false
        /// The replacement text of the token being consumed, when it is
        /// rewritten; `nil` copies the token as written.
        var rewrite: [Unicode.Scalar]?

        init(_ scalars: [Unicode.Scalar]) {
            p = scalars
        }

        enum Quantifiers {
            /// The previous token was not a quantifier.
            case none
            /// `count` nested repetitions; `lazyable` while the last operator
            /// may still take a `?` suffix; `greedyPlus` when the outermost
            /// operator is a bare `+`.
            case chain(count: Int, lazyable: Bool, greedyPlus: Bool)
        }

        mutating func run() -> String? {
            while i < p.count {
                let start = i
                let before = out.count
                rewrite = nil
                isAssertion = false
                let keepAssertion = flags.last?.extended == true && skipsTrivia(p[i])
                if let id = step() {
                    return id
                }
                out.append(contentsOf: rewrite ?? Array(p[start..<i]))
                if isAssertion {
                    assertionStart = before
                } else if !keepAssertion {
                    assertionStart = nil
                }
            }
            if let id = endQuantifiers() {
                return id
            }
            return flags.count > 1 ? "syntax" : deferred
        }

        /// Whether `c` is `(?x)` trivia (so it neither ends a quantifier run
        /// nor separates an assertion from its quantifier).
        private func skipsTrivia(_ c: Unicode.Scalar) -> Bool {
            c == "#" || c.properties.isWhitespace
        }

        /// Note the first reference extra; it is reported if nothing else is
        /// wrong with the pattern.
        private mutating func noteExtra(_ id: String) {
            if deferred == nil {
                deferred = id
            }
        }

        private func at(_ index: Int) -> Unicode.Scalar? {
            index < p.count ? p[index] : nil
        }

        private func scalars(_ text: String) -> [Unicode.Scalar] {
            Array(text.unicodeScalars)
        }

        /// Consume one token; a non-nil result is the rejection id.
        private mutating func step() -> String? {
            let c = p[i]
            if flags.last?.extended == true, let skipped = skipExtendedTrivia(c) {
                i = skipped
                return nil
            }
            if !isQuantifierStart(c) {
                if let id = endQuantifiers() {
                    return id
                }
            }
            switch c {
            case "\\":
                return scanEscape()
            case "[":
                return scanClass()
            case "(":
                return scanGroupOpen()
            case ")":
                guard flags.count > 1 else { return "syntax" }
                flags.removeLast()
                i += 1
                hasOperand = true
                return nil
            case "|":
                i += 1
                hasOperand = false
                return nil
            case "*", "+", "?":
                i += 1
                return quantify(c)
            case "{":
                return scanCountedRepetition()
            case ".":
                i += 1
                hasOperand = true
                // Swift's `.` also stops at CR, NEL and the Unicode line
                // separators; the dialect's stops only at `\n`.
                if flags.last?.dotAll != true {
                    rewrite = scalars(#"[^\n]"#)
                }
                return nil
            case "^", "$":
                i += 1
                hasOperand = true
                isAssertion = true
                return nil
            default:
                i += 1
                hasOperand = true
                return nil
            }
        }

        /// Whether `c` starts a quantifier when it follows an operand.
        private func isQuantifierStart(_ c: Unicode.Scalar) -> Bool {
            c == "*" || c == "+" || c == "?" || c == "{"
        }

        /// Whitespace and `#` comments are not part of an `(?x)` pattern.
        private func skipExtendedTrivia(_ c: Unicode.Scalar) -> Int? {
            if c == "#" {
                var j = i + 1
                while j < p.count, p[j] != "\n" {
                    j += 1
                }
                return j
            }
            if c.properties.isWhitespace {
                return i + 1
            }
            return nil
        }

        /// A quantifier operator. Right after an operator a `?` makes it lazy;
        /// any other operator nests another repetition, which is a reference
        /// extra (possessive when the outermost is a bare `+`, else `syntax`).
        private mutating func quantify(_ c: Unicode.Scalar) -> String? {
            guard case .chain(let count, let lazyable, _) = quantifier else {
                guard hasOperand else { return "syntax" }
                wrapAssertion()
                quantifier = .chain(count: 1, lazyable: true, greedyPlus: c == "+")
                return nil
            }
            if c == "?", lazyable {
                quantifier = .chain(count: count, lazyable: false, greedyPlus: false)
            } else {
                quantifier = .chain(count: count + 1, lazyable: true, greedyPlus: c == "+")
            }
            return nil
        }

        /// A quantifier after an assertion: the engine only quantifies a
        /// group, so the assertion is wrapped as `(?:…)`.
        private mutating func wrapAssertion() {
            guard let start = assertionStart else { return }
            out.insert(contentsOf: scalars("(?:"), at: start)
            out.append(")")
            assertionStart = nil
        }

        /// The run of quantifiers ended: more than one nested repetition is a
        /// reference extra.
        private mutating func endQuantifiers() -> String? {
            guard case .chain(let count, _, let greedyPlus) = quantifier else { return nil }
            quantifier = .none
            if count > 1 {
                noteExtra(greedyPlus ? "possessive" : "syntax")
            }
            return nil
        }

        /// `{n}`, `{n,}` and `{n,m}`; any other `{` is not a repetition.
        private mutating func scanCountedRepetition() -> String? {
            var j = i + 1
            guard let low = digits(&j) else { return "syntax" }
            var high = low
            if at(j) == "," {
                j += 1
                high = digits(&j) ?? Int.max
            }
            guard at(j) == "}", high >= low else { return "syntax" }
            if low > repeatLimit || (high != Int.max && high > repeatLimit) {
                noteExtra("syntax")
            }
            i = j + 1
            return quantify("{")
        }

        /// A run of ASCII decimal digits, or `nil` when there is none; the value
        /// saturates far above the repeat limit.
        private func digits(_ j: inout Int) -> Int? {
            var value: Int?
            while let d = at(j), d.value >= 48, d.value <= 57 {
                value = min((value ?? 0) * 10 + Int(d.value) - 48, 1_000_000)
                j += 1
            }
            return value
        }

        // MARK: escapes

        /// An escape outside a class.
        private mutating func scanEscape() -> String? {
            if let id = escape(inClass: false) {
                return id
            }
            hasOperand = true
            return nil
        }

        /// Consume `\` and its escape at `i`; a non-nil result is the id. A
        /// shorthand the engine reads differently sets `rewrite`.
        private mutating func escape(inClass: Bool) -> String? {
            guard let e = at(i + 1) else { return "syntax" }
            i += 2
            switch e {
            case "0"..."9":
                return "backreference"
            case "k":
                // The parser reads every `\k` as a named backreference.
                return "backreference"
            case "x":
                return hexEscape()
            case "p", "P":
                return propertyEscape(negated: e == "P")
            case "w":
                let set = RegexPatternScanner.wordProperties
                rewrite = scalars(inClass ? set : "[\(set)]")
                return nil
            case "W":
                rewrite = scalars("[^\(RegexPatternScanner.wordProperties)]")
                return nil
            case "d":
                rewrite = scalars(#"\p{Nd}"#)
                return nil
            case "D":
                rewrite = scalars(#"\P{Nd}"#)
                return nil
            case "v":
                // A vertical tab, not Swift's "any vertical whitespace".
                rewrite = scalars(#"\x{B}"#)
                return nil
            case "n", "r", "t", "f", "a", "s", "S":
                return nil
            case "A", "z", "B":
                isAssertion = !inClass
                return inClass ? "syntax" : nil
            case "b":
                // `\b{start}` and the other braced forms are reference
                // extras; `\b{2}` is a counted repetition of the assertion; a
                // bare `\b` is a word boundary only outside a class.
                if inClass {
                    return "syntax"
                }
                if at(i) == "{", !(at(i + 1).map { $0.value >= 48 && $0.value <= 57 } ?? false) {
                    return "syntax"
                }
                isAssertion = true
                return nil
            case "<", ">", "u", "U":
                return "syntax"
            default:
                // Escaped ASCII punctuation and a space are literals; any
                // other letter or non-ASCII scalar is not an escape.
                let literal = e.isASCII && e.value >= 32 && e.value != 127
                    && !e.properties.isAlphabetic
                return literal ? nil : "syntax"
            }
        }

        /// `\x41` or `\x{1F600}`.
        private mutating func hexEscape() -> String? {
            if at(i) == "{" {
                var j = i + 1
                var count = 0
                while let d = at(j), d.properties.isASCIIHexDigit {
                    j += 1
                    count += 1
                }
                guard count > 0, at(j) == "}" else { return "syntax" }
                i = j + 1
                return nil
            }
            guard let a = at(i), let b = at(i + 1),
                a.properties.isASCIIHexDigit, b.properties.isASCIIHexDigit
            else { return "syntax" }
            i += 2
            return nil
        }

        /// `\pL` (rewritten to `\p{L}`), `\p{Greek}`, `\PL`, `\P{Greek}`; the
        /// engine judges the property name.
        private mutating func propertyEscape(negated: Bool) -> String? {
            guard let next = at(i) else { return "syntax" }
            if next == "{" {
                var j = i + 1
                while let d = at(j), d != "}" {
                    j += 1
                }
                guard at(j) == "}" else { return "syntax" }
                i = j + 1
                return nil
            }
            i += 1
            rewrite = scalars("\\\(negated ? "P" : "p"){\(next)}")
            return nil
        }

        // MARK: bracket classes

        /// A bracket class: ranges, escapes and `[:name:]`; a nested class
        /// and the set operations `&&`, `--` and `~~` are reference extras.
        private mutating func scanClass() -> String? {
            i += 1
            var text: [Unicode.Scalar] = ["["]
            if at(i) == "^" {
                text.append("^")
                i += 1
            }
            var first = true
            while true {
                guard let c = at(i) else { return "syntax" }
                let start = i
                let atStart = first
                rewrite = nil
                if c == "]", !first {
                    i += 1
                    text.append("]")
                    rewrite = text
                    hasOperand = true
                    return nil
                }
                first = false
                if c == "]" {
                    // A `]` first in a class is a literal; the engine reads
                    // `[]` as an empty class.
                    i += 1
                    rewrite = scalars(#"\]"#)
                } else if atStart, c == "-" {
                    // A `-` first in a class starts a range or is a literal;
                    // the engine reads a leading `--` as a set operation.
                    i += 1
                    rewrite = scalars(#"\-"#)
                } else if c == "[" {
                    guard at(i + 1) == ":", let (name, after) = posixName(from: i + 2) else {
                        return "syntax"
                    }
                    i = after
                    rewrite = posixClass(name)
                } else if c == "\\" {
                    if let id = escape(inClass: true) {
                        return id
                    }
                } else if !atStart, let next = at(i + 1), c == next,
                    c == "&" || c == "-" || c == "~"
                {
                    return "syntax"
                } else {
                    i += 1
                }
                text.append(contentsOf: rewrite ?? Array(p[start..<i]))
            }
        }

        /// The name of a POSIX class starting at `from` (after `[:`) and the
        /// index just past its `:]`.
        private func posixName(from: Int) -> (String, Int)? {
            var j = from
            var name = String.UnicodeScalarView()
            while let c = at(j) {
                if c == ":" {
                    guard at(j + 1) == "]" else { return nil }
                    return (String(name), j + 2)
                }
                guard c == "^" || (c.isASCII && c.properties.isAlphabetic) else { return nil }
                name.append(c)
                j += 1
            }
            return nil
        }

        /// A POSIX class as its ASCII ranges; `[:^name:]` is the complement,
        /// a nested negated class. An unknown name is left for the engine.
        private func posixClass(_ name: String) -> [Unicode.Scalar]? {
            let negated = name.hasPrefix("^")
            guard
                let ranges = RegexPatternScanner.posixRanges(
                    negated ? String(name.dropFirst()) : name)
            else {
                return nil
            }
            return scalars(negated ? "[^\(ranges)]" : ranges)
        }

        // MARK: groups

        /// `(`, `(?:`, `(?P<n>`, `(?<n>` open a group; `(?i)` only sets flags;
        /// every other `(?` form is a rejected construct.
        private mutating func scanGroupOpen() -> String? {
            i += 1
            hasOperand = false
            guard at(i) == "?" else {
                flags.append(flags.last ?? Flags())
                return nil
            }
            i += 1
            guard let kind = at(i) else { return "syntax" }
            switch kind {
            case ":":
                i += 1
                flags.append(flags.last ?? Flags())
                return nil
            case "=", "!":
                return "lookahead"
            case ">":
                return "atomic_group"
            case "(":
                return "conditional"
            case "#", "'":
                return "syntax"
            case "<":
                return namedOrLookbehind()
            case "P":
                return pythonNamed()
            case "&", "+", "0"..."9":
                return "recursion"
            default:
                return flagGroup()
            }
        }

        /// `(?<=` and `(?<!` are lookbehind; `(?<name>` opens a named group.
        private mutating func namedOrLookbehind() -> String? {
            if let next = at(i + 1), next == "=" || next == "!" {
                return "lookbehind"
            }
            return openNamed(from: i + 1)
        }

        /// `(?P<name>` opens a named group, `(?P=name)` is a backreference
        /// and `(?P>name)` a recursion.
        private mutating func pythonNamed() -> String? {
            switch at(i + 1) {
            case "<":
                return openNamed(from: i + 2)
            case "=":
                return "backreference"
            case ">":
                return "recursion"
            default:
                // A `P` that opens nothing is an unknown flag letter.
                return "unsupported_flag"
            }
        }

        /// Consume a group name up to `>`; the engine judges the name.
        private mutating func openNamed(from start: Int) -> String? {
            var j = start
            while let c = at(j), c != ">" {
                j += 1
            }
            guard at(j) == ">" else { return "syntax" }
            i = j + 1
            flags.append(flags.last ?? Flags())
            return nil
        }

        /// `(?flags)` and `(?flags:`: `i`, `m`, `s`, `x`, each at most once, and
        /// one `-` followed by a flag. `u`, `U` and `R` parse but are reference
        /// extras, reported once the group is complete.
        private mutating func flagGroup() -> String? {
            var j = i
            var on = true
            var sawMinus = false
            var flagsAfterMinus = 0
            var seen = Set<Unicode.Scalar>()
            var extra: String?
            var state = flags.last ?? Flags()
            while let c = at(j) {
                switch c {
                case "i", "m", "s", "x", "u", "U", "R":
                    guard seen.insert(c).inserted else { return "syntax" }
                    flagsAfterMinus += 1
                    switch c {
                    case "s": state.dotAll = on
                    case "x": state.extended = on
                    case "u", "U": extra = extra ?? "unsupported_flag"
                    case "R": extra = extra ?? "recursion"
                    default: break
                    }
                case "-":
                    guard !sawMinus else { return "syntax" }
                    if let next = at(j + 1), next.isASCII, next.properties.numericType != nil {
                        return "recursion"
                    }
                    sawMinus = true
                    on = false
                    flagsAfterMinus = 0
                case ")", ":":
                    if sawMinus, flagsAfterMinus == 0 {
                        return "syntax"
                    }
                    if let extra {
                        noteExtra(extra)
                    }
                    i = j + 1
                    if c == ")" {
                        flags[flags.count - 1] = state
                    } else {
                        flags.append(state)
                    }
                    return nil
                default:
                    return c.isASCII && c.properties.isAlphabetic ? "unsupported_flag" : "syntax"
                }
                j += 1
            }
            return "syntax"
        }
    }
}
