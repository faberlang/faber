/**
 * Faber TypeScript runtime — the `display` namespace.
 *
 * Generated TypeScript files `import { display } from "@faber/runtime"` and
 * call `display.value(x, hint)` instead of inlining the family into every
 * emitted file (the package re-exports this module as the `display`
 * namespace).
 *
 * The helpers mirror the former inline emission exactly: display formatting
 * produced by this package is byte-identical to the compiler-inlined text.
 * Only the exported names changed (`__faberDisplay` is `display.value`,
 * `__faberDisplay<Name>` is `display.<name>`).
 */

/**
 * Display hints select how a value renders for `nota`/`scriptum` output.
 * String hints cover the primitive carriers; object hints add collection
 * shape (nullable, list, tensor, sparse, map, set).
 */
export type DisplayHint =
  | "unknown"
  | "textus"
  | "ascii"
  | "numerus"
  | "fractus"
  | "bivalens"
  | "nihil"
  | "vacuum"
  | "valor"
  | "instans"
  | "regex"
  | { kind: "nullable"; inner: DisplayHint }
  | { kind: "lista"; element: DisplayHint }
  | { kind: "tensor"; element: DisplayHint }
  | { kind: "sparsa"; element: DisplayHint }
  | { kind: "map"; key: DisplayHint; value: DisplayHint }
  | { kind: "set"; element: DisplayHint };

/** Render any value under a display hint. */
export function value(subject: any, hint: DisplayHint = "unknown"): string {
  if (typeof hint === "object") {
    if (hint.kind === "nullable") {
      return subject === null || subject === undefined ? "nihil" : value(subject, hint.inner);
    }
    if (hint.kind === "lista") {
      return list(subject, hint.element);
    }
    if (hint.kind === "tensor") {
      return tensor(subject, hint.element);
    }
    if (hint.kind === "sparsa") {
      return sparsa(subject, hint.element);
    }
    if (hint.kind === "map") {
      return map(subject, hint.key, hint.value);
    }
    return list(subject, hint.element);
  }
  if (subject === null || subject === undefined) {
    return hint === "vacuum" ? "vacuum" : "nihil";
  }
  switch (hint) {
    case "bivalens":
      return subject ? "verum" : "falsum";
    case "fractus":
      return fractus(subject);
    case "nihil":
      return "nihil";
    case "vacuum":
      return "vacuum";
    case "valor":
      return valor(subject);
    case "regex":
      return String(subject);
    case "textus":
    case "ascii":
    case "instans":
      return String(subject);
    case "numerus":
      return numerus(subject);
    default:
      return valor(subject);
  }
}

/**
 * Render an integer carried as a JS number. Beyond 2^53 - 1 `String(value)`
 * falls back to shortest round-trip digits padded with zeros (`-2^63` prints
 * `-9223372036854776000`); `BigInt(value).toString()` prints the exact integer
 * the number holds.
 */
export function numerus(subject: any): string {
  if (typeof subject === "number" && Number.isInteger(subject) && Math.abs(subject) > Number.MAX_SAFE_INTEGER) {
    return BigInt(subject).toString();
  }
  return String(subject);
}

/**
 * Render an f64 fractus as text, byte-identical to the MIR runner and the Rust
 * runtime: a float with a zero fraction prints its exact decimal expansion with
 * one fraction digit (`18446744073709551616.0`, `-0.0`); every other finite
 * value prints its shortest round-trip digits in plain positional form, never
 * an exponent; infinities print `inf` / `-inf`; NaN prints `NaN`.
 */
export function fractus(subject: any): string {
  const n = Number(subject);
  if (Number.isNaN(n)) {
    return "NaN";
  }
  if (!Number.isFinite(n)) {
    return n < 0 ? "-inf" : "inf";
  }
  if (Object.is(n, -0)) {
    return "-0.0";
  }
  if (Number.isInteger(n)) {
    return `${BigInt(n)}.0`;
  }
  // JS prints a non-integral number with an exponent only below 1e-6.
  const text = String(n);
  const small = /^(-?)(\d)(?:\.(\d+))?e-(\d+)$/.exec(text);
  if (small === null) {
    return text;
  }
  return `${small[1]}0.${"0".repeat(Number(small[4]) - 1)}${small[2]}${small[3] ?? ""}`;
}

/** Render a list/array under an element hint. */
export function list(subject: any, element: DisplayHint): string {
  if (subject === null || subject === undefined) {
    return "nihil";
  }
  return `[${Array.from(subject as Array<any>).map((item) => value(item, element)).join(", ")}]`;
}

/** Render a dense tensor under an element hint. */
export function tensor(subject: any, element: DisplayHint): string {
  if (subject === null || subject === undefined) {
    return "nihil";
  }
  if (typeof subject.planata === "function") {
    return list(subject.planata(), element);
  }
  if (Array.isArray(subject.data)) {
    return list(subject.data, element);
  }
  return valor(subject);
}

/** Render a sparse tensor under an element hint. */
export function sparsa(subject: any, element: DisplayHint): string {
  if (subject === null || subject === undefined) {
    return "nihil";
  }
  if (typeof subject.densata === "function") {
    return tensor(subject.densata(), element);
  }
  return valor(subject);
}

/** Render a map/record under key and value hints. */
export function map(
  subject: any,
  keyHint: DisplayHint,
  valueHint: DisplayHint,
): string {
  if (subject === null || subject === undefined) {
    return "nihil";
  }
  const entries = subject instanceof Map ? Array.from(subject.entries()) : Object.entries(subject);
  return `{${entries.map(([key, item]) => `${JSON.stringify(value(key, keyHint))}: ${value(item, valueHint)}`).join(", ")}}`;
}

/** Render a Faber valor (dynamic value carrier) as text. */
export function valor(subject: any): string {
  if (
    subject !== null &&
    typeof subject === "object" &&
    typeof subject.__faberValorTag === "string"
  ) {
    return taggedValor(subject);
  }
  if (subject === null || subject === undefined) {
    return "nihil";
  }
  if (typeof subject === "boolean") {
    return subject ? "verum" : "falsum";
  }
  if (typeof subject === "number") {
    // A bare JS number is a numerus unless it can only be a float (fractional,
    // non-finite or beyond the integer range).
    return Number.isInteger(subject) && Math.abs(subject) < 1e21
      ? String(subject)
      : fractus(subject);
  }
  if (typeof subject === "string") {
    return subject;
  }
  if (Array.isArray(subject)) {
    return list(subject, "valor");
  }
  if (typeof subject.text === "function") {
    return String(subject.text());
  }
  if (typeof subject.toString === "function" && subject.toString !== Object.prototype.toString) {
    return String(subject);
  }
  return map(subject, "textus", "valor");
}

/**
 * Unbox valor tags so a nota of valor prints `42` / `[1, 2]` / `{…}` rather
 * than `Numerus(42)` / `Lista([...])` (mirrors Faber/rust `display_valor`).
 */
export function taggedValor(subject: any): string {
  const payload = subject.__faberValorPayload;
  switch (subject.__faberValorTag) {
    case "Nihil":
      return "nihil";
    case "Bivalens":
      return payload ? "verum" : "falsum";
    case "Numerus":
      return String(payload);
    case "Fractus":
      return fractus(payload);
    case "Textus":
      return String(payload);
    case "Instans":
      return String(payload);
    case "Octeti":
      return `<${Array.isArray(payload) ? payload.length : 0} bytes>`;
    case "Lista":
      return list(payload, "valor");
    case "Tabula":
      return map(payload, "textus", "valor");
    default:
      return valor(payload);
  }
}
