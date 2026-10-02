import _StringProcessing

/// The engine behind a compiled pattern: Swift's own `Regex`, spelled through
/// its module because the namespace below is also called `Regex`.
private typealias Engine = _StringProcessing.Regex<AnyRegexOutput>

/// One match as the engine reports it, before it becomes a Faber match value.
private typealias _Match = _StringProcessing.Regex<AnyRegexOutput>.Match

/// The `regex` runtime namespace of Faber-generated Swift (`Namespace::Regex`
/// in the compiler's runtime-helper table; spelling `Regex.<name>`).
///
/// A Faber `regex` is a pattern compiled once, at the `↦ regex` conversion, by
/// Swift's native `Regex` under scalar semantics and the simple word boundary.
/// The dialect is RE2: the pattern is checked by `RegexPatternScanner` before
/// the engine sees it, so the constructs Swift accepts and the dialect does not
/// (lookaround, backreferences, ...) fail the conversion with a stable
/// construct id. Positions are Unicode code points, half-open; the empty-match
/// rule is the reference engine's: an empty match that starts where the
/// previous match ended is skipped.
///
/// Target gaps (the engine cannot carry them; see `docs/design/regex-dialect.md`
/// "Target gap list"): `(?m)` line boundaries other than `\n` (G-3,
/// `multiline_line_terminators`), a word boundary next to a combining mark
/// (G-4, `word_boundary_combining_marks`) and linear-time matching (G-7,
/// `linear_time`).
public enum Regex {
    /// A compiled pattern. Prints as its pattern text.
    public final class Regex: CustomStringConvertible {
        /// The pattern text, as written.
        public let pattern: String
        fileprivate let engine: Engine

        fileprivate init(pattern: String, engine: Engine) {
            self.pattern = pattern
            self.engine = engine
        }

        public var description: String { pattern }
    }

    /// One successful match: its text, its code point span and its groups.
    public struct Match {
        /// The matched text.
        public let text: String
        /// The code point offset of the match start.
        public let start: Int
        /// The code point offset just past the match end.
        public let end: Int
        /// Group 0 (the whole match) and each capture group, `nil` for a
        /// group that did not take part in the match.
        fileprivate let groups: [String?]
        /// Named group to group index.
        fileprivate let names: [String: Int]
    }

    // MARK: conversion

    /// Compile `pattern`; a pattern the dialect rejects fails with a payload
    /// `<construct id>: <detail>` (`syntax`, `lookahead`, ...). A conversion
    /// used as a statement only checks the pattern.
    @discardableResult
    public static func compile(_ pattern: String) throws -> Regex {
        let translated: String
        switch RegexPatternScanner.scan(pattern) {
        case .rejected(let id):
            throw FaberRuntimeError.error("\(id): \(pattern)")
        case .translated(let text):
            translated = text
        }
        do {
            let engine = try Engine(translated)
                .matchingSemantics(.unicodeScalar)
                .wordBoundaryKind(.simple)
            return Regex(pattern: pattern, engine: engine)
        } catch {
            throw FaberRuntimeError.error("syntax: \(error)")
        }
    }

    // MARK: operations

    /// `consentit`: whether the pattern matches anywhere in `text`.
    public static func matches(_ regex: Regex, _ text: String) -> Bool {
        text.firstMatch(of: regex.engine) != nil
    }

    /// `inventa`: the leftmost-first match, or `nil`.
    public static func find(_ regex: Regex, _ text: String) -> Match? {
        guard let found = text.firstMatch(of: regex.engine) else { return nil }
        var offsets = Offsets(text)
        return offsets.match(found)
    }

    /// `collecta`: every match, left to right, in one pass.
    public static func findAll(_ regex: Regex, _ text: String) -> [Match] {
        var offsets = Offsets(text)
        return iterate(regex, text).map { offsets.match($0) }
    }

    /// `divide`: the pieces between the matches; leading and trailing empty
    /// pieces are kept and capture groups are not spliced in.
    public static func split(_ regex: Regex, _ text: String) -> [String] {
        let scalars = text.unicodeScalars
        var pieces: [String] = []
        var from = scalars.startIndex
        for found in iterate(regex, text) {
            pieces.append(String(scalars[from..<found.range.lowerBound]))
            from = found.range.upperBound
        }
        pieces.append(String(scalars[from...]))
        return pieces
    }

