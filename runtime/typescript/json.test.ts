import assert from "node:assert/strict";
import test from "node:test";
import { json } from "./index.ts";

test("ordinary JSON codecs keep native values and failures", () => {
  const value: json.Value = { wire_name: "Ada", items: [1, { ok: true }] };
  const wire = json.stringify(value);
  assert.equal(wire, '{"wire_name":"Ada","items":[1,{"ok":true}]}');
  assert.deepEqual(json.parse(wire), value);
  assert.throws(() => json.parse("{"), SyntaxError);
});

test("inf JSON preserves integer tokens and emits bare numbers", () => {
  const wire = '{"big":18446744073709551616,"negative":-18446744073709551617,"safe":9007199254740991,"float":1.5,"exp":1e3,"text":"9007199254740993"}';
  const value = json.parseBigInt(wire);
  assert.equal(value.big, 18446744073709551616n);
  assert.equal(value.negative, -18446744073709551617n);
  assert.equal(value.safe, 9007199254740991);
  assert.equal(value.float, 1.5);
  assert.equal(value.exp, 1000);
  assert.equal(value.text, "9007199254740993");
  assert.equal(json.stringifyBigInt([value.big, value.negative]), "[18446744073709551616,-18446744073709551617]");
  assert.throws(() => json.parseBigInt("{"), SyntaxError);
});

test("thousand-digit JSON integers round trip exactly", () => {
  const digits = "9" + "7".repeat(999);
  const wire = `{"n":${digits},"xs":[${digits},-${digits}]}`;
  assert.equal(json.stringifyBigInt(json.parseBigInt(wire)), wire);
});
