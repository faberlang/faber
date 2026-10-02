// Run: node --test runtime/typescript/regex.test.ts   (Node 24+ strips types)
//      bun test runtime/typescript/regex.test.ts
//
// Every row is the value the Faber MIR runner (the Rust `regex` crate 1.13.1,
// the dialect's reference engine) gives for the same pattern and text, taken
// from `docs/design/regex-dialect.md` (pins and Appendix B) and the corpus
// cells `corpus/regex/*.fab`. The two runtimes (Node, Bun) must agree on all
// of them.
import assert from "node:assert/strict";
import { test } from "node:test";
import { Match, Regex, RegexError, escape } from "./regex.ts";

function compile(pattern: string): Regex {
  return Regex.from(pattern);
}

function idOf(pattern: string): string {
  try {
    Regex.from(pattern);
  } catch (error) {
    assert.ok(error instanceof RegexError, "a rejection is a RegexError");
    assert.ok(
      error.message.startsWith(error.id + ":"),
      "the message starts with the construct id",
    );
    return error.id;
  }
  return "accepted";
}

function spans(pattern: string, text: string): string {
  return compile(pattern)
    .findAll(text)
    .map((m) => m.start() + "-" + m.end())
    .join(" ");
}

function u(...cps: number[]): string {
  return String.fromCodePoint(...cps);
}

// ── the stable construct ids (RD-4) ─────────────────────────────────────────

test("rejected constructs carry their stable id", () => {
  const rows: Array<[string, string]> = [
    ["a(?=b)", "lookahead"],
    ["a(?!b)", "lookahead"],
    ["(?<=a)b", "lookbehind"],
    ["(?<!a)b", "lookbehind"],
    ["(a)\\1", "backreference"],
    ["(?P<n>a)\\k<n>", "backreference"],
    ["(?P<n>a)(?P=n)", "backreference"],
    ["\\0", "backreference"],
    ["\\012", "backreference"],
    ["(?>a)", "atomic_group"],
    ["a++", "possessive"],
    ["a*+", "possessive"],
    ["a?+", "possessive"],
    ["a{2}+", "possessive"],
    ["(?R)", "recursion"],
    ["(a)(?1)", "recursion"],
    ["(?&n)", "recursion"],
    ["(?P>n)", "recursion"],
    ["(?(1)a|b)", "conditional"],
    ["(?g)a", "unsupported_flag"],
    ["(?U)a+", "unsupported_flag"],
    ["(?u)a", "unsupported_flag"],
    ["(?-u)a", "unsupported_flag"],
    ["(?y)a", "unsupported_flag"],
    ["(?'n'a)", "syntax"],
    ["(?P<n>a)(?P<n>b)", "syntax"],
    ["(?P<1a>a)", "syntax"],
    ["(?P<>a)", "syntax"],
    ["(?#c)a", "syntax"],
    ["(", "syntax"],
    ["a)", "syntax"],
    ["[", "syntax"],
    ["[]", "syntax"],
    ["[z-a]", "syntax"],
    ["a{2,1}", "syntax"],
    ["*a", "syntax"],
    ["a**", "syntax"],
    ["\\", "syntax"],
    ["\\Z", "syntax"],
    ["\\G", "syntax"],
    ["\\Qa.b\\E", "syntax"],
    ["\\h", "syntax"],
    ["\\R", "syntax"],
    ["\\K", "syntax"],
    ["\\p{Foo}", "syntax"],
    ["a{100000}", "syntax"],
    ["\\u0041", "syntax"],
    ["\\u{41}", "syntax"],
    ["[a-z&&[^m]]", "syntax"],
    ["[a[bc]]", "syntax"],
    ["[a-z--m]", "syntax"],
    ["\\<", "syntax"],
    ["\\b{start}", "syntax"],
    ["\\d\\", "syntax"],
  ];
  for (const [pattern, id] of rows) {
    assert.equal(idOf(pattern), id, pattern);
  }
});

