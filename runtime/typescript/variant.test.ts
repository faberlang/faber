import assert from "node:assert/strict";
import test from "node:test";
import { variant } from "./index.ts";

test("variant preserves identity, fields and non-enumerable display", () => {
  const fields = { tag: "Click", x: 4 };
  const value = variant.value(fields, () => "Event.Click { x = 4 }");
  assert.equal(value, fields);
  assert.equal(String(value), "Event.Click { x = 4 }");
  assert.deepEqual(Object.keys(value), ["tag", "x"]);
  assert.equal(JSON.stringify(value), '{"tag":"Click","x":4}');
  assert.deepEqual(Object.getOwnPropertyDescriptor(value, "toString"), {
    value: value.toString, writable: false, enumerable: false, configurable: false,
  });
});

test("colliding tag field keeps the authored discriminant", () => {
  const value = variant.value({ __tag: "V", tag: "payload" }, () => 'U.V { tag = "payload" }');
  assert.equal(value.__tag, "V");
  assert.equal(value.tag, "payload");
  assert.equal(String(value), 'U.V { tag = "payload" }');
});
