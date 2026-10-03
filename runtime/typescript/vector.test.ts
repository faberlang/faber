// Run: node --test runtime/typescript/vector.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";
import { vector } from "./index.ts";

test("vector normalization returns a copy with unit length", () => {
  const input = [3, 4];
  assert.deepEqual(vector.normalize(input), [0.6, 0.8]);
  assert.deepEqual(input, [3, 4]);
});

test("vector zero and near-zero threshold remain unchanged", () => {
  assert.deepEqual(vector.normalize([0, 0]), [0, 0]);
  assert.deepEqual(vector.normalize([0.0000005]), [0]);
  assert.deepEqual(vector.normalize([0.000001]), [0]);
  assert.deepEqual(vector.normalize([]), []);
  assert.equal(Object.hasOwn(Array.prototype, "normalizata"), false);
});
