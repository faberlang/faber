// Run: node --test runtime/typescript/sparsa.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";
import { sparsa, tensor } from "./index.ts";

test("sparse conversion round-trips and removes default-valued entries", () => {
  const dense = new tensor.Tensor([1, 0, 0, 2], [2, 2]);
  const value = sparsa.Sparsa.fromTensor(dense);
  assert.equal(value.nonnihil(), 2);
  assert.deepEqual(value.densata().data, dense.data);
  assert.equal(value.accipe([0, 1]), 0);
  value.ponde([0, 1], 3);
  assert.equal(value.nonnihil(), 3);
  value.ponde([0, 1], 0);
  assert.equal(value.nonnihil(), 2);
  assert.deepEqual(value.magnitudines(), [2, 2]);
});

test("sparse constructors copy inputs and preserve original traps", () => {
  const shape = [2];
  const entries = new Map([["[0]", 4]]);
  const value = new sparsa.Sparsa(shape, entries);
  shape[0] = 9;
  entries.set("[0]", 9);
  assert.equal(value.accipe([0]), 4);
  assert.deepEqual(value.shape, [2]);
  assert.throws(() => new sparsa.Sparsa([-1]), { message: "sparsa shape dimension must be non-negative" });
  assert.throws(() => value.accipe([2]), { message: "sparsa index out of bounds" });
});