test("accepted constructs", () => {
  for (const pattern of [
    "",
    "(?:a)b",
    "(?<n>a)b",
    "(?P<n>a)b",
    "(?i)a",
    "(?ims)a",
    "(?x:a b)",
    "[[:^alpha:]]",
    "a{1000}",
    "a{2,}",
    "a*?",
    "\\x41",
    "\\x{1F600}",
    "\\.",
    "[\\]]",
    "[]a]",
    "[^]a]",
    "\\p{Greek}",
    "\\pL",
    "\\p{^Lu}",
    "\\bé",
    "^*",
  ]) {
    assert.equal(idOf(pattern), "accepted", pattern);
  }
});

// ── Unicode classes, boundaries, folding ────────────────────────────────────

test("\\w is the reference engine's Unicode word class", () => {
  const w = compile("^\\w$");
  for (const cp of [0x5f, 0xe9, 0x1d400, 0x301, 0x200d, 0x203f, 0x2167, 0x663]) {
    assert.ok(w.matches(u(cp)), "word: " + cp.toString(16));
  }
  assert.ok(!w.matches("²"));
  assert.ok(!w.matches(" "));
  assert.ok(compile("^\\W$").matches("²"));
});

test("\\d and \\s are the reference engine's Unicode classes", () => {
  const d = compile("^\\d$");
  for (const cp of [0x663, 0x1d7d8, 0xff13]) {
    assert.ok(d.matches(u(cp)), "digit: " + cp.toString(16));
  }
  for (const cp of [0xb2, 0x2167]) {
    assert.ok(!d.matches(u(cp)), "not digit: " + cp.toString(16));
  }
  const s = compile("^\\s$");
  for (const cp of [0x0b, 0x85, 0xa0, 0x2003, 0x2028, 0x3000]) {
    assert.ok(s.matches(u(cp)), "space: " + cp.toString(16));
  }
  for (const cp of [0x1c, 0x180e, 0x200b, 0xfeff]) {
    assert.ok(!s.matches(u(cp)), "not space: " + cp.toString(16));
  }
});

test("word boundaries are Unicode", () => {
  assert.ok(compile("\\bé").matches("é"));
  assert.ok(!compile("a\\b").matches("aé"));
  assert.ok(compile("a\\Bé").matches("aé"));
  assert.ok(!compile("\\b").matches(""));
  assert.ok(compile("\\B").matches(""));
  assert.ok(!compile("e\\b").matches("e" + u(0x301)));
  assert.equal(spans("\\b", "hé x"), "0-0 2-2 3-3 4-4");
  // a boundary inside a surrogate pair is no position (the engines report one)
  assert.equal(spans("\\B", "é😀a_A"), "3-3 4-4");
  assert.equal(spans("\\b", "é😀a_A"), "0-0 1-1 2-2 5-5");
  assert.ok(!compile("\\b{1,3}[ ,]").matches(""));
});

test("classes", () => {
  assert.ok(!compile("[[:alpha:]]").matches("é"));
  assert.ok(compile("[[:alpha:]]").matches("e"));
  assert.ok(compile("\\p{L}").matches("é"));
  assert.ok(compile("\\pL").matches("é"));
  assert.ok(compile("\\p{Greek}").matches("α"));
  assert.ok(!compile("\\p{Greek}").matches("a"));
  assert.ok(compile("\\p{Lu}").matches("É"));
  assert.ok(!compile("\\p{Lu}").matches("é"));
  assert.ok(compile("^[\\w-]+$").matches("é-_"));
  assert.ok(!compile("[^\\W\\d]").matches("3"));
  assert.ok(compile("[^\\W\\d]").matches("é"));
  assert.ok(!compile("[^\\W\\d]").matches("-"));
  assert.ok(compile("^[\\x41-\\x{5A}]$").matches("Q"));
});

test("case folding is Unicode simple folding", () => {
  assert.ok(compile("(?i)σ").matches("ς"));
  assert.ok(compile("(?i)σ").matches("Σ"));
  assert.ok(compile("(?i)ß").matches("ẞ"));
  assert.ok(compile("(?i)k").matches(u(0x212a)));
  assert.ok(compile("(?i)ǆ").matches("ǅ"));
  assert.ok(!compile("(?i)ß").matches("SS"));
  assert.ok(!compile("(?i)i").matches("İ"));
  // a class range is closed over the three-member groups on both runtimes
  assert.ok(compile("(?i)^[a-z]+$").matches(u(0x17f)));
  assert.ok(compile("(?i)^[a-z]+$").matches(u(0x212a)));
  assert.ok(!compile("(?i)^[^a-z]$").matches(u(0x17f)));
});

