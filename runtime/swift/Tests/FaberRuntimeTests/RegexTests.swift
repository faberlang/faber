import XCTest

@testable import FaberRuntime

/// The `regex` runtime namespace: conversion failures carry a stable
/// construct id, positions are code points, and the iteration rule is the
/// reference engine's (compiler docs `docs/design/regex-dialect.md`).
final class RegexTests: XCTestCase {
    /// The leading construct id of a rejected pattern, `nil` when accepted.
    private func rejection(_ pattern: String) -> String? {
        do {
            _ = try Regex.compile(pattern)
            return nil
        } catch {
            return FaberRuntimeError.message(from: error).split(separator: ":").first.map(String.init)
        }
    }

    private func regex(_ pattern: String) throws -> Regex.Regex {
        try Regex.compile(pattern)
    }

    // MARK: conversion

    func testRejectedConstructsNameTheirId() {
        let cases: [(String, String)] = [
            ("a(?=b)", "lookahead"),
            ("a(?!b)", "lookahead"),
            ("(?<=a)b", "lookbehind"),
            ("(?<!a)b", "lookbehind"),
            ("(a)\\1", "backreference"),
            ("(?P<n>a)(?P=n)", "backreference"),
            ("(?P<n>a)\\k<n>", "backreference"),
            ("\\012", "backreference"),
            ("(?>a)", "atomic_group"),
            ("a++", "possessive"),
            ("a*+", "possessive"),
            ("a?+", "possessive"),
            ("a{2}+", "possessive"),
            ("(?R)", "recursion"),
            ("(a)(?1)", "recursion"),
            ("(?(1)a|b)", "conditional"),
            ("(?g)a", "unsupported_flag"),
            ("(?U)a+", "unsupported_flag"),
            ("(?u)a", "unsupported_flag"),
            ("(?-u)a", "unsupported_flag"),
        ]
        for (pattern, id) in cases {
            XCTAssertEqual(rejection(pattern), id, pattern)
        }
    }

    func testReferenceExtrasAndMalformedPatternsAreSyntax() {
        let patterns = [
            "(", "a)", "[", "a{2,1}", "*a", "a**", "\\", "\\Z", "\\G", "\\Qa.b\\E", "\\h", "\\R",
            "\\K", "\\p{Foo}", "a{100000}", "[]", "[z-a]", "\\u0041", "\\u{41}", "[a-z&&[^m]]",
            "[a[bc]]", "[a-z--m]", "\\<", "\\b{start}", "(?P<>a)", "(?#c)a", "(?'n'a)",
            "(?P<n>a)(?P<n>b)", "(?P<1a>a)", "(*COMMIT)a",
        ]
        for pattern in patterns {
            XCTAssertEqual(rejection(pattern), "syntax", pattern)
        }
    }

    /// The runner reads a nested repeat by its outermost operator, a flag group
    /// only once it is complete, and reports parse errors before the
    /// reference extras it accepts (all checked against `faber run`).
    func testRunnerOrderingAndFlagRules() {
        let cases: [(String, String?)] = [
            ("a*?+", "possessive"),
            ("(a|b)*?+", "possessive"),
            ("a++?", "syntax"),
            ("a???", "syntax"),
            ("a++(", "syntax"),
            ("(?ii)a", "syntax"),
            ("(?i-i)a", "syntax"),
            ("(?i-)a", "syntax"),
            ("(?-)a", "syntax"),
            ("(?R", "syntax"),
            ("(?R)", "recursion"),
            ("(?u", "syntax"),
            ("(?P", "unsupported_flag"),
            ("\\k", "backreference"),
            ("\\kx", "backreference"),
            ("\\b{2}", nil),
            ("\\b{start}", "syntax"),
            ("[]]", nil),
            ("[^]]", nil),
            ("^*", nil),
            ("$+", nil),
            ("\\b+", nil),
            ("\\A+", nil),
        ]
        for (pattern, id) in cases {
            XCTAssertEqual(rejection(pattern), id, pattern)
        }
    }

    func testAssertionsTakeQuantifiersAndALeadingBracketIsALiteral() throws {
        XCTAssertTrue(Regex.matches(try regex("^*a"), "a"))
        XCTAssertTrue(Regex.matches(try regex("[]]"), "]"))
        XCTAssertFalse(Regex.matches(try regex("[^]]"), "]"))
        XCTAssertTrue(Regex.matches(try regex("[]a]+"), "a]"))
    }

    func testAcceptedConstructs() {
        let patterns = [
            "(?:a)b", "(?<n>a)b", "(?P<n>a)b", "[[:^alpha:]]", "\\x{1F600}", "\\x41", "a+?", "a{2,3}",
            "a{2,}", "()", "(|a)", "#", "$", "(?i)a(?-i)b", "(?ims)a", "(?x) a b # c", "(?s:.)",
            "\\pL", "\\p{Greek}", "[\\w-]", "[^\\W\\d]", "\\v", "\\bé",
        ]
        for pattern in patterns {
            XCTAssertNil(rejection(pattern), pattern)
        }
    }

    func testRegexPrintsAsItsPattern() throws {
        XCTAssertEqual("\(try regex("(?i)[a-z]+"))", "(?i)[a-z]+")
    }

    // MARK: matching

    func testMatchesIsASearch() throws {
        XCTAssertTrue(Regex.matches(try regex("a+"), "caab"))
        XCTAssertFalse(Regex.matches(try regex("a+"), "cb"))
    }

