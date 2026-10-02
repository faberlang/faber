// Run: node --test runtime/typescript/format.test.ts   (Node 24+ strips types)
//
// The rows pin the former inline helpers' results, including the `¶` corpus
// exemplar's renderings.
import assert from "node:assert/strict";
import { test } from "node:test";
import { format } from "./index.ts";

function spec(over: Partial<format.Spec> = {}): format.Spec {
  return { fill: " ", align: ">", plus: false, zero: false, width: 0, precision: -1, kind: "", ...over };
}

test("pad aligns right, left and center, and counts code points", () => {
  assert.equal(format.pad("", "ab", spec({ width: 5 }), false), "   ab");
  assert.equal(format.pad("", "ab", spec({ width: 5, align: "<" }), false), "ab   ");
  assert.equal(format.pad("", "ab", spec({ width: 5, align: "^" }), false), " ab  ");
  assert.equal(format.pad("", "é😀", spec({ width: 4 }), false), "  é😀");
  assert.equal(format.pad("-", "7", spec({ width: 4 }), true), "-007");
  assert.equal(format.pad("", "wide", spec({ width: 2 }), false), "wide");
});

test("int prints sign, radix kinds, precision and zero fill", () => {
  assert.equal(format.int(42, spec({ width: 5 })), "   42");
  assert.equal(format.int(-42, spec({ width: 6, zero: true })), "-00042");
  assert.equal(format.int(42, spec({ plus: true })), "+42");
  assert.equal(format.int(255, spec({ kind: "x" })), "ff");
  assert.equal(format.int(-5, spec({ kind: "b" })), "-101");
  assert.equal(format.int(8, spec({ kind: "o" })), "10");
  assert.equal(format.int(1500, spec({ kind: "e", precision: 1 })), "1.5e3");
  assert.equal(format.int(3, spec({ precision: 2 })), "3.00");
  assert.equal(format.int(2n ** 70n, spec()), "1180591620717411303424");
});

test("float prints non-finite values, precision, scientific and the default body", () => {
  assert.equal(format.float(NaN, spec({ width: 5 })), "  NaN");
  assert.equal(format.float(Infinity, spec()), "∞");
  assert.equal(format.float(-Infinity, spec()), "-∞");
  assert.equal(format.float(-0, spec()), "-0.0");
  assert.equal(format.float(1.5, spec()), "1.5");
  assert.equal(format.float(2, spec()), "2.0");
  assert.equal(format.float(3.14159, spec({ precision: 2 })), "3.14");
  assert.equal(format.float(1500, spec({ kind: "e" })), "1.5e3");
  assert.equal(format.float(1.5, spec({ plus: true, width: 6, zero: true })), "+001.5");
});

test("text pads and truncates by code points", () => {
  assert.equal(format.text("abc", spec({ width: 5, align: "<" })), "abc  ");
  assert.equal(format.text("héllo", spec({ precision: 2 })), "hé");
  assert.equal(format.text("😀😀😀", spec({ precision: 2 })), "😀😀");
});

test("instans cuts an ISO text to its date or time preset", () => {
  const iso = "2026-10-02T14:28:25.123Z";
  assert.equal(format.instans(iso, "i"), iso);
  assert.equal(format.instans(iso, "d"), "2026-10-02");
  assert.equal(format.instans(iso, "t"), "14:28:25");
  assert.equal(format.instans("2026-10-02", "d"), "2026-10-02");
  assert.equal(format.instans("2026-10-02", "t"), "");
});
