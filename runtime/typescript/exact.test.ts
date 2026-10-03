// Run: node --test runtime/typescript/exact.test.ts   (Node 24+ strips types)
//
// Every trap text is the text the Faber MIR runner reports for the same
// failure; the rows pin the former inline helpers' results and messages.
import assert from "node:assert/strict";
import { test } from "node:test";
import { exact } from "./index.ts";

const MAX = 9007199254740991;

function trapText(run: () => unknown): string {
  try {
    run();
  } catch (error) {
    assert.ok(error instanceof Error, "a trap is an Error");
    return error.message;
  }
  return "no trap";
}

test("chk accepts the safe-integer range and normalises -0", () => {
  assert.equal(exact.chk(MAX), MAX);
  assert.equal(exact.chk(-MAX), -MAX);
  assert.ok(Object.is(exact.chk(-0), 0));
});

test("chk traps outside the safe-integer range", () => {
  assert.equal(trapText(() => exact.chk(MAX + 2)), "numerus overflow");
  assert.equal(trapText(() => exact.chk(-MAX - 2)), "numerus overflow");
});

test("add, sub and mul trap past the representation window", () => {
  assert.equal(exact.add(2, 3), 5);
  assert.equal(exact.sub(2, 3), -1);
  assert.equal(exact.mul(-4, 5), -20);
  assert.equal(exact.add(MAX - 1, 1), MAX);
  assert.equal(trapText(() => exact.add(MAX, 1)), "numerus overflow");
  assert.equal(trapText(() => exact.sub(-MAX, 2)), "numerus overflow");
  assert.equal(trapText(() => exact.mul(MAX, 2)), "numerus overflow");
});

test("div floors and traps on a zero divisor", () => {
  assert.equal(exact.div(7, 2), 3);
  assert.equal(exact.div(-7, 2), -4);
  assert.equal(exact.div(7, -2), -4);
  assert.equal(exact.div(-7, -2), 3);
  assert.equal(trapText(() => exact.div(1, 0)), "numerus division failed");
});

test("mod takes the divisor's sign and traps on a zero divisor", () => {
  assert.equal(exact.mod(7, 2), 1);
  assert.equal(exact.mod(-7, 2), 1);
  assert.equal(exact.mod(7, -2), -1);
  assert.equal(exact.mod(-7, -2), -1);
  assert.ok(Object.is(exact.mod(-4, 2), 0));
  assert.equal(trapText(() => exact.mod(1, 0)), "numerus division failed");
});

test("shl and shr scale by exact powers of two", () => {
  assert.equal(exact.shl(1, 52), 4503599627370496);
  assert.equal(exact.shl(0, 70), 0);
  assert.equal(exact.shr(-5, 1), -3);
  assert.equal(exact.shr(5, 1), 2);
  assert.equal(exact.shr(-1, 63), -1);
  assert.equal(exact.shr(1, 63), 0);
  assert.equal(trapText(() => exact.shl(1, 53)), "numerus overflow");
  assert.equal(trapText(() => exact.shl(1, 63)), "numerus overflow");
  assert.equal(trapText(() => exact.shl(1, -1)), "negative shift count");
  assert.equal(trapText(() => exact.shr(1, -1)), "negative shift count");
});

test("floatRem takes the divisor's sign, including a signed zero", () => {
  assert.equal(exact.floatRem(7.5, 2), 1.5);
  assert.equal(exact.floatRem(-7.5, 2), 0.5);
  assert.equal(exact.floatRem(7.5, -2), -0.5);
  assert.ok(Object.is(exact.floatRem(4, 2), 0));
  assert.ok(Object.is(exact.floatRem(4, -2), -0));
  assert.ok(Number.isNaN(exact.floatRem(1, 0)));
});

test("neg, not and abs check the window", () => {
  assert.equal(exact.neg(5), -5);
  assert.ok(Object.is(exact.neg(0), 0));
  assert.equal(exact.not(5), -6);
  assert.equal(exact.not(-1), 0);
  assert.equal(exact.abs(-5), 5);
  assert.equal(trapText(() => exact.neg(MAX + 2)), "numerus overflow");
  assert.equal(trapText(() => exact.abs(-MAX - 2)), "numerus overflow");
});

test("pow is exact and traps on a negative exponent or overflow", () => {
  assert.equal(exact.pow(2, 10), 1024);
  assert.equal(exact.pow(-3, 3), -27);
  assert.equal(exact.pow(5, 0), 1);
  assert.equal(
    trapText(() => exact.pow(2, -1)),
    "numerus potentia failed: negative exponent",
  );
  assert.equal(trapText(() => exact.pow(2, 53)), "numerus overflow");
});

