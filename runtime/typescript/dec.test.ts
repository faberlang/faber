// Run: node --test runtime/typescript/dec.test.ts   (Node 24+ strips types)
//
// Every trap text is the text the Faber MIR runner reports for the same
// failure; the rows pin the former inline helpers' results and messages.
import assert from "node:assert/strict";
import { test } from "node:test";
import { dec } from "./index.ts";

const LIMIT = 1n << 127n;

function trapText(run: () => unknown): string {
  try {
    run();
  } catch (error) {
    assert.ok(error instanceof Error, "a trap is an Error");
    return error.message;
  }
  return "no trap";
}

// A stored d64 value is the integer at scale 8.
const d = (whole: number) => dec.of(BigInt(whole) * 100000000n);

test("of and int build carrier/scale pairs", () => {
  assert.deepEqual(dec.of(150000000n), [150000000n, 8]);
  assert.deepEqual(dec.int(7n), [7n, 0]);
});

test("add and sub align scales and normalise down to scale 8", () => {
  assert.deepEqual(dec.add(dec.int(1n), dec.of(50000000n)), [150000000n, 8]);
  assert.deepEqual(dec.sub(dec.int(1n), dec.of(50000000n)), [50000000n, 8]);
  // A scale above 8 sheds trailing zeros but never below 8.
  assert.deepEqual(dec.add([10n, 10], [10n, 10]), [2n, 9]);
});

test("mul adds the scales", () => {
  assert.deepEqual(dec.mul(dec.int(3n), dec.int(4n)), [12n, 0]);
  assert.deepEqual(dec.mul([15n, 1], [15n, 1]), [225n, 2]);
});

test("div rounds half-even at the larger operand scale", () => {
  assert.deepEqual(dec.div(dec.int(1n), dec.int(3n)), [0n, 0]);
  assert.deepEqual(dec.div(dec.int(5n), dec.int(2n)), [2n, 0]);
  assert.deepEqual(dec.div(dec.int(7n), dec.int(2n)), [4n, 0]);
  assert.deepEqual(dec.div(dec.of(100000000n), dec.of(300000000n)), [33333333n, 8]);
});

test("div by zero traps with the division text", () => {
  assert.equal(trapText(() => dec.div(dec.int(1n), dec.int(0n))), "numerus division failed");
});

test("rem is the floor remainder with the sign of the divisor", () => {
  assert.deepEqual(dec.rem(dec.int(7n), dec.int(3n)), [1n, 0]);
  assert.deepEqual(dec.rem(dec.int(-7n), dec.int(3n)), [2n, 0]);
  assert.deepEqual(dec.rem(dec.int(7n), dec.int(-3n)), [-2n, 0]);
  assert.equal(trapText(() => dec.rem(dec.int(1n), dec.int(0n))), "numerus division failed");
});

test("neg negates the carrier", () => {
  assert.deepEqual(dec.neg(dec.int(5n)), [-5n, 0]);
});

test("an intermediate past the 128-bit carrier traps", () => {
  const near: dec.Carrier = [LIMIT - 1n, 0];
  assert.equal(trapText(() => dec.add(near, dec.int(1n))), "numerus overflow");
  assert.equal(trapText(() => dec.sub([-LIMIT, 0], dec.int(1n))), "numerus overflow");
  assert.equal(trapText(() => dec.mul(near, dec.int(2n))), "numerus overflow");
  assert.equal(trapText(() => dec.neg([-LIMIT, 0])), "numerus overflow");
  assert.equal(trapText(() => dec.div(near, [1n, 5])), "numerus division failed");
});

test("cmp orders by exact value across scales", () => {
  assert.equal(dec.cmp(dec.int(1n), dec.of(100000000n)), 0);
  assert.equal(dec.cmp(dec.int(1n), dec.of(100000001n)), -1);
  assert.equal(dec.cmp(dec.of(100000001n), dec.int(1n)), 1);
});

test("fmt trims trailing fraction zeros and keeps the sign", () => {
  assert.equal(dec.fmt(dec.of(150000000n)), "1.5");
  assert.equal(dec.fmt(dec.of(100000000n)), "1");
  assert.equal(dec.fmt(dec.of(-5n)), "-0.00000005");
  assert.equal(dec.fmt(dec.int(0n)), "0");
  assert.equal(dec.fmtList([100000000n, 250000000n]), "[1, 2.5]");
  assert.equal(dec.fmtList([]), "[]");
});

