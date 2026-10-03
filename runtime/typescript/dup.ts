/**
 * Faber TypeScript runtime — the `dup` namespace.
 *
 * Generated TypeScript files `import { dup } from "@faber/runtime"` and call
 * `dup.duplicate(value)` at a `copy` bind, a copy parameter, and the sermo
 * boundary instead of inlining the family into every emitted file (the package
 * re-exports this module as the `dup` namespace).
 *
 * The helper mirrors the former inline emission exactly: the copy produced by
 * this package is byte-identical to the compiler-inlined text.
 */

/**
 * Deep duplicate preserving class prototypes: arrays, maps, sets, typed
 * arrays, and genus instances are rebuilt; sharing and cycles inside the
 * copied graph are preserved through `seen`; primitives and functions pass
 * through.
 */
export function duplicate<T>(value: T, seen: Map<unknown, unknown> = new Map()): T {
  if (value === null || typeof value !== "object") return value;
  if (seen.has(value)) return seen.get(value) as T;
  if (ArrayBuffer.isView(value)) {
    const out = (value as unknown as { slice(): unknown }).slice();
    seen.set(value, out);
    return out as T;
  }
  if (Array.isArray(value)) {
    const out: unknown[] = [];
    seen.set(value, out);
    for (const item of value) out.push(duplicate(item, seen));
    return out as T;
  }
  if (value instanceof Map) {
    const out = new Map<unknown, unknown>();
    seen.set(value, out);
    for (const [key, item] of value) out.set(key, duplicate(item, seen));
    return out as T;
  }
  if (value instanceof Set) {
    const out = new Set<unknown>(value);
    seen.set(value, out);
    return out as T;
  }
  const out = Object.create(Object.getPrototypeOf(value)) as Record<string, unknown>;
  seen.set(value, out);
  for (const key of Object.keys(value as object)) {
    out[key] = duplicate((value as Record<string, unknown>)[key], seen);
  }
  return out as T;
}
