/**
 * Faber TypeScript runtime — the `exact` namespace.
 *
 * Generated TypeScript files `import { exact } from "@faber/runtime"` and call
 * `exact.mul(a, b)` instead of inlining the exact-integer helpers into every
 * emitted file (the package re-exports this module as the `exact` namespace).
 *
 * TS integers are JS `number`. They are exact only up to 2^53: an integer
 * intermediate or result outside ±(2^53 - 1) traps instead of rounding (the
 * documented capability gap; no `BigInt` carrier, so the clamp stays at the
 * safe-integer range rather than the true 64-bit one). Inside that range every
 * helper computes the exact value: sums and products that leave it round to a
 * magnitude of at least 2^53 and are caught by the range check; floor division
 * goes through the (exact) remainder; shifts scale by exact powers of two;
 * bitwise operators split the operands into a signed high part and an unsigned
 * low 32-bit part, which is two's complement on the full value.
 *
 * Nothing here narrows per operation: `store`, `wrap` and `clamp` are where a
 * value leaves the carrier into a bounded slot, and they are the only places a
 * width limit applies.
 *
 * The bodies mirror the former inline emission exactly: results and trap texts
 * are byte-identical to the compiler-inlined helpers.
 */

/** Range check: the value must lie within ±(2^53 - 1). Returns it with `-0` normalised to `0`. */
export function chk(v: number): number {
  if (v > 9007199254740991 || v < -9007199254740991) {
    throw new Error("numerus overflow");
  }
  return v + 0;
}

export function add(a: number, b: number): number {
  return chk(a + b);
}

export function sub(a: number, b: number): number {
  return chk(a - b);
}

export function mul(a: number, b: number): number {
  return chk(a * b);
}

/** Floor division: rounds toward negative infinity. */
export function div(a: number, b: number): number {
  if (b === 0) {
    throw new Error("numerus division failed");
  }
  const r = a % b;
  const q = (a - r) / b;
  return chk(r !== 0 && (r < 0) !== (b < 0) ? q - 1 : q);
}

/** Floor remainder: the result takes the divisor's sign. */
export function mod(a: number, b: number): number {
  if (b === 0) {
    throw new Error("numerus division failed");
  }
  const r = a % b;
  return (r !== 0 && (r < 0) !== (b < 0) ? r + b : r) + 0;
}

export function shl(a: number, n: number): number {
  if (n < 0) {
    throw new Error("negative shift count");
  }
  if (a === 0) {
    return 0;
  }
  if (n > 62) {
    throw new Error("numerus overflow");
  }
  return chk(a * Math.pow(2, n));
}

export function shr(a: number, n: number): number {
  if (n < 0) {
    throw new Error("negative shift count");
  }
  if (n > 62) {
    return a < 0 ? -1 : 0;
  }
  return Math.floor(a / Math.pow(2, n)) + 0;
}

/** Floor remainder of two floats (F9 ruling 29): the divisor's sign, never JS's truncating `%`. */
export function floatRem(a: number, b: number): number {
  const r = a % b;
  return r === 0 ? (b < 0 ? -0 : 0) : (r < 0) !== (b < 0) ? r + b : r;
}

export function neg(a: number): number {
  return chk(-a);
}

export function not(a: number): number {
  return chk(-a - 1);
}

export function abs(a: number): number {
  return chk(Math.abs(a));
}

export function pow(base: number, exponent: number): number {
  if (exponent < 0) {
    throw new Error("numerus potentia failed: negative exponent");
  }
  let accumulator = 1;
  let b = base;
  let e = exponent;
  while (e > 0) {
    if (e % 2 !== 0) {
      accumulator = mul(accumulator, b);
    }
    e = Math.floor(e / 2);
    if (e > 0) {
      b = mul(b, b);
    }
  }
  return accumulator;
}

/** The signed high part of `a` (above bit 32). */
export function hi(a: number): number {
  return Math.floor(chk(a) / 4294967296);
}

/** The unsigned low 32-bit part of `a`. */
export function lo(a: number): number {
  return a - hi(a) * 4294967296;
}

/** Rejoin a high part and a low 32-bit part into one value. */
export function join(hi: number, lo: number): number {
  return chk(hi * 4294967296 + (lo >>> 0));
}

export function and(a: number, b: number): number {
  return join(hi(a) & hi(b), lo(a) & lo(b));
}

export function or(a: number, b: number): number {
  return join(hi(a) | hi(b), lo(a) | lo(b));
}

export function xor(a: number, b: number): number {
  return join(hi(a) ^ hi(b), lo(a) ^ lo(b));
}

/** The trapping store: a value outside `[lo, hi]` throws naming value, type and slot. */
export function store(
  v: number,
  lo: number,
  hi: number,
  ty: string,
  pos: string,
  extra: string,
): number {
  if (v < lo || v > hi) {
    throw new Error(
      String(v) +
        " does not fit in `" +
        ty +
        "` (" +
        pos +
        ")" +
        extra +
        (lo === 0 && v < 0
          ? " (a negative value cannot be stored in an unsigned slot)"
          : ""),
    );
  }
  return v + 0;
}

/** The saturating store: a value outside `[lo, hi]` clamps to the nearer bound. */
export function clamp(v: number, lo: number, hi: number): number {
  return v < lo ? lo : v > hi ? hi : v + 0;
}

/** The wrapping store: the value reduced modulo 2^bits, signed or unsigned. */
export function wrap(v: number, bits: number, signed: boolean): number {
  const m = Math.pow(2, bits);
  let r = v % m;
  if (r < 0) {
    r += m;
  }
  if (signed && r >= m / 2) {
    r -= m;
  }
  return r + 0;
}

/** Integer `÷`: true division of two integers as an f64. */
export function trueDiv(a: number, b: number): number {
  if (b === 0) {
    throw new Error("numerus division failed");
  }
  return a / b;
}

/** Integer `÷` landing in an f32. */
export function trueDivF32(a: number, b: number): number {
  if (b === 0) {
    throw new Error("numerus division failed");
  }
  return Math.fround(a / b);
}
