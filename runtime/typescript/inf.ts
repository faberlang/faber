/**
 * Faber TypeScript runtime — the `inf` namespace.
 *
 * Generated TypeScript files `import { inf } from "@faber/runtime"` and call
 * `inf.div(a, b)` instead of inlining the unbounded-integer helpers into every
 * emitted file (the package re-exports this module as the `inf` namespace).
 *
 * The unbounded integer `inf` rides native `bigint`: exact, never a size trap.
 * `div` and `mod` floor, `pow` and the shifts are exact, and `approx` is the
 * exact relative-1e-9 rule. A `bigint` leaves the carrier into a bounded slot
 * in two places only: `store` (the ruled trap when the value does not fit) and
 * `clamp` (the saturating store); `toNumber` and the conversions below carry
 * the `number` carrier's own 2^53 limit. Nothing narrows per operation.
 *
 * The bodies mirror the former inline emission exactly: results and trap texts
 * are byte-identical to the compiler-inlined helpers.
 */

/** Floor division of two integers. */
export function div(a: bigint, b: bigint): bigint {
  if (b === 0n) {
    throw new Error("numerus division failed");
  }
  const q = a / b;
  return a % b !== 0n && (a < 0n) !== (b < 0n) ? q - 1n : q;
}

/** Floor remainder of two integers (the result takes the sign of `b`). */
export function mod(a: bigint, b: bigint): bigint {
  if (b === 0n) {
    throw new Error("numerus division failed");
  }
  const r = a % b;
  return r !== 0n && (r < 0n) !== (b < 0n) ? r + b : r;
}

/** Exact power; a negative exponent traps. */
export function pow(base: bigint, exponent: bigint): bigint {
  if (exponent < 0n) {
    throw new Error("numerus potentia failed: negative exponent");
  }
  return base ** exponent;
}

/** Exact left shift; a negative count traps. */
export function shl(a: bigint, n: bigint): bigint {
  if (n < 0n) {
    throw new Error("negative shift count");
  }
  return a << n;
}

/** Arithmetic right shift; a negative count traps. */
export function shr(a: bigint, n: bigint): bigint {
  if (n < 0n) {
    throw new Error("negative shift count");
  }
  return a >> n;
}

/** Absolute value. */
export function abs(a: bigint): bigint {
  return a < 0n ? -a : a;
}

function max(a: bigint, b: bigint): bigint {
  return a > b ? a : b;
}

/** Integer `÷`: true division in `f64`. */
export function trueDiv(a: bigint, b: bigint): number {
  if (b === 0n) {
    throw new Error("numerus division failed");
  }
  return Number(a) / Number(b);
}

/** The exact relative rule `10^9 * |a - b| <= max(|a|, |b|)`. */
export function approx(a: bigint, b: bigint): boolean {
  const d = abs(a - b);
  return 1000000000n * d <= max(abs(a), abs(b));
}

/**
 * Exact three-way comparison of an integer with a float: -1, 0 or 1, and NaN
 * when the float is NaN (NaN orders nothing).
 */
export function cmpFloat(a: bigint, f: number): number {
  if (Number.isNaN(f)) {
    return NaN;
  }
  if (f === Infinity) {
    return -1;
  }
  if (f === -Infinity) {
    return 1;
  }
  const t = Math.trunc(f);
  const ti = BigInt(t);
  if (a < ti) {
    return -1;
  }
  if (a > ti) {
    return 1;
  }
  const frac = f - t;
  return frac > 0 ? -1 : frac < 0 ? 1 : 0;
}

/** The `number` image of an integer; a magnitude past 2^53 - 1 traps. */
export function toNumber(v: bigint): number {
  if (v > 9007199254740991n || v < -9007199254740991n) {
    throw new Error("numerus overflow");
  }
  return Number(v);
}

/**
 * The checked store of an integer into a bounded `number` slot: the value must
 * lie in `[lo, hi]`, else the ruled store trap; then it must fit the carrier.
 */
export function store(
  v: bigint,
  lo: bigint,
  hi: bigint,
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
        (lo === 0n && v < 0n ? " (a negative value cannot be stored in an unsigned slot)" : ""),
    );
  }
  return toNumber(v);
}

/** The saturating store: the value clamped to `[lo, hi]`. */
export function clamp(v: bigint, lo: bigint, hi: bigint): number {
  return Number(v < lo ? lo : v > hi ? hi : v);
}

/** Parse integer text at `radix` (2, 8, 10 or 16); `null` for malformed text. */
export function parse(text: string, radix: number): bigint | null {
  const digits = radix === 16 ? "0-9a-fA-F" : radix === 2 ? "01" : radix === 8 ? "0-7" : "0-9";
  if (!new RegExp("^[+-]?[" + digits + "]+$").test(text)) {
    return null;
  }
  const prefix = radix === 16 ? "0x" : radix === 2 ? "0b" : radix === 8 ? "0o" : "";
  const v = BigInt(prefix + text.replace(/^[+-]/, ""));
  return text.startsWith("-") ? -v : v;
}

/**
 * The fixed-width digit pack of a non-negative integer at `radix`. A negative
 * source or a value wider than `width` digits takes `recover` when given, else
 * traps.
 */
export function pack(v: bigint, radix: number, width: number, recover?: () => string): string {
  const digits = v < 0n ? null : v.toString(radix);
  if (digits === null || digits.length > width) {
    if (recover) {
      return recover();
    }
    throw new Error(digits === null ? "ascii_pack_negative_source" : "ascii_pack_width_overflow");
  }
  return digits.padStart(width, "0");
}

/** The scaled `d64` carrier of an integer, or `null` outside the `i64` range. */
export function toDec(v: bigint, scale: bigint): bigint | null {
  const c = v * scale;
  return c < -9223372036854775808n || c > 9223372036854775807n ? null : c;
}

/** The minimal two's-complement bytes of an integer, big- or little-endian. */
export function toBytes(v: bigint, little: boolean): number[] {
  const bytes: number[] = [];
  let x = v;
  for (;;) {
    const byte = Number(BigInt.asUintN(8, x));
    bytes.push(byte);
    x = x >> 8n;
    if ((x === 0n && (byte & 0x80) === 0) || (x === -1n && (byte & 0x80) !== 0)) {
      break;
    }
  }
  return little ? bytes : bytes.reverse();
}

/** The integer a two's-complement byte sequence encodes, big- or little-endian. */
export function fromBytes(bytes: number[], little: boolean): bigint {
  const b = little ? [...bytes].reverse() : bytes;
  let v = 0n;
  for (const x of b) {
    v = (v << 8n) | BigInt(x);
  }
  return b.length > 0 && (b[0] & 0x80) !== 0 ? v - (1n << BigInt(8 * b.length)) : v;
}

/**
 * The integer a `valor` carries (a JS integer `number`, a `bigint`, a boxed
 * `Numerus`), or `null` when it carries none.
 */
export function fromValor(v: any): bigint | null {
  const p =
    v !== null && typeof v === "object" && typeof v.__faberValorTag === "string"
      ? v.__faberValorTag === "Numerus"
        ? v.__faberValorPayload
        : undefined
      : v;
  return typeof p === "bigint" ? p : typeof p === "number" && Number.isInteger(p) ? BigInt(p) : null;
}
