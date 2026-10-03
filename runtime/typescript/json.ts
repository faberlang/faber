/** Structural JSON carrier and wire codecs. Big-int codecs preserve inf tokens. */
export type Value = null | boolean | number | string | Value[] | { [key: string]: Value };

export function parse(text: string): any {
  return JSON.parse(text);
}

export function stringify(value: any): string {
  return JSON.stringify(value);
}

/** Integer tokens beyond 2^53 retain their original digits as bigint. */
export function parseBigInt(text: string): any {
  return JSON.parse(text, (_key: string, value: any, context?: { source?: string }) =>
    typeof value === "number" && context?.source !== undefined &&
    /^-?[0-9]+$/.test(context.source) && !Number.isSafeInteger(value)
      ? BigInt(context.source) : value);
}

/** Big integers are bare JSON numbers, never quoted strings. */
export function stringifyBigInt(value: any): string {
  return JSON.stringify(value, (_key: string, item: any) =>
    typeof item === "bigint" ? (JSON as any).rawJSON(item.toString()) : item);
}
