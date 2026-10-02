/**
 * Faber TypeScript runtime — the `format` namespace.
 *
 * Generated TypeScript files `import { format } from "@faber/runtime"` and call
 * `format.int(value, spec)` for the `¶` format operator instead of inlining the
 * renderers into every emitted file (the package re-exports this module as the
 * `format` namespace).
 *
 * The renderers reproduce the reference renderer: integer precision pads, radix
 * kinds print sign plus magnitude, non-finite floats print `NaN`/`∞`/`-∞`,
 * scientific uses the `1.5e3` shape, the default float body is the Faber
 * `fractus` display, and widths count code points. A renderer lays out text; it
 * never stores into a bounded cell, so no width limit applies here.
 *
 * The bodies mirror the former inline emission exactly: results are
 * byte-identical to the compiler-inlined helpers.
 */

/** The decomposed layout spec a `¶` call site builds as an object literal. */
export type Spec = {
  fill: string;
  align: string;
  plus: boolean;
  zero: boolean;
  width: number;
  precision: number;
  kind: string;
};

/** Pad `sign + body` to the spec width with its fill, align and zero rules. */
export function pad(sign: string, body: string, spec: Spec, zero: boolean): string {
  const n = Array.from(sign).length + Array.from(body).length;
  if (n >= spec.width) {
    return sign + body;
  }
  const fill = spec.width - n;
  if (zero) {
    return sign + "0".repeat(fill) + body;
  }
  if (spec.align === "<") {
    return sign + body + spec.fill.repeat(fill);
  }
  if (spec.align === "^") {
    return (
      spec.fill.repeat(Math.floor(fill / 2)) + sign + body + spec.fill.repeat(fill - Math.floor(fill / 2))
    );
  }
  return spec.fill.repeat(fill) + sign + body;
}

/** The scientific body of a magnitude (`1.5e3`). */
export function sci(mag: number, precision: number): string {
  return (precision >= 0 ? mag.toExponential(precision) : mag.toExponential()).replace("e+", "e");
}

/** Render an integer (`number` or `bigint`). */
export function int(value: number | bigint, spec: Spec): string {
  const v = typeof value === "bigint" ? value : BigInt(Math.trunc(value));
  const neg = v < 0n;
  const mag = neg ? -v : v;
  let body: string;
  if (spec.kind === "x") {
    body = mag.toString(16);
  } else if (spec.kind === "b") {
    body = mag.toString(2);
  } else if (spec.kind === "o") {
    body = mag.toString(8);
  } else if (spec.kind === "e") {
    body = sci(Number(mag), spec.precision);
  } else {
    body = mag.toString() + (spec.precision > 0 ? "." + "0".repeat(spec.precision) : "");
  }
  return pad(neg ? "-" : spec.plus ? "+" : "", body, spec, spec.zero);
}

/** Render a float. */
export function float(value: number, spec: Spec): string {
  if (Number.isNaN(value)) {
    return pad("", "NaN", spec, false);
  }
  const sign = value < 0 || Object.is(value, -0) ? "-" : spec.plus ? "+" : "";
  const mag = Math.abs(value);
  if (mag === Infinity) {
    return pad(sign, "∞", spec, false);
  }
  let body: string;
  if (spec.kind === "e") {
    body = sci(mag, spec.precision);
  } else if (spec.precision >= 0) {
    body = mag.toFixed(spec.precision);
  } else {
    body = String(mag);
    if (!/[.eE]/.test(body)) {
      body += ".0";
    }
  }
  return pad(sign, body, spec, spec.zero);
}

/** Render text (precision truncates by code points). */
export function text(value: string, spec: Spec): string {
  const body = spec.precision >= 0 ? Array.from(value).slice(0, spec.precision).join("") : value;
  return pad("", body, spec, false);
}

/** Render an `instans` ISO text through a preset (`i` ISO, `d` date, `t` time). */
export function instans(iso: string, preset: string): string {
  const cut = iso.indexOf("T");
  if (preset === "d") {
    return cut < 0 ? iso : iso.slice(0, cut);
  }
  if (preset === "t") {
    return cut < 0 ? "" : iso.slice(cut + 1, cut + 9);
  }
  return iso;
}
