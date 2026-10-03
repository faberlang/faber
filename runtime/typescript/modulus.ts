/**
 * Faber TypeScript runtime — the `modulus` namespace.
 *
 * The word-ring operations formerly emitted inline, with the same number
 * carriers for 8/16/32 bits and bigint carriers for 64 bits. These are ring
 * operations, not the width-policy stores of `exact.wrap` or `exact.clamp`.
 * Division/remainder floor on signed representatives; zero divisors keep the
 * runner's failure text. Shift counts are never masked (ruling 10).
 *
 * The family is built by width and operation, rather than copying twelve
 * function bodies for each of the eight widths.
 */

type Ring<T> = {
  add: (a: T, b: T) => T;
  sub: (a: T, b: T) => T;
  mul: (a: T, b: T) => T;
  and: (a: T, b: T) => T;
  or: (a: T, b: T) => T;
  xor: (a: T, b: T) => T;
  neg: (a: T) => T;
  bitNot: (a: T) => T;
  div: (a: T, b: T) => T;
  mod: (a: T, b: T) => T;
  shl: (a: T, b: T) => T;
  shr: (a: T, b: T) => T;
};

type NumberWidth = "U8" | "U16" | "U32" | "I8" | "I16" | "I32";
type BigWidth = "U64" | "I64";
type Family = {
  [Op in keyof Ring<number> as `${Op}${NumberWidth}`]: Ring<number>[Op];
} & {
  [Op in keyof Ring<bigint> as `${Op}${BigWidth}`]: Ring<bigint>[Op];
};

function divisionFailed(): never {
  throw new Error("modulus division failed");
}

function numberRing(bits: number, signed: boolean): Ring<number> {
  const shift = 32 - bits;
  const mask = bits === 8 ? 0xff : 0xffff;
  const reduce = signed
    ? (a: number) => bits === 32 ? a | 0 : (a << shift) >> shift
    : (a: number) => bits === 32 ? a >>> 0 : a & mask;
  return {
    add: (a, b) => reduce(a + b),
    sub: (a, b) => reduce(a - b),
    mul: (a, b) => reduce(bits === 32 ? Math.imul(a, b) : a * b),
    and: (a, b) => signed ? a & b : reduce(a & b),
    or: (a, b) => signed ? a | b : reduce(a | b),
    xor: (a, b) => signed ? a ^ b : reduce(a ^ b),
    neg: (a) => reduce(-a),
    bitNot: (a) => signed ? ~a : reduce(~a),
    div: (a, b) => b === 0 ? divisionFailed()
      : signed ? reduce(Math.floor(a / b)) : Math.floor(a / b),
    mod: (a, b) => b === 0 ? divisionFailed()
      : signed ? a - b * Math.floor(a / b) : a % b,
    shl: (a, b) => b >= bits ? 0 : reduce(a << b),
    shr: (a, b) => signed
      ? b >= bits ? (a < 0 ? -1 : 0) : a >> b
      : b >= bits ? 0 : reduce(a >>> b),
  };
}

function bigRing(signed: boolean): Ring<bigint> {
  const reduce = signed
    ? (a: bigint) => BigInt.asIntN(64, a)
    : (a: bigint) => a & 0xffffffffffffffffn;
  return {
    add: (a, b) => reduce(a + b),
    sub: (a, b) => reduce(a - b),
    mul: (a, b) => reduce(a * b),
    and: (a, b) => reduce(a & b),
    or: (a, b) => reduce(a | b),
    xor: (a, b) => reduce(a ^ b),
    neg: (a) => reduce(-a),
    bitNot: (a) => reduce(~a),
    div: (a, b) => b === 0n ? divisionFailed()
      : signed ? reduce(a % b !== 0n && (a < 0n) !== (b < 0n) ? a / b - 1n : a / b)
      : a / b,
    mod: (a, b) => b === 0n ? divisionFailed()
      : signed && a % b !== 0n && (a % b < 0n) !== (b < 0n) ? a % b + b : a % b,
    shl: (a, b) => b >= 64n ? 0n : reduce(a << b),
    shr: (a, b) => b >= 64n ? (signed && a < 0n ? -1n : 0n) : signed ? a >> b : reduce(a >> b),
  };
}

function family(): Family {
  const functions: Record<string, Ring<number>[keyof Ring<number>] | Ring<bigint>[keyof Ring<bigint>]> = {};
  for (const signed of [false, true]) {
    for (const bits of [8, 16, 32, 64]) {
      const suffix = `${signed ? "I" : "U"}${bits}`;
      const ring = bits === 64 ? bigRing(signed) : numberRing(bits, signed);
      for (const [op, body] of Object.entries(ring)) {
        functions[op + suffix] = body;
      }
    }
  }
  return functions as Family;
}

export const {
  addU8, subU8, mulU8, andU8, orU8, xorU8, negU8, bitNotU8, divU8, modU8, shlU8, shrU8,
  addU16, subU16, mulU16, andU16, orU16, xorU16, negU16, bitNotU16, divU16, modU16, shlU16, shrU16,
  addU32, subU32, mulU32, andU32, orU32, xorU32, negU32, bitNotU32, divU32, modU32, shlU32, shrU32,
  addU64, subU64, mulU64, andU64, orU64, xorU64, negU64, bitNotU64, divU64, modU64, shlU64, shrU64,
  addI8, subI8, mulI8, andI8, orI8, xorI8, negI8, bitNotI8, divI8, modI8, shlI8, shrI8,
  addI16, subI16, mulI16, andI16, orI16, xorI16, negI16, bitNotI16, divI16, modI16, shlI16, shrI16,
  addI32, subI32, mulI32, andI32, orI32, xorI32, negI32, bitNotI32, divI32, modI32, shlI32, shrI32,
  addI64, subI64, mulI64, andI64, orI64, xorI64, negI64, bitNotI64, divI64, modI64, shlI64, shrI64,
} = family();
