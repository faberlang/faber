// Run: node --test runtime/typescript/tensor.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";
import { tensor } from "./index.ts";

test("tensor constructors copy inputs and preserve reshape and fill semantics", () => {
  const data = [1, 2, 3, 4];
  const shape = [2, 2];
  const value = new tensor.Tensor(data, shape);
  data[0] = 99;
  shape[0] = 99;
  assert.deepEqual(value.planata(), [1, 2, 3, 4]);
  assert.deepEqual(value.magnitudines(), [2, 2]);
  assert.deepEqual(value.forma([4]).data, [1, 2, 3, 4]);
  assert.deepEqual(value.strue([4, 3, 2, 1], [2, 2]).data, [4, 3, 2, 1]);
  const copy = value.materialize();
  copy.reple(8);
  assert.deepEqual(copy.data, [8, 8, 8, 8]);
  assert.deepEqual(value.data, [1, 2, 3, 4]);
});

test("tensor arithmetic, broadcasting, matmul, transpose and softmax", () => {
  const left = new tensor.Tensor([1, 2, 3, 4], [2, 2]);
  const row = new tensor.Tensor([10, 20], [2]);
  assert.deepEqual(left.addita(row).data, [11, 22, 13, 24]);
  assert.deepEqual(left.subtrahe(row).data, [-9, -18, -7, -16]);
  assert.deepEqual(left.multiplica(row).data, [10, 40, 30, 80]);
  assert.deepEqual(left.matmul(left).data, [7, 10, 15, 22]);
  assert.deepEqual(left.transpone().data, [1, 3, 2, 4]);
  assert.equal(left.summa(), 10);
  assert.equal(left.media(), 2.5);
  const probabilities = left.activatio_softmax().data;
  assert.ok(Math.abs(probabilities[0] + probabilities[1] - 1) < 1e-12);
  assert.ok(Math.abs(probabilities[2] + probabilities[3] - 1) < 1e-12);
});

test("tensor conversion recovery, indexing, and original shape traps", () => {
  const value = tensor.Tensor.fromArray([[1, 2], [3, 4]], [2, 2], Number);
  assert.equal(value.accipe([1, 0]), 3);
  assert.equal(value.accipe([2, 0]), null);
  value.ponde([1, 0], 9);
  assert.deepEqual(value.sectio(1, 2).data, [9, 4]);
  const fallback = tensor.Tensor.empty<number>([2, 2]);
  assert.equal(tensor.Tensor.fromArray([], [2, 2], Number, fallback), fallback);
  assert.throws(() => value.forma([3]), { message: "tensor forma (reshape) element count mismatch" });
  assert.throws(() => value.strue([1], [2]), { message: "tensor structa element count does not match shape" });
  assert.throws(() => value.addita(new tensor.Tensor([1, 2, 3], [3])), { message: "tensor broadcast shape mismatch" });
});
