import assert from "node:assert/strict";
import test from "node:test";
import { modulus } from "./index.ts";

const operations = ["add", "sub", "mul", "and", "or", "xor", "neg", "bitNot", "div", "mod", "shl", "shr"] as const;

test("the generated family exports twelve functions for all eight widths", () => {
  assert.equal(Object.keys(modulus).length, 96);
  for (const signed of [false, true]) {
    for (const bits of [8, 16, 32, 64]) {
      for (const op of operations) {
        assert.equal(typeof modulus[`${op}${signed ? "I" : "U"}${bits}`], "function");
      }
    }
  }
});

// Independent bigint ring oracle: signed division/remainder floor on the
// representatives; oversized right shifts sign-fill and left shifts are zero.
function expected(op: typeof operations[number], a: bigint, b: bigint, bits: number, signed: boolean): bigint {
  const reduce = (n: bigint) => signed ? BigInt.asIntN(bits, n) : BigInt.asUintN(bits, n);
  const floor = () => a % b !== 0n && (a < 0n) !== (b < 0n) ? a / b - 1n : a / b;
  switch (op) {
    case "add": return reduce(a + b);
    case "sub": return reduce(a - b);
    case "mul": return reduce(a * b);
    case "and": return reduce(a & b);
    case "or": return reduce(a | b);
    case "xor": return reduce(a ^ b);
    case "neg": return reduce(-a);
    // The legacy signed number helper uses JS's 32-bit complement, not
    // a width store. Preserve that representation even for 8/16 bits.
    case "bitNot": return ~a & (signed ? -1n : (1n << BigInt(bits)) - 1n);
    case "div": return reduce(signed ? floor() : a / b);
    case "mod": return signed ? a - b * floor() : a % b;
    case "shl": return b >= BigInt(bits) ? 0n : reduce(a << b);
    case "shr": return b >= BigInt(bits) ? (signed && a < 0n ? -1n : 0n) : reduce(a >> b);
  }
}

for (const signed of [false, true]) {
  for (const bits of [8, 16, 32, 64]) {
    const suffix = `${signed ? "I" : "U"}${bits}`;
    const max = (1n << BigInt(bits - (signed ? 1 : 0))) - 1n;
    const min = signed ? -(1n << BigInt(bits - 1)) : 0n;
    const values = signed ? [min, -7n, -1n, 0n, 1n, 7n, max] : [0n, 1n, 7n, max];

    test(`${suffix}: all ring operations retain their results`, () => {
      for (const op of operations) {
        const fn = modulus[op + suffix];
        for (const a of values) {
          const rhs = op === "shl" || op === "shr" ? [0n, 1n, BigInt(bits - 1), BigInt(bits), BigInt(bits + 1)] : values;
          for (const b of rhs) {
            if ((op === "div" || op === "mod") && b === 0n) continue;
            const actual = bits === 64 ? fn(a, b) : fn(Number(a), Number(b));
            assert.equal(BigInt(actual), expected(op, a, b, bits, signed), `${op}${suffix}(${a}, ${b})`);
          }
        }
      }
    });

    test(`${suffix}: both zero-divisor failures retain the runner text`, () => {
      const seven = bits === 64 ? 7n : 7;
      const zero = bits === 64 ? 0n : 0;
      for (const op of ["div", "mod"]) {
        assert.throws(() => modulus[op + suffix](seven, zero), { message: "modulus division failed" });
      }
    });
  }
}

test("unsigned 32-bit multiplication keeps exact low bits", () => {
  assert.equal(modulus.mulU32(0xffffffff, 0xffffffff), 1);
});

test("64-bit signed MIN / -1 reduces to MIN", () => {
  const min = -(1n << 63n);
  assert.equal(modulus.divI64(min, -1n), min);
  assert.equal(modulus.modI64(min, -1n), 0n);
});