test("tryStore rounds half-even to scale 8 and bounds at i64", () => {
  assert.equal(dec.tryStore(dec.int(2n)), 200000000n);
  assert.equal(dec.tryStore([125n, 10]), 1n); // 0.0000000125 -> 1.25e-8 -> 1
  assert.equal(dec.tryStore([15n, 10]), 0n); // 1.5e-9 -> 0.15 ulp -> 0
  assert.equal(dec.tryStore([5n, 9]), 0n); // exact tie rounds to the even 0
  assert.equal(dec.tryStore([15n, 9]), 2n); // tie rounds to the even 2
  assert.equal(dec.tryStore(dec.int(92233720369n)), undefined);
  assert.equal(dec.tryStore(dec.int(-92233720369n)), undefined);
});

test("store traps outside the i64 range with the d64 text", () => {
  assert.equal(dec.store(dec.int(2n), "x", ""), 200000000n);
  assert.equal(
    trapText(() => dec.store(dec.int(92233720369n), "declaration of `x`", "")),
    "92233720369 does not fit in `d64` (declaration of `x`)",
  );
  assert.equal(
    trapText(() => dec.store(dec.int(-92233720369n), "return", " (inferred)")),
    "-92233720369 does not fit in `d64` (return) (inferred)",
  );
});

test("toInt rounds half-even to a whole integer", () => {
  assert.equal(dec.toInt(dec.of(250000000n)), 2n);
  assert.equal(dec.toInt(dec.of(350000000n)), 4n);
  assert.equal(dec.toInt(dec.of(-250000000n)), -2n);
  assert.equal(dec.toInt(dec.int(9n)), 9n);
});

test("f64 divides the carrier by the scale", () => {
  assert.equal(dec.f64(dec.of(150000000n)), 1.5);
  assert.equal(dec.f64(dec.int(3n)), 3);
});

test("floatCmp compares exact values and orders NaN against nothing", () => {
  assert.equal(dec.floatCmp(d(1), 1), 0);
  assert.equal(dec.floatCmp(d(1), 1.5), -1);
  assert.equal(dec.floatCmp(d(2), 1.5), 1);
  // 0.1 as a double is not the decimal 0.1.
  assert.notEqual(dec.floatCmp(dec.of(10000000n), 0.1), 0);
  assert.equal(dec.floatCmp(d(1), Infinity), -1);
  assert.equal(dec.floatCmp(d(1), -Infinity), 1);
  assert.ok(Number.isNaN(dec.floatCmp(d(1), NaN)));
});

test("tryParse yields the scaled integer or undefined", () => {
  assert.equal(dec.tryParse("1.5", 8), 150000000n);
  assert.equal(dec.tryParse("-0.25", 8), -25000000n);
  assert.equal(dec.tryParse("+2", 8), 200000000n);
  assert.equal(dec.tryParse("1e2", 8), 10000000000n);
  assert.equal(dec.tryParse("1.5e-1", 8), 15000000n);
  assert.equal(dec.tryParse("0.000000005", 8), 0n); // tie -> even
  assert.equal(dec.tryParse("0.000000015", 8), 2n);
  assert.equal(dec.tryParse("0e999", 8), 0n);
  assert.equal(dec.tryParse("abc", 8), undefined);
  assert.equal(dec.tryParse("", 8), undefined);
  assert.equal(dec.tryParse("1.5x", 8), undefined);
  assert.equal(dec.tryParse("1e", 8), undefined);
  assert.equal(dec.tryParse("92233720369", 8), undefined);
});

test("parse traps with the conversion texts", () => {
  assert.equal(dec.parse("1.5", 8), 150000000n);
  assert.equal(trapText(() => dec.parse("abc", 8)), "textus to numerus conversion failed");
  assert.equal(
    trapText(() => dec.parse("92233720369", 8)),
    "textus to numerus conversion out of range",
  );
  assert.equal(
    trapText(() => dec.parse("1e99", 8)),
    "textus to numerus conversion out of range",
  );
});
