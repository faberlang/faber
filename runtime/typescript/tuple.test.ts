import assert from "node:assert/strict";
import test from "node:test";
import { tuple } from "./index.ts";

test("constructor and set compare tuple keys by value and retain first identity", () => {
  const first: [string, number] = ["a", 1];
  const map = new tuple.TupleMap<[string, number], number>([[first, 3], [["a", 1], 4]]);
  assert.equal(map.size, 1);
  assert.equal(map.get(["a", 1]), 4);
  assert.equal(map.has(["a", 1]), true);
  assert.equal(map.keys().next().value, first);
  assert.equal(map.set(["a", 1], 5), map);
  assert.deepEqual(Array.from(map.values()), [5]);
  assert.deepEqual(Array.from(map), [[first, 5]]);
});

test("delete and clear reset canonical keys without affecting another map", () => {
  const first: [string, number] = ["a", 1];
  const map = new tuple.TupleMap<[string, number], number>([[first, 3]]);
  const other = new tuple.TupleMap<[string, number], number>([[first, 7]]);
  assert.equal(map.delete(["a", 1]), true);
  assert.equal(map.has(["a", 1]), false);
  assert.equal(map.delete(["a", 1]), false);
  const next: [string, number] = ["a", 1];
  map.set(next, 5);
  assert.equal(map.keys().next().value, next);
  map.clear();
  assert.equal(map.size, 0);
  assert.equal(map.get(["a", 1]), undefined);
  map.set(first, 8);
  assert.equal(map.keys().next().value, first);
  assert.equal(other.get(["a", 1]), 7);
});
