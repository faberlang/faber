/**
 * Faber TypeScript runtime — display helper family.
 *
 * Extracted from the radix-hir-ts emit surface (faber-target-runtime S4-U1).
 * Generated TypeScript files import `__faberDisplay` from this package
 * (`@faber/runtime`, inventory §7 default identity) instead of inlining the
 * family into every emitted file.
 *
 * The helpers mirror the former inline emission exactly: display formatting
 * produced by this package is byte-identical to the compiler-inlined text.
 */

/**
 * Display hints select how a value renders for `nota`/`scriptum` output.
 * String hints cover the primitive carriers; object hints add collection
 * shape (nullable, list, tensor, sparse, map, set).
 */
export type FaberDisplayHint =
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
  | { kind: "nullable"; inner: FaberDisplayHint }
  | { kind: "lista"; element: FaberDisplayHint }
  | { kind: "tensor"; element: FaberDisplayHint }
  | { kind: "sparsa"; element: FaberDisplayHint }
  | { kind: "map"; key: FaberDisplayHint; value: FaberDisplayHint }
  | { kind: "set"; element: FaberDisplayHint };

/** Render any value under a display hint. */
export function __faberDisplay(value: any, hint: FaberDisplayHint = "unknown"): string {
  if (typeof hint === "object") {
    if (hint.kind === "nullable") {
      return value === null || value === undefined ? "nihil" : __faberDisplay(value, hint.inner);
    }
    if (hint.kind === "lista") {
      return __faberDisplayList(value, hint.element);
    }
    if (hint.kind === "tensor") {
      return __faberDisplayTensor(value, hint.element);
    }
    if (hint.kind === "sparsa") {
      return __faberDisplaySparsa(value, hint.element);
    }
    if (hint.kind === "map") {
      return __faberDisplayMap(value, hint.key, hint.value);
    }
    return __faberDisplayList(value, hint.element);
  }
  if (value === null || value === undefined) {
    return hint === "vacuum" ? "vacuum" : "nihil";
  }
  switch (hint) {
    case "bivalens":
      return value ? "verum" : "falsum";
    case "fractus":
      return __faberDisplayFractus(value);
    case "nihil":
      return "nihil";
    case "vacuum":
      return "vacuum";
    case "valor":
      return __faberDisplayValor(value);
    case "regex":
      return String(value);
    case "textus":
    case "ascii":
    case "instans":
    case "numerus":
      return String(value);
    default:
      return __faberDisplayValor(value);
  }
}

/** Render a fractus (fraction) as text. */
export function __faberDisplayFractus(value: any): string {
  const n = Number(value);
  if (!Number.isFinite(n)) {
    return String(value);
  }
  return Number.isInteger(n) ? `${n}.0` : String(n);
}

/** Render a list/array under an element hint. */
export function __faberDisplayList(value: any, element: FaberDisplayHint): string {
  if (value === null || value === undefined) {
    return "nihil";
  }
  return `[${Array.from(value as Array<any>).map((item) => __faberDisplay(item, element)).join(", ")}]`;
}

/** Render a dense tensor under an element hint. */
export function __faberDisplayTensor(value: any, element: FaberDisplayHint): string {
  if (value === null || value === undefined) {
    return "nihil";
  }
  if (typeof value.planata === "function") {
    return __faberDisplayList(value.planata(), element);
  }
  if (Array.isArray(value.data)) {
    return __faberDisplayList(value.data, element);
  }
  return __faberDisplayValor(value);
}

/** Render a sparse tensor under an element hint. */
export function __faberDisplaySparsa(value: any, element: FaberDisplayHint): string {
  if (value === null || value === undefined) {
    return "nihil";
  }
  if (typeof value.densata === "function") {
    return __faberDisplayTensor(value.densata(), element);
  }
  return __faberDisplayValor(value);
}

/** Render a map/record under key and value hints. */
export function __faberDisplayMap(
  value: any,
  keyHint: FaberDisplayHint,
  valueHint: FaberDisplayHint,
): string {
  if (value === null || value === undefined) {
    return "nihil";
  }
  const entries = value instanceof Map ? Array.from(value.entries()) : Object.entries(value);
  return `{${entries.map(([key, item]) => `${JSON.stringify(__faberDisplay(key, keyHint))}: ${__faberDisplay(item, valueHint)}`).join(", ")}}`;
}

/** Render a Faber valor (dynamic value carrier) as text. */
export function __faberDisplayValor(value: any): string {
  if (
    value !== null &&
    typeof value === "object" &&
    typeof value.__faberValorTag === "string"
  ) {
    return __faberDisplayTaggedValor(value);
  }
  if (value === null || value === undefined) {
    return "nihil";
  }
  if (typeof value === "boolean") {
    return value ? "verum" : "falsum";
  }
  if (typeof value === "number") {
    return String(value);
  }
  if (typeof value === "string") {
    return value;
  }
  if (Array.isArray(value)) {
    return __faberDisplayList(value, "valor");
  }
  if (typeof value.text === "function") {
    return String(value.text());
  }
  if (typeof value.toString === "function" && value.toString !== Object.prototype.toString) {
    return String(value);
  }
  return __faberDisplayMap(value, "textus", "valor");
}

/**
 * Unbox valor tags so a nota of valor prints `42` / `[1, 2]` / `{…}` rather
 * than `Numerus(42)` / `Lista([...])` (mirrors Faber/rust `display_valor`).
 */
export function __faberDisplayTaggedValor(value: any): string {
  const payload = value.__faberValorPayload;
  switch (value.__faberValorTag) {
    case "Nihil":
      return "nihil";
    case "Bivalens":
      return payload ? "verum" : "falsum";
    case "Numerus":
      return String(payload);
    case "Fractus":
      return __faberDisplayFractus(payload);
    case "Textus":
      return String(payload);
    case "Instans":
      return String(payload);
    case "Octeti":
      return `<${Array.isArray(payload) ? payload.length : 0} bytes>`;
    case "Lista":
      return __faberDisplayList(payload, "valor");
    case "Tabula":
      return __faberDisplayMap(payload, "textus", "valor");
    default:
      return __faberDisplayValor(payload);
  }
}
