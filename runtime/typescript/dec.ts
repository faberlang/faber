/**
 * Faber TypeScript runtime — the `dec` namespace.
 *
 * Generated TypeScript files `import { dec } from "@faber/runtime"` and call
 * `dec.add(a, b)` instead of inlining the exact-decimal helpers into every
 * emitted file (the package re-exports this module as the `dec` namespace).
 *
 * An unstored decimal is a `[bigint, number]` carrier/scale pair; `+ -` align
 * scales, `*` adds scales, `/` rounds half-even at the larger operand scale,
 * every intermediate is bounded by the 128-bit carrier, and only the store
 * rounds half-even to scale 8 and traps outside the `i64` range. Mirrors the
 * MIR runner's `decimal` module, the golden authority.
 *
 * Nothing here narrows per operation: `store` and `tryStore` are where a
 * decimal leaves the carrier into a stored `d64` slot, and the only places the
 * `i64` width limit applies. The 128-bit bound on an intermediate is a
 * representation limit of the carrier, not a per-operation width limit.
 *
 * The bodies mirror the former inline emission exactly: results and trap texts
 * are byte-identical to the compiler-inlined helpers.
 */

/** An unstored decimal: a carrier integer and its scale (digits after the point). */
export type Carrier = [bigint, number];

const LIMIT = 1n << 127n;

function bound(carrier: bigint, message: string): bigint {
  if (carrier >= LIMIT || carrier < -LIMIT) {
    throw new Error(message);
  }
  return carrier;
}

function pow10(digits: number): bigint {
  return 10n ** BigInt(digits);
}

/** Round-half-even quotient of two integers. */
function rhe(numerator: bigint, denominator: bigint): bigint {
  if (denominator === 0n) {
    throw new Error("numerus division failed");
  }
  const quotient = numerator / denominator;
  const remainder = numerator % denominator;
  if (remainder === 0n) {
    return quotient;
  }
  const remAbs = remainder < 0n ? -remainder : remainder;
  const denAbs = denominator < 0n ? -denominator : denominator;
  const twice = remAbs * 2n;
  const rounds = twice > denAbs || (twice === denAbs && quotient % 2n !== 0n);
  if (!rounds) {
    return quotient;
  }
  const step = (numerator < 0n) === (denominator < 0n) ? 1n : -1n;
  return quotient + step;
}

/** A stored `d64` value (scale 8) as an unstored decimal. */
export function of(value: bigint): Carrier {
  return [value, 8];
}

/** A stored integer as an unstored decimal (scale 0). */
export function int(value: bigint): Carrier {
  return [value, 0];
}

function norm(carrier: bigint, scale: number): Carrier {
  while (scale > 8 && carrier % 10n === 0n) {
    carrier = carrier / 10n;
    scale -= 1;
  }
  return [carrier, scale];
}

function align(a: Carrier, b: Carrier): [bigint, bigint, number] {
  const scale = Math.max(a[1], b[1]);
  const x = bound(a[0] * pow10(scale - a[1]), "numerus overflow");
  const y = bound(b[0] * pow10(scale - b[1]), "numerus overflow");
  return [x, y, scale];
}

export function add(a: Carrier, b: Carrier): Carrier {
  const [x, y, scale] = align(a, b);
  return norm(bound(x + y, "numerus overflow"), scale);
}

export function sub(a: Carrier, b: Carrier): Carrier {
  const [x, y, scale] = align(a, b);
  return norm(bound(x - y, "numerus overflow"), scale);
}

export function mul(a: Carrier, b: Carrier): Carrier {
  return norm(bound(a[0] * b[0], "numerus overflow"), a[1] + b[1]);
}

export function div(a: Carrier, b: Carrier): Carrier {
  if (b[0] === 0n) {
    throw new Error("numerus division failed");
  }
  const scale = Math.max(a[1], b[1]);
  const numerator = bound(a[0] * pow10(scale + b[1] - a[1]), "numerus division failed");
  return norm(rhe(numerator, b[0]), scale);
}

export function rem(a: Carrier, b: Carrier): Carrier {
  const [x, y, scale] = align(a, b);
  if (y === 0n) {
    throw new Error("numerus division failed");
  }
  // Floor remainder (F9 ruling 29): the sign of the divisor, exact on the carriers.
  let r = x % y;
  if (r !== 0n && (r < 0n) !== (y < 0n)) {
    r += y;
  }
  return norm(r, scale);
}

export function neg(a: Carrier): Carrier {
  return norm(bound(-a[0], "numerus overflow"), a[1]);
}

/** Three-way comparison: -1, 0 or 1. */
export function cmp(a: Carrier, b: Carrier): number {
  const scale = Math.max(a[1], b[1]);
  const x = a[0] * pow10(scale - a[1]);
  const y = b[0] * pow10(scale - b[1]);
  return x < y ? -1 : x > y ? 1 : 0;
}

/** The decimal's text: no trailing fraction zeros, no point for a whole value. */
export function fmt(a: Carrier): string {
  const digits = a[1];
  let magnitude = (a[0] < 0n ? -a[0] : a[0]).toString();
  if (magnitude.length < digits + 1) {
    magnitude = "0".repeat(digits + 1 - magnitude.length) + magnitude;
  }
  const integer = magnitude.slice(0, magnitude.length - digits);
  const fraction = magnitude.slice(magnitude.length - digits).replace(/0+$/, "");
  const sign = a[0] < 0n ? "-" : "";
  return fraction === "" ? sign + integer : sign + integer + "." + fraction;
}

