// Run: node --test runtime/typescript/dup.test.ts   (Node 24+ strips types)
import assert from "node:assert/strict";
import { test } from "node:test";
import { dup } from "./index.ts";

test("primitives pass through", () => {
  assert.equal(dup.duplicate(5), 5);
  assert.equal(dup.duplicate("x"), "x");
  assert.equal(dup.duplicate(null), null);
});

test("arrays are deep-copied", () => {
  const source = [1, [2, 3]];
  const copy = dup.duplicate(source);
  assert.notEqual(copy, source);
  assert.notEqual(copy[1], source[1]);
  assert.deepEqual(copy, source);
});

test("maps and sets are rebuilt", () => {
  const source = new Map([["k", { n: 1 }]]);
  const copy = dup.duplicate(source);
  assert.notEqual(copy, source);
  assert.notEqual(copy.get("k"), source.get("k"));
  assert.deepEqual([...copy.entries()], [...source.entries()]);
  const set = dup.duplicate(new Set([1, 2, 3]));
  assert.deepEqual([...set], [1, 2, 3]);
});

test("genus instances keep their prototype and methods", () => {
  class Genus {
    value = 1;
    double(): number {
      return this.value * 2;
    }
  }
  const copy = dup.duplicate(new Genus());
  assert.ok(copy instanceof Genus);
  assert.equal(copy.double(), 2);
});

test("cycles and shared references inside the graph are preserved", () => {
  const source: { self?: unknown; shared?: unknown } = {};
  source.self = source;
  const shared = { n: 1 };
  source.shared = [shared, shared];
  const copy = dup.duplicate(source);
  assert.equal(copy.self, copy);
  const pair = copy.shared as unknown[];
  assert.equal(pair[0], pair[1]);
  assert.notEqual(pair[0], shared);
});
