// Run: node --test runtime/typescript/inf.test.ts   (Node 24+ strips types)
//
// Every trap text is the text the Faber MIR runner reports for the same
// failure; the rows pin the former inline helpers' results and messages.
import assert from "node:assert/strict";
import { test } from "node:test";
import { inf } from "./index.ts";

const SAFE = 9007199254740991n;

function trapText(run: () => unknown): string {
  try {
    run();
  } catch (error) {
    assert.ok(error instanceof Error, "a trap is an Error");
    return error.message;
  }
  return "no trap";
}

test("div and mod floor toward negative infinity", () => {
  assert.equal(inf.div(7n, 2n), 3n);
  assert.equal(inf.div(-7n, 2n), -4n);
  assert.equal(inf.div(7n, -2n), -4n);
  assert.equal(inf.div(-7n, -2n), 3n);
  assert.equal(inf.mod(7n, 2n), 1n);
  assert.equal(inf.mod(-7n, 2n), 1n);
  assert.equal(inf.mod(7n, -2n), -1n);
  assert.equal(inf.mod(-7n, -2n), -1n);
});

test("div and mod trap on a zero divisor", () => {
  assert.equal(trapText(() => inf.div(1n, 0n)), "numerus division failed");
  assert.equal(trapText(() => inf.mod(1n, 0n)), "numerus division failed");
  assert.equal(trapText(() => inf.trueDiv(1n, 0n)), "numerus division failed");
});

test("pow is exact and traps on a negative exponent", () => {
  assert.equal(inf.pow(2n, 200n), 1606938044258990275541962092341162602522202993782792835301376n);
  assert.equal(trapText(() => inf.pow(2n, -1n)), "numerus potentia failed: negative exponent");
});

test("shl and shr are exact and trap on a negative count", () => {
  assert.equal(inf.shl(1n, 100n), 1267650600228229401496703205376n);
  assert.equal(inf.shr(-5n, 1n), -3n);
  assert.equal(trapText(() => inf.shl(1n, -1n)), "negative shift count");
  assert.equal(trapText(() => inf.shr(1n, -1n)), "negative shift count");
});

test("abs and trueDiv", () => {
  assert.equal(inf.abs(-9n), 9n);
  assert.equal(inf.abs(9n), 9n);
  assert.equal(inf.trueDiv(7n, 2n), 3.5);
});

test("approx is the exact relative 1e-9 rule", () => {
  assert.equal(inf.approx(1000000000n, 1000000001n), true);
  assert.equal(inf.approx(1000000000n, 1000000002n), false);
  assert.equal(inf.approx(0n, 0n), true);
  assert.equal(inf.approx(0n, 1n), false);
});

test("cmpFloat orders exactly and NaN orders nothing", () => {
  assert.equal(inf.cmpFloat(1n, 1.5), -1);
  assert.equal(inf.cmpFloat(2n, 1.5), 1);
  assert.equal(inf.cmpFloat(1n, 1), 0);
  assert.equal(inf.cmpFloat(-1n, -1.5), 1);
  assert.equal(inf.cmpFloat(5n, Infinity), -1);
  assert.equal(inf.cmpFloat(5n, -Infinity), 1);
  assert.ok(Number.isNaN(inf.cmpFloat(5n, NaN)));
});

test("toNumber traps past the safe-integer range", () => {
  assert.equal(inf.toNumber(SAFE), 9007199254740991);
  assert.equal(inf.toNumber(-SAFE), -9007199254740991);
  assert.equal(trapText(() => inf.toNumber(SAFE + 1n)), "numerus overflow");
  assert.equal(trapText(() => inf.toNumber(-SAFE - 1n)), "numerus overflow");
});

test("store checks the slot bounds and names the failure", () => {
  assert.equal(inf.store(255n, 0n, 255n, "u8", "declaration of `x`", ""), 255);
  assert.equal(
    trapText(() => inf.store(256n, 0n, 255n, "u8", "declaration of `x`", "")),
    "256 does not fit in `u8` (declaration of `x`)",
  );
  assert.equal(
    trapText(() => inf.store(-1n, 0n, 255n, "u8", "declaration of `x`", "")),
    "-1 does not fit in `u8` (declaration of `x`) (a negative value cannot be stored in an unsigned slot)",
  );
  assert.equal(
    trapText(() => inf.store(-129n, -128n, 127n, "i8", "return", " extra")),
    "-129 does not fit in `i8` (return) extra",
  );
});

