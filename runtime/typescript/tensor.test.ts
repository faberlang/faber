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

test("tensor checked method forms return the element or the runner's text error", () => {
  const grid = new tensor.Tensor([1, 2, 3, 4], [2, 2]);
  assert.deepEqual(grid.accipeChecked([1, 0]), { ok: true, value: 3 });
  assert.deepEqual(grid.accipeChecked([2, 0]), { ok: false, error: "tensor accipe invalid index" });
  assert.deepEqual(grid.pondeChecked([0, 1], 9), { ok: true, value: undefined });
  assert.deepEqual(grid.accipeChecked([0, 1]), { ok: true, value: 9 });
  assert.deepEqual(grid.pondeChecked([0, 5], 1), { ok: false, error: "tensor ponde invalid index" });
  assert.deepEqual(grid.planata(), [1, 9, 3, 4]);
});

test("tensor expande inserts, stretches and rejects like the Rust carrier", () => {
  const row = new tensor.Tensor([7, 9], [2]);
  const inserted = row.expande([2, 2]);
  assert.deepEqual(inserted.magnitudines(), [2, 2]);
  assert.deepEqual(inserted.planata(), [7, 9, 7, 9]);
  // A copy: later writes to the receiver are not visible through it.
  row.ponde([0], 70);
  assert.deepEqual(inserted.planata(), [7, 9, 7, 9]);

  const column = new tensor.Tensor([10, 20, 30], [3, 1]);
  assert.deepEqual(column.expande([3, 4]).planata(), [10, 10, 10, 10, 20, 20, 20, 20, 30, 30, 30, 30]);

  const three = new tensor.Tensor([2, 4, 6], [3]);
  assert.deepEqual(three.expande([2, 3]).planata(), [2, 4, 6, 2, 4, 6]);
  assert.deepEqual(three.expande([3]).planata(), [2, 4, 6]);

  assert.throws(() => three.expande([2, 2]), { message: "tensor broadcast shape mismatch" });
  assert.throws(() => three.expande([-1, 3]), { message: "tensor shape dimension must be non-negative" });
  assert.throws(() => three.expande([3, 1e300]), { message: "tensor element count overflow" });
});
