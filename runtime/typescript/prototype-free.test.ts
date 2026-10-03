// Runtime loading must not install the retired numeric/list augmentations.
import assert from "node:assert/strict";
import { test } from "node:test";

test("loading runtime namespaces leaves builtin prototypes unchanged", async () => {
  const number = Object.getOwnPropertyDescriptors(Number.prototype);
  const array = Object.getOwnPropertyDescriptors(Array.prototype);
  const { exact } = await import("./index.ts");
  assert.equal(exact.pow(2, 10), 1024);
  assert.deepEqual(Object.getOwnPropertyDescriptors(Number.prototype), number);
  assert.deepEqual(Object.getOwnPropertyDescriptors(Array.prototype), array);
  assert.ok(!Object.hasOwn(Number.prototype, "potentia"));
  for (const name of ["all", "any", "omnia", "quilibet"]) {
    assert.ok(!Object.hasOwn(Array.prototype, name));
  }
});
