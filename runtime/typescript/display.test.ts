// Run: node --test runtime/typescript/display.test.ts   (Node 24+ strips types)
//
// Every row is the text the Faber MIR runner prints for the same f64 value
// (`faber run` on `print <float>`), recorded 2026-10-01 against radix main.
import assert from "node:assert/strict";
import { test } from "node:test";
import { display } from "./index.ts";

const ROWS: Array<[number, string]> = [
  // A float with a zero fraction prints its exact decimal expansion plus `.0`.
  [18446744073709551616, "18446744073709551616.0"],
  [123456789012345680000, "123456789012345683968.0"],
  [9007199254740993, "9007199254740992.0"],
  [1e21, "1000000000000000000000.0"],
  [-1e21, "-1000000000000000000000.0"],
  [5, "5.0"],
  [0, "0.0"],
  [-0, "-0.0"],
  // Other finite values print shortest round-trip digits, never an exponent.
  [1.5, "1.5"],
  [0.3, "0.3"],
  [1e-7, "0.0000001"],
  [1.5e-10, "0.00000000015"],
  [-2.5e-9, "-0.0000000025"],
  // Non-finite values print as the runner prints them.
  [Infinity, "inf"],
  [-Infinity, "-inf"],
  [NaN, "NaN"],
];

test("fractus display matches the MIR runner", () => {
  for (const [value, expected] of ROWS) {
    assert.equal(display.fractus(value), expected, `value ${String(value)}`);
    assert.equal(display.value(value, "fractus"), expected);
  }
});

test("exact expansion of the largest finite float and the smallest subnormal", () => {
  const max = display.fractus(Number.MAX_VALUE);
  assert.match(max, /^17976931348623157\d{292}\.0$/);
  assert.equal(max.length, 309 + 2);
  const tiny = display.fractus(5e-324);
  assert.equal(tiny.length, 2 + 323 + 1);
  assert.match(tiny, /^0\.0{323}5$/);
});

test("fractus elements render inside a list", () => {
  assert.equal(
    display.value([1, 1e21, 2.5, Infinity], { kind: "lista", element: "fractus" }),
    "[1.0, 1000000000000000000000.0, 2.5, inf]",
  );
});

test("a bare number in a valor is an integer unless it can only be a float", () => {
  assert.equal(display.value(42, "valor"), "42");
  assert.equal(display.value(1.5, "valor"), "1.5");
  assert.equal(display.value(1e-7, "valor"), "0.0000001");
  assert.equal(display.value(Infinity, "valor"), "inf");
  assert.equal(display.value(1e21, "valor"), "1000000000000000000000.0");
});

test("a tagged Fractus valor uses the fractus display", () => {
  const boxed = { __faberValorTag: "Fractus", __faberValorPayload: 18446744073709551616 };
  assert.equal(display.value(boxed, "valor"), "18446744073709551616.0");
});

test("a numerus beyond 2^53 displays its exact integer", () => {
  assert.equal(display.value(-9223372036854775808, "numerus"), "-9223372036854775808");
  assert.equal(display.value(18446744073709551616, "numerus"), "18446744073709551616");
  assert.equal(display.value(9007199254740991, "numerus"), "9007199254740991");
  assert.equal(display.value(-9007199254740991, "numerus"), "-9007199254740991");
  assert.equal(display.value(42, "numerus"), "42");
  assert.equal(display.value(-0, "numerus"), "0");
});

test("english tokens print true/false/none through every display renderer", () => {
  const english = { true_: "true", false_: "false", none: "none" };
  assert.equal(display.value(true, "bivalens", english), "true");
  assert.equal(display.value(false, "bivalens", english), "false");
  assert.equal(display.value(null, "unknown", english), "none");
  assert.equal(display.value(null, { kind: "nullable", inner: "numerus" }, english), "none");
  assert.equal(display.value(7, { kind: "nullable", inner: "numerus" }, english), "7");
  assert.equal(display.value([true, false], { kind: "lista", element: "bivalens" }, english), "[true, false]");
  assert.equal(
    display.value(new Map([["k", null]]), { kind: "map", key: "textus", value: { kind: "nullable", inner: "numerus" } }, english),
    '{"k": none}',
  );
  assert.equal(display.valor([true, null], english), "[true, none]");
  assert.equal(display.taggedValor({ __faberValorTag: "Bivalens", __faberValorPayload: false }, english), "false");
});

test("the Latin default is unchanged when no tokens are passed", () => {
  assert.equal(display.value(true, "bivalens"), "verum");
  assert.equal(display.value(false, "bivalens"), "falsum");
  assert.equal(display.value(null), "nihil");
  assert.equal(display.value([true, null], { kind: "lista", element: "valor" }), "[verum, nihil]");
});