test("store traps past the number carrier even inside the slot bounds", () => {
  const hi = 18446744073709551615n;
  assert.equal(trapText(() => inf.store(SAFE + 1n, 0n, hi, "u64", "x", "")), "numerus overflow");
});

test("clamp saturates and never traps", () => {
  assert.equal(inf.clamp(300n, 0n, 255n), 255);
  assert.equal(inf.clamp(-5n, 0n, 255n), 0);
  assert.equal(inf.clamp(7n, 0n, 255n), 7);
});

test("parse reads digits at a radix and rejects malformed text", () => {
  assert.equal(inf.parse("18446744073709551616", 10), 18446744073709551616n);
  assert.equal(inf.parse("-42", 10), -42n);
  assert.equal(inf.parse("+42", 10), 42n);
  assert.equal(inf.parse("ff", 16), 255n);
  assert.equal(inf.parse("-ff", 16), -255n);
  assert.equal(inf.parse("101", 2), 5n);
  assert.equal(inf.parse("17", 8), 15n);
  assert.equal(inf.parse("", 10), null);
  assert.equal(inf.parse("1.5", 10), null);
  assert.equal(inf.parse("12", 2), null);
  assert.equal(inf.parse("0x1f", 16), null);
});

test("pack pads to the width and traps or recovers outside it", () => {
  assert.equal(inf.pack(255n, 16, 4), "00ff");
  assert.equal(trapText(() => inf.pack(-1n, 16, 4)), "ascii_pack_negative_source");
  assert.equal(trapText(() => inf.pack(65536n, 16, 4)), "ascii_pack_width_overflow");
  assert.equal(inf.pack(-1n, 16, 4, () => "none"), "none");
  assert.equal(inf.pack(65536n, 16, 4, () => "wide"), "wide");
});

test("toDec scales into the i64 carrier or yields null", () => {
  assert.equal(inf.toDec(3n, 100000000n), 300000000n);
  assert.equal(inf.toDec(92233720368n, 100000000n), 9223372036800000000n);
  assert.equal(inf.toDec(92233720369n, 100000000n), null);
  assert.equal(inf.toDec(-92233720369n, 100000000n), null);
});

test("toBytes is the minimal two's complement, fromBytes inverts it", () => {
  assert.deepEqual(inf.toBytes(0n, false), [0]);
  assert.deepEqual(inf.toBytes(127n, false), [127]);
  assert.deepEqual(inf.toBytes(128n, false), [0, 128]);
  assert.deepEqual(inf.toBytes(128n, true), [128, 0]);
  assert.deepEqual(inf.toBytes(-1n, false), [255]);
  assert.deepEqual(inf.toBytes(-129n, false), [255, 127]);
  for (const v of [0n, 1n, -1n, 127n, 128n, -128n, -129n, 65535n, -65536n, 1n << 100n]) {
    for (const little of [false, true]) {
      assert.equal(inf.fromBytes(inf.toBytes(v, little), little), v);
    }
  }
  assert.equal(inf.fromBytes([0xff, 0x7f], false), -129n);
  assert.equal(inf.fromBytes([0x00, 0x80], false), 128n);
  assert.equal(inf.fromBytes([0xff, 0x7f], true), 32767n);
  assert.equal(inf.fromBytes([], false), 0n);
});

test("fromValor extracts an integer from any carrier or yields null", () => {
  assert.equal(inf.fromValor(5n), 5n);
  assert.equal(inf.fromValor(7), 7n);
  assert.equal(inf.fromValor(1.5), null);
  assert.equal(inf.fromValor("x"), null);
  assert.equal(inf.fromValor(null), null);
  assert.equal(inf.fromValor({ __faberValorTag: "Numerus", __faberValorPayload: 9n }), 9n);
  assert.equal(inf.fromValor({ __faberValorTag: "Numerus", __faberValorPayload: 9 }), 9n);
  assert.equal(inf.fromValor({ __faberValorTag: "Textus", __faberValorPayload: "9" }), null);
});
