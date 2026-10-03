// Run: node --test runtime/typescript/order.test.ts   (Node 24+ strips types)
import assert from "node:assert/strict";
import { test } from "node:test";
import { order } from "./index.ts";

test("textCompare orders by Unicode code point, not UTF-16 code unit", () => {
  // U+10000 (surrogate pair D800 DC00) sorts above U+E000 by code point but
  // below it by code unit; the helper must agree with the former.
  assert.ok(order.textCompare("\u{10000}", "\uE000") > 0);
  assert.ok(order.textCompare("a", "b") < 0);
  assert.equal(order.textCompare("ab", "ab"), 0);
  assert.ok(order.textCompare("a", "ab") < 0);
});

test("compare orders numbers by IEEE 754 totalOrder", () => {
  assert.equal(order.compare(-0, 0), -1);
  assert.equal(order.compare(0, -0), 1);
  assert.equal(order.compare(NaN, 5), 1);
  assert.equal(order.compare(5, NaN), -1);
  assert.equal(order.compare(NaN, NaN), 0);
  assert.ok(order.compare(2, 10) < 0);
  assert.ok(order.compare(7n, 3n) > 0);
});

test("compare orders text by code point, tuples lexicographically", () => {
  assert.equal(order.compare("\u{10000}", "\uE000"), 1);
  assert.equal(order.compare([1, 2], [1, 3]), -1);
  assert.equal(order.compare([1, 2], [1, 2]), 0);
});

test("compare uses Date time and a genus's own compare", () => {
  assert.equal(order.compare(new Date(1), new Date(2)), -1);
  assert.equal(order.compare({ compare: () => 1 }, {}), 1);
});