test("flags scope to their group", () => {
  assert.ok(compile("(?i)a(?-i)b").matches("Ab"));
  assert.ok(!compile("(?i)a(?-i)b").matches("AB"));
  assert.ok(compile("(?i:a)b").matches("Ab"));
  assert.ok(!compile("(?i:a)b").matches("AB"));
  assert.ok(!compile("(a(?i)b)c").matches("aBC"));
  assert.ok(compile("(a(?i)b)c").matches("aBc"));
  assert.ok(compile("(?x) a b # note\n c").matches("abc"));
  assert.ok(!compile("(?x)[a ]b").matches(" b"));
  assert.ok(compile("(?x)a\\ b").matches("a b"));
});

test("dot and anchors break lines at U+000A only", () => {
  assert.ok(!compile("a.b").matches("a\nb"));
  assert.ok(compile("(?s)a.b").matches("a\nb"));
  assert.ok(compile("a.b").matches("a\rb"));
  assert.ok(compile("a.b").matches("a" + u(0x2028) + "b"));
  assert.ok(compile("^.$").matches("😀"));
  assert.ok(!compile("^.$").matches("e" + u(0x301)));
  assert.ok(!compile("a$").matches("a\n"));
  assert.ok(!compile("a\\z").matches("a\n"));
  assert.ok(compile("\\Aa").matches("a"));
  assert.ok(compile("(?m)\\Ab").matches("b"));
  assert.ok(!compile("(?m)\\Ab").matches("a\nb"));
  assert.ok(compile("(?m)^b").matches("a\nb"));
  assert.ok(!compile("(?m)a$").matches("a\r\nb"));
  assert.ok(!compile("(?m)^b").matches("a\rb"));
  assert.equal(spans("(?m)^", "a\nb\n"), "0-0 2-2 4-4");
});

// ── groups ──────────────────────────────────────────────────────────────────

test("groups and names", () => {
  const named = compile("(?P<key>[a-z]+)=([0-9]+)?");
  const all = named.findAll("a=1 b= c=3");
  assert.deepEqual(
    all.map((m) => [m.named("key"), m.group(2)]),
    [
      ["a", "1"],
      ["b", null],
      ["c", "3"],
    ],
  );
  const angle = compile("(?<n>a)(b)?").find("a") as Match;
  assert.equal(angle.named("n"), "a");
  assert.equal(angle.group(2), null);
  assert.equal(angle.named("missing"), null);
  assert.equal(angle.group(9), null);
  assert.equal(angle.group(0), "a");
  const leftmost = compile("(a|ab)(c|bcd)(d*)").find("abcd") as Match;
  assert.deepEqual([0, 1, 2, 3].map((i) => leftmost.group(i)), ["abcd", "a", "bcd", ""]);
  assert.equal(compile("(?:a)(b)").find("ab")?.group(1), "b");
});

// ── iteration, offsets, bulk operations ─────────────────────────────────────

test("an empty match that starts where the previous match ended is skipped", () => {
  assert.equal(spans("a*", "baaac"), "0-0 1-4 5-5");
  assert.equal(spans("a*", "aa"), "0-2");
  assert.equal(spans("a|", "aa"), "0-1 1-2");
  assert.equal(spans("|a", "aa"), "0-0 1-1 2-2");
  assert.equal(spans("", "ab"), "0-0 1-1 2-2");
  assert.equal(spans("^a", "aa"), "0-1");
  assert.equal(spans("\\Aa", "aa"), "0-1");
  assert.equal(spans("a*", "😀"), "0-0 1-1");
});