    func testDialectShorthands() throws {
        // `\w`, `\d`, `.` and POSIX names read the way the reference reads them.
        XCTAssertTrue(Regex.matches(try regex("^\\w+$"), "e\u{301}"))
        XCTAssertTrue(Regex.matches(try regex("^\\w$"), "\u{200D}"))
        XCTAssertFalse(Regex.matches(try regex("^\\d$"), "\u{B2}"))
        XCTAssertTrue(Regex.matches(try regex("^.$"), "\r"))
        XCTAssertFalse(Regex.matches(try regex("^.$"), "\n"))
        XCTAssertTrue(Regex.matches(try regex("(?s)^.$"), "\n"))
        XCTAssertFalse(Regex.matches(try regex("^[[:alpha:]]$"), "é"))
        XCTAssertTrue(Regex.matches(try regex("^\\pL$"), "é"))
        XCTAssertTrue(Regex.matches(try regex("^\\v$"), "\u{B}"))
        XCTAssertFalse(Regex.matches(try regex("^\\v$"), "\n"))
    }

    func testFindReportsCodePointOffsets() throws {
        let hit = try XCTUnwrap(Regex.find(try regex("[a-z]+"), "😀é abc"))
        XCTAssertEqual(Regex.matchText(hit), "abc")
        XCTAssertEqual(Regex.matchStart(hit), 3)
        XCTAssertEqual(Regex.matchEnd(hit), 6)
        XCTAssertNil(Regex.find(try regex("[0-9]"), "abc"))
    }

    func testGroupsAndNames() throws {
        let hit = try XCTUnwrap(
            Regex.find(try regex("(?P<key>[a-z]+)=(?P<val>[0-9]+)"), "x: size=42;"))
        XCTAssertEqual(Regex.matchGroup(hit, 0), "size=42")
        XCTAssertEqual(Regex.matchGroup(hit, 1), "size")
        XCTAssertEqual(Regex.matchGroup(hit, 2), "42")
        XCTAssertEqual(Regex.matchNamed(hit, "key"), "size")
        XCTAssertNil(Regex.matchNamed(hit, "nope"))
        XCTAssertNil(Regex.matchGroup(hit, 9))
        XCTAssertNil(Regex.matchGroup(hit, -1))

        let either = try XCTUnwrap(Regex.find(try regex("(a)|(b)"), "xb"))
        XCTAssertNil(Regex.matchGroup(either, 1))
        XCTAssertEqual(Regex.matchGroup(either, 2), "b")
    }

    func testEmptyMatchRuleSkipsAnEmptyMatchRightAfterAMatch() throws {
        let spans = Regex.findAll(try regex("a*"), "baaac").map {
            [Regex.matchStart($0), Regex.matchEnd($0)]
        }
        XCTAssertEqual(spans, [[0, 0], [1, 4], [5, 5]])
        let empty = Regex.findAll(try regex(""), "ab").map { Regex.matchStart($0) }
        XCTAssertEqual(empty, [0, 1, 2])
    }

    func testFindAllOffsetsAreAscendingCodePoints() throws {
        let hits = Regex.findAll(try regex("a"), "😀a😀a")
        XCTAssertEqual(hits.map { Regex.matchStart($0) }, [1, 3])
    }

    func testSplitKeepsEdgePiecesAndDoesNotSpliceGroups() throws {
        XCTAssertEqual(Regex.split(try regex(","), ",a,"), ["", "a", ""])
        XCTAssertEqual(Regex.split(try regex(","), "a,,b"), ["a", "", "b"])
        XCTAssertEqual(Regex.split(try regex("(,)"), "a,b"), ["a", "b"])
        XCTAssertEqual(Regex.split(try regex("a*"), "baaac"), ["", "b", "c", ""])
        XCTAssertEqual(Regex.split(try regex(""), "abc"), ["", "a", "b", "c", ""])
        XCTAssertEqual(Regex.split(try regex("x"), "abc"), ["abc"])
        XCTAssertEqual(Regex.split(try regex(","), ""), [""])
    }

    func testReplaceIsLiteralAndReplacesEveryMatch() throws {
        XCTAssertEqual(Regex.replace(try regex("a+"), "caab aa", "x"), "cxb x")
        XCTAssertEqual(Regex.replace(try regex("([a-z])([0-9])"), "a1 b2", "$1"), "$1 $1")
        XCTAssertEqual(Regex.replace(try regex("a*"), "baaac", "-"), "-b-c-")
        XCTAssertEqual(Regex.replace(try regex("[0-9]+"), "a12b3", ""), "ab")
    }

    func testReplaceEachRunsTheClosureOncePerMatchInOrder() throws {
        var seen: [String] = []
        let result = Regex.replaceEach(try regex("[a-z]+"), "ab 12 cd") { match in
            seen.append(Regex.matchText(match))
            return Regex.matchText(match).uppercased() + "$1"
        }
        XCTAssertEqual(seen, ["ab", "cd"])
        XCTAssertEqual(result, "AB$1 12 CD$1")
    }

    // MARK: escape

    func testEscapeMatchesTheInputLiterally() throws {
        XCTAssertEqual(Regex.escape("a.b"), "a\\.b")
        for text in ["a.b*c", "(x)|[y]", "^$#&-~{}+?\\", "é😀 plain"] {
            let pattern = "^" + Regex.escape(text) + "$"
            let hit = Regex.find(try regex(pattern), text)
            XCTAssertEqual(hit.map { Regex.matchText($0) }, text, text)
        }
    }
}
