import assert from "node:assert/strict";
import test from "node:test";

import { Instans } from "./instans.ts";

test("parse preserves the requested precision and normalizes offsets", () => {
  const utc = Instans.parse("1979-05-27T07:32:00Z", "s");
  const offset = Instans.parse("1979-05-27T03:32:00-04:00", "s");
  assert.equal(utc.text(), "1979-05-27T07:32:00Z");
  assert.equal(utc.equals(offset), true);
});

test("fromEpoch retains subsecond precision", () => {
  assert.equal(Instans.fromEpoch(123456n, "ms").text(), "1970-01-01T00:02:03.456Z");
});
