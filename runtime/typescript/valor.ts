/** Dynamic-value carrier. Tags and payload layout are shared with display and inf. */
export type Tag = "Nihil" | "Bivalens" | "Numerus" | "Fractus" | "Textus" | "Octeti" | "Lista" | "Tabula" | "Instans";

export function box(tag: Tag, payload: any): any {
  return { __faberValorTag: tag, __faberValorPayload: payload };
}

function tagged(value: any): boolean {
  return value !== null && typeof value === "object" && typeof value.__faberValorTag === "string";
}

export function payload(value: any): any {
  return tagged(value) ? value.__faberValorPayload : value;
}

/** Widths are erased in valor: bounded integers test against i64, inf has no bound. */
export function isInteger(value: any, bounded: boolean): boolean {
  const p = tagged(value)
    ? (value.__faberValorTag === "Numerus" ? value.__faberValorPayload : undefined) : value;
  return (typeof p === "bigint" && (!bounded || (p >= -9223372036854775808n && p <= 9223372036854775807n))) ||
    (typeof p === "number" && Number.isInteger(p) && (!bounded || (p >= -9223372036854775808 && p < 9223372036854775808)));
}

/** A tag wins over the raw carrier probe, including whole-number Fractus values. */
export function is(value: any, tag: Exclude<Tag, "Numerus" | "Octeti">): boolean {
  if (tagged(value)) return value.__faberValorTag === tag;
  switch (tag) {
    case "Nihil": return value === null || value === undefined;
    case "Bivalens": return typeof value === "boolean";
    case "Fractus": return typeof value === "number" && !Number.isInteger(value);
    case "Textus": return typeof value === "string";
    case "Instans": return value !== null && typeof value === "object" && typeof value.text === "function";
    case "Lista": return Array.isArray(value) || typeof value?.planata === "function";
    case "Tabula": return value instanceof Map || (value !== null && typeof value === "object" && !Array.isArray(value) && typeof value.planata !== "function" && typeof value.densata !== "function" && typeof value.text !== "function");
  }
}