test("offsets are code points", () => {
  assert.equal((compile("a").find("😀a") as Match).start(), 1);
  assert.equal((compile("a").find("😀a") as Match).end(), 2);
  assert.equal(spans("[a-z]+", "😀é abc d"), "3-6 7-8");
  assert.equal(compile("[a-z]+").findAll("😀é abc d")[0].text(), "abc");
  assert.equal(compile("[a-z]").find("zzz")?.start(), 0);
});

test("find returns the leftmost-first match or none", () => {
  assert.equal(compile("\\d+").find("ab 12 345")?.text(), "12");
  assert.equal(compile("\\d+").find("none"), null);
  assert.ok(compile("\\d+").matches("a1"));
  assert.ok(!compile("\\d+").matches("a"));
});

test("split keeps leading and trailing empties and skips captures", () => {
  assert.deepEqual(compile(",").split("a,,b,"), ["a", "", "b", ""]);
  assert.deepEqual(compile("").split("abc"), ["", "a", "b", "c", ""]);
  assert.deepEqual(compile(",").split(""), [""]);
  assert.deepEqual(compile("(,)").split("a,b"), ["a", "b"]);
  assert.deepEqual(compile("a*").split("baaac"), ["", "b", "c", ""]);
  assert.deepEqual(compile("x").split("abc"), ["abc"]);
  assert.deepEqual(compile(",").split("😀,é"), ["😀", "é"]);
});

test("replace is literal and left to right", () => {
  assert.equal(compile("a*").replace("baaac", "-"), "-b-c-");
  assert.equal(compile("(a)").replace("xax", "$1"), "x$1x");
  assert.equal(compile("(?P<n>a)").replace("xax", "${n}\\1"), "x${n}\\1x");
  assert.equal(compile("x").replace("abc", "-"), "abc");
  assert.equal(compile("\\d+").replace("a1b22", (m) => "<" + m.text() + "@" + m.start() + ">"), "a<1@1>b<22@3>");
  // a result is never searched again
  assert.equal(compile("a").replace("aa", () => "a$0a"), "a$0aa$0a");
});

test("a replace callback may use the same pattern", () => {
  const word = compile("[a-z]+");
  const out = word.replace("ab cd", (m) => word.findAll(m.text() + " x").length + ":" + m.text());
  assert.equal(out, "2:ab 2:cd");
});

test("escape matches the text literally", () => {
  const probes = ["a.b", "1+1=2", "(x|y)[z]{2}", "^$#&-~\\", "mixed é 😀 text", " spaced "];
  for (const probe of probes) {
    const exact = compile("^" + escape(probe) + "$");
    assert.ok(exact.matches(probe), probe);
    assert.ok(!exact.matches(probe + "x"), probe);
  }
  assert.equal(escape("a.b"), "a\\.b");
  assert.equal(escape("é 😀"), "é 😀");
});

test("the source is kept for display", () => {
  assert.equal(compile("(?i)[a-z]+").toString(), "(?i)[a-z]+");
  assert.equal(compile("\\d+").source, "\\d+");
});

// ── cost ────────────────────────────────────────────────────────────────────

function megabyteTime(bytes: number): number {
  let text = "ab, ";
  while (text.length < bytes) {
    text += text;
  }
  const word = compile("[a-z]+");
  const start = performance.now();
  const found = word.findAll(text);
  const elapsed = performance.now() - start;
  assert.equal(found.length, text.length / 4);
  assert.equal(found[found.length - 1].end(), text.length - 2);
  return elapsed;
}

test("find_all is linear in the text", () => {
  megabyteTime(1 << 16); // warm up the engine and the allocator
  const one = megabyteTime(1 << 20);
  const two = megabyteTime(1 << 21);
  assert.ok(two <= 3 * one + 50, "2 MB took " + two + " ms against " + one + " ms for 1 MB");
});

test("compiling is once per conversion, matching reuses the engine", () => {
  const word = compile("(?i)([a-z]+)[0-9]{2,}(foo|bar|baz)+\\b");
  const start = performance.now();
  let hits = 0;
  for (let i = 0; i < 100000; i++) {
    if (word.matches("abc1234foobar")) {
      hits++;
    }
  }
  assert.equal(hits, 100000);
  assert.ok(performance.now() - start < 5000);
});