    /// `muta` with a text: every match replaced by `replacement`, taken
    /// literally (`$1`, `${name}` and `\1` are plain text).
    public static func replace(_ regex: Regex, _ text: String, _ replacement: String) -> String {
        splice(regex, text) { _ in replacement }
    }

    /// `muta` with a closure: every match replaced by the closure's result
    /// for it, in order; results are literal and never searched again.
    public static func replaceEach(
        _ regex: Regex, _ text: String, _ replacement: (Match) -> String
    ) -> String {
        var offsets = Offsets(text)
        return splice(regex, text) { replacement(offsets.match($0)) }
    }

    /// The text between the matches joined with `replacement(match)`; one
    /// pass, one string built.
    private static func splice(
        _ regex: Regex, _ text: String,
        _ replacement: (_Match) -> String
    ) -> String {
        let scalars = text.unicodeScalars
        var out: [Unicode.Scalar] = []
        var from = scalars.startIndex
        for found in iterate(regex, text) {
            out.append(contentsOf: scalars[from..<found.range.lowerBound])
            out.append(contentsOf: replacement(found).unicodeScalars)
            from = found.range.upperBound
        }
        out.append(contentsOf: scalars[from...])
        var result = String.UnicodeScalarView()
        result.append(contentsOf: out)
        return String(result)
    }

    /// `munita`: a pattern text that matches `text` literally.
    public static func escape(_ text: String) -> String {
        var out = String.UnicodeScalarView()
        for scalar in text.unicodeScalars {
            if metacharacters.contains(scalar) {
                out.append("\\")
            }
            out.append(scalar)
        }
        return String(out)
    }

    private static let metacharacters = Set("\\.+*?()|[]{}^$#&-~".unicodeScalars)

    // MARK: match accessors

    /// `inventum`: the matched text.
    public static func matchText(_ match: Match) -> String { match.text }

    /// `principium`: the code point offset of the match start.
    public static func matchStart(_ match: Match) -> Int { match.start }

    /// `terminus`: the code point offset just past the match end.
    public static func matchEnd(_ match: Match) -> Int { match.end }

    /// `coetus`: group `index` (0 is the whole match), `nil` for a group that
    /// did not take part and for an index outside the pattern.
    public static func matchGroup(_ match: Match, _ index: Int) -> String? {
        match.groups.indices.contains(index) ? match.groups[index] : nil
    }

    /// `nominatus`: the group called `name`, `nil` when it did not take part
    /// and when the pattern has no such name.
    public static func matchNamed(_ match: Match, _ name: String) -> String? {
        match.names[name].flatMap { match.groups[$0] }
    }

    // MARK: iteration

    /// The engine's matches in order, without the empty match that starts
    /// where the previous match ended (the reference engine's rule).
    private static func iterate(_ regex: Regex, _ text: String) -> [_Match] {
        var kept: [_Match] = []
        var previousEnd: String.Index?
        for found in text.matches(of: regex.engine) {
            if found.range.isEmpty, found.range.lowerBound == previousEnd {
                continue
            }
            previousEnd = found.range.upperBound
            kept.append(found)
        }
        return kept
    }

    /// Code point offsets for a text, for matches that arrive in ascending
    /// order: each step counts only the scalars since the previous match.
    private struct Offsets {
        let text: String
        var index: String.Index
        var count = 0

        init(_ text: String) {
            self.text = text
            index = text.unicodeScalars.startIndex
        }

        mutating func offset(_ at: String.Index) -> Int {
            let scalars = text.unicodeScalars
            if at >= index {
                count += scalars.distance(from: index, to: at)
            } else {
                count -= scalars.distance(from: at, to: index)
            }
            index = at
            return count
        }

        mutating func match(_ found: _Match) -> Match {
            let scalars = text.unicodeScalars
            let start = offset(found.range.lowerBound)
            let end = offset(found.range.upperBound)
            var groups: [String?] = []
            var names: [String: Int] = [:]
            for (position, element) in found.output.enumerated() {
                groups.append(element.substring.map { String($0.unicodeScalars) })
                if let name = element.name {
                    names[name] = position
                }
            }
            if groups.isEmpty {
                groups = [String(scalars[found.range])]
            }
            return Match(
                text: String(scalars[found.range]),
                start: start,
                end: end,
                groups: groups,
                names: names
            )
        }
    }
}
