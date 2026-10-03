/** Keep tagged objects plain, with a non-enumerable construction-literal display. */
export function value<T extends object>(value: T, display: () => string): T {
  return Object.defineProperty(value, "toString", { value: display });
}