test("pow preserves the expression value until a bounded store", () => {
  assert.equal(exact.pow(20, 2), 400);
  assert.equal(exact.pow(20, 2) / 2, 200);
  assert.equal(exact.wrap(exact.pow(20, 2) / 2, 8, false), 200);
  assert.equal(exact.wrap(exact.pow(20, 2), 8, false), 144);
  assert.equal(exact.clamp(exact.pow(20, 2), 0, 255), 255);
});

test("pow preserves zero-exponent and signed-base behavior", () => {
  assert.equal(exact.pow(0, 0), 1);
  assert.equal(exact.pow(0, 5), 0);
  assert.equal(exact.pow(-3, 2), 9);
  assert.equal(exact.pow(-1, 63), -1);
  assert.equal(exact.pow(2, 52), 4503599627370496);
});

test("hi, lo and join split and rejoin a value", () => {
  for (const v of [0, 1, -1, 4294967295, 4294967296, -4294967297, MAX, -MAX]) {
    assert.equal(exact.join(exact.hi(v), exact.lo(v)), v);
  }
  assert.equal(exact.hi(-1), -1);
  assert.equal(exact.lo(-1), 4294967295);
  assert.equal(trapText(() => exact.hi(MAX + 2)), "numerus overflow");
});

test("and, or and xor are two's complement on the full value", () => {
  assert.equal(exact.and(12, 10), 8);
  assert.equal(exact.or(12, 10), 14);
  assert.equal(exact.xor(12, 10), 6);
  assert.equal(exact.and(-1, 4294967296 + 5), 4294967296 + 5);
  assert.equal(exact.or(-4294967296, 7), -4294967289);
  assert.equal(exact.xor(-1, 1), -2);
});

test("store passes an in-range value and names a failure", () => {
  assert.equal(exact.store(5, 0, 255, "u8", "declaration of `x`", ""), 5);
  assert.ok(Object.is(exact.store(-0, -128, 127, "i8", "assignment", ""), 0));
  assert.equal(
    trapText(() => exact.store(300, 0, 255, "u8", "declaration of `x`", "")),
    "300 does not fit in `u8` (declaration of `x`)",
  );
  assert.equal(
    trapText(() => exact.store(200, -128, 127, "i8", "argument `n`", "")),
    "200 does not fit in `i8` (argument `n`)",
  );
});

test("store adds the inferred-slot note and the unsigned-negative note", () => {
  assert.equal(
    trapText(() =>
      exact.store(
        -1,
        0,
        255,
        "u8",
        "declaration of `x`",
        "; `x` was inferred as `u8` from a literal, declare its type",
      ),
    ),
    "-1 does not fit in `u8` (declaration of `x`); `x` was inferred as `u8` from a literal, declare its type (a negative value cannot be stored in an unsigned slot)",
  );
  assert.equal(
    trapText(() => exact.store(-1, 0, 255, "u8", "assignment", "")),
    "-1 does not fit in `u8` (assignment) (a negative value cannot be stored in an unsigned slot)",
  );
});

test("clamp saturates to the nearer bound", () => {
  assert.equal(exact.clamp(300, 0, 255), 255);
  assert.equal(exact.clamp(-5, 0, 255), 0);
  assert.equal(exact.clamp(7, 0, 255), 7);
  assert.ok(Object.is(exact.clamp(-0, -10, 10), 0));
});

test("wrap reduces modulo 2^bits", () => {
  assert.equal(exact.wrap(256, 8, false), 0);
  assert.equal(exact.wrap(-1, 8, false), 255);
  assert.equal(exact.wrap(128, 8, true), -128);
  assert.equal(exact.wrap(127, 8, true), 127);
  assert.equal(exact.wrap(-129, 8, true), 127);
  assert.equal(exact.wrap(4294967296 + 5, 32, false), 5);
});

test("trueDiv and trueDivF32 divide as floats and trap on zero", () => {
  assert.equal(exact.trueDiv(7, 2), 3.5);
  assert.equal(exact.trueDivF32(1, 3), Math.fround(1 / 3));
  assert.equal(trapText(() => exact.trueDiv(1, 0)), "numerus division failed");
  assert.equal(
    trapText(() => exact.trueDivF32(1, 0)),
    "numerus division failed",
  );
});
