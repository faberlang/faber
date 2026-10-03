// Run: node --test runtime/typescript/matrix.test.ts
import assert from "node:assert/strict";
import { test } from "node:test";
import { matrix } from "./index.ts";

test("matrix arithmetic, indexing, application and iteration", () => {
  const left = new matrix.Matrix([1, 2, 3, 4], [2, 2]);
  const right = new matrix.Matrix([4, 3, 2, 1], [2, 2]);
  assert.deepEqual(left.addita(right).data, [5, 5, 5, 5]);
  assert.deepEqual(left.subtrahe(right).data, [-3, -1, 1, 3]);
  assert.deepEqual(left.applica([10, 20]), [50, 110]);
  assert.equal(left.accipe([1, 0]), 3);
  assert.deepEqual([...left], [1, 2, 3, 4]);
});

test("matrix preserves copies and shape/index trap strings", () => {
  const data = [1, 2];
  const shape = [1, 2];
  const value = new matrix.Matrix(data, shape);
  data[0] = 9;
  shape[0] = 9;
  assert.deepEqual(value.data, [1, 2]);
  assert.deepEqual(value.shape, [1, 2]);
  const wrong = new matrix.Matrix([1], [1]);
  assert.throws(() => value.addita(wrong), { message: "matrix addita shape mismatch" });
  assert.throws(() => value.subtrahe(wrong), { message: "matrix subtrahe shape mismatch" });
  assert.throws(() => value.accipe([2, 0]), { message: "matrix accipe invalid index" });
  assert.throws(() => value.applica([1]), { message: "matrix applica shape mismatch" });
});
