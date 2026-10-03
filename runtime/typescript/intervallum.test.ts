import assert from "node:assert/strict";
import test from "node:test";

import { Intervallum } from "./intervallum.ts";

test("iteration preserves inclusive bounds and stride", () => {
  assert.deepEqual([...new Intervallum(1, 5, true, 2)], [1, 3, 5]);
});

test("coerce clamps to the requested interval", () => {
  const range = new Intervallum(2, 6, true);
  assert.equal(Intervallum.coerce(9, range), 6);
  assert.deepEqual(Intervallum.coerce(new Intervallum(1, 4, true), range), new Intervallum(2, 4, true));
});
