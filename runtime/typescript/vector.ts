/** Faber vector normalization, without augmenting Array.prototype. */
export function normalize<T>(values: T[]): T[] {
    const length = Math.sqrt(values.reduce((sum: number, value: T) => sum + Number(value) * Number(value), 0));
    if (length <= 0.000001) { return values.map(() => 0 as T); }
    return values.map((value) => (Number(value) / length) as T);
}
