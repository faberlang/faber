/**
 * Faber TypeScript runtime — the `order` namespace.
 *
 * Generated TypeScript files `import { order } from "@faber/runtime"` and call
 * `order.compare(a, b)` / `order.textCompare(a, b)` instead of inlining the
 * family into every emitted file (the package re-exports this module as the
 * `order` namespace).
 *
 * The helpers mirror the former inline emission exactly: the ordering produced
 * by this package is byte-identical to the compiler-inlined text.
 */

/** Three-way text compare in Unicode code-point order (D1.6). */
export function textCompare(a: string, b: string): number {
  // JS string `<` compares UTF-16 code units, which misorders non-BMP scalars
  // (surrogates `D800–DFFF`) against BMP `E000–FFFF`. At the first differing
  // unit, remapping surrogates above `E000–FFFF` yields code-point order; a
  // proper prefix sorts first.
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) {
    let x = a.charCodeAt(i);
    let y = b.charCodeAt(i);
    if (x === y) continue;
    if (x >= 0xd800) x += x >= 0xe000 ? -0x800 : 0x2000;
    if (y >= 0xd800) y += y >= 0xe000 ? -0x800 : 0x2000;
    return x - y;
  }
  return a.length - b.length;
}

/** Three-way compare for the ordering contract (D1.4). */
export function compare(a: any, b: any): number {
  // Numbers use IEEE 754 totalOrder (operator default F2): −0 below +0 and NaN
  // above every number (JS does not expose a NaN sign). Text uses code point
  // order (D1.6), tuples lexicographic order, `Date` its time value, and any
  // other object its own `compare` method (a conforming genus).
  if (typeof a === "number") {
    if (a < b) return -1;
    if (a > b) return 1;
    if (a === b) return Object.is(a, -0) === Object.is(b, -0) ? 0 : (Object.is(a, -0) ? -1 : 1);
    return a !== a ? (b !== b ? 0 : 1) : -1;
  }
  if (typeof a === "bigint") return a < b ? -1 : (a > b ? 1 : 0);
  if (typeof a === "string") return Math.sign(textCompare(a, b));
  if (a instanceof Date) return Math.sign(a.getTime() - b.getTime());
  if (Array.isArray(a)) {
    for (let i = 0; i < a.length; i++) {
      const c = compare(a[i], b[i]);
      if (c !== 0) return c;
    }
    return 0;
  }
  return Math.sign(a.compare(b));
}