/** The text of a list of stored `d64` values. */
export function fmtList(values: bigint[]): string {
  return "[" + values.map((value) => fmt(of(value))).join(", ") + "]";
}

/** Round half-even to scale 8; `undefined` when the result is outside the `i64` range. */
export function tryStore(a: Carrier): bigint | undefined {
  const rounded = a[1] > 8 ? rhe(a[0], pow10(a[1] - 8)) : a[0] * pow10(8 - a[1]);
  if (rounded > 9223372036854775807n || rounded < -9223372036854775808n) {
    return undefined;
  }
  return rounded;
}

/** The store: round half-even to scale 8, trapping outside the `i64` range. */
export function store(a: Carrier, at: string, inferred: string): bigint {
  const value = tryStore(a);
  if (value === undefined) {
    throw new Error(fmt(a) + " does not fit in `d64` (" + at + ")" + inferred);
  }
  return value;
}

/** The decimal rounded half-even to a whole integer. */
export function toInt(a: Carrier): bigint {
  return rhe(a[0], pow10(a[1]));
}

export function f64(a: Carrier): number {
  return Number(a[0]) / Number(pow10(a[1]));
}

/** Exact three-way comparison against a float; `NaN` orders and equals nothing. */
export function floatCmp(a: Carrier, f: number): number {
  if (Number.isNaN(f)) {
    return NaN;
  }
  if (f === Infinity) {
    return -1;
  }
  if (f === -Infinity) {
    return 1;
  }
  let mantissa = f;
  let shift = 0;
  while (!Number.isInteger(mantissa)) {
    mantissa *= 2;
    shift += 1;
  }
  const left = a[0] << BigInt(shift);
  const right = BigInt(mantissa) * pow10(a[1]);
  return left < right ? -1 : left > right ? 1 : 0;
}

// The scan is unbounded and never throws: `undefined` is malformed text. The
// i64 bound applies to the conversion's result only (decimal-widths §2.2), in
// the two entry points below.
function scan(text: string, scaleDigits: number): bigint | undefined {
  let negative = false;
  let digits = text;
  if (digits.startsWith("-")) {
    negative = true;
    digits = digits.slice(1);
  } else if (digits.startsWith("+")) {
    digits = digits.slice(1);
  }
  let mantissa = 0n;
  let exp10 = 0;
  let seenDigit = false;
  let rest = digits;
  while (rest.length > 0 && rest[0] >= "0" && rest[0] <= "9") {
    mantissa = mantissa * 10n + BigInt(rest.charCodeAt(0) - 48);
    seenDigit = true;
    rest = rest.slice(1);
  }
  if (rest.startsWith(".")) {
    rest = rest.slice(1);
    while (rest.length > 0 && rest[0] >= "0" && rest[0] <= "9") {
      mantissa = mantissa * 10n + BigInt(rest.charCodeAt(0) - 48);
      exp10 -= 1;
      seenDigit = true;
      rest = rest.slice(1);
    }
  }
  if (rest.length > 0 && (rest[0] === "e" || rest[0] === "E")) {
    let exp = rest.slice(1);
    let sign = 1;
    if (exp.startsWith("-")) {
      sign = -1;
      exp = exp.slice(1);
    } else if (exp.startsWith("+")) {
      exp = exp.slice(1);
    }
    if (!/^[0-9]+$/.test(exp)) {
      return undefined;
    }
    exp10 = exp10 + sign * Number(exp);
  } else if (rest.length > 0) {
    return undefined;
  }
  if (!seenDigit) {
    return undefined;
  }
  if (negative) {
    mantissa = -mantissa;
  }
  const shift = exp10 + scaleDigits;
  if (mantissa === 0n) {
    return 0n;
  }
  if (shift >= 0) {
    // Past 40 digits the scaled mantissa is beyond every carrier: stop before
    // `10n ** shift` is built.
    if (shift > 40) {
      return mantissa < 0n ? -(1n << 127n) : 1n << 127n;
    }
    return mantissa * 10n ** BigInt(shift);
  }
  if (-shift > mantissa.toString().length + 1) {
    return 0n;
  }
  return rhe(mantissa, 10n ** BigInt(-shift));
}

/** Parse decimal text at `scaleDigits` digits; `undefined` for malformed or out-of-range text. */
export function tryParse(text: string, scaleDigits: number): bigint | undefined {
  const value = scan(text, scaleDigits);
  if (value === undefined) {
    return undefined;
  }
  if (value > 9223372036854775807n || value < -9223372036854775808n) {
    return undefined;
  }
  return value;
}

/** Parse decimal text at `scaleDigits` digits, trapping on malformed or out-of-range text. */
export function parse(text: string, scaleDigits: number): bigint {
  const value = scan(text, scaleDigits);
  if (value === undefined) {
    throw new Error("textus to numerus conversion failed");
  }
  if (value > 9223372036854775807n || value < -9223372036854775808n) {
    throw new Error("textus to numerus conversion out of range");
  }
  return value;
}
