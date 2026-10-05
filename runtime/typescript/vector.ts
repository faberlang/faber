/** Faber vector normalization, without augmenting Array.prototype. */
export function normalize<T>(values: T[]): T[] {
    let sum = 0;
    for (let i = 0; i < values.length; i++) {
        if (!(i in values)) continue; // reduce skips holes; keep that exact behavior
        const value = Number(values[i]);
        sum += value * value;
    }
    const length = Math.sqrt(sum);
    const result = new Array(values.length);
    if (length <= 0.000001) {
        for (let i = 0; i < result.length; i++) {
            if (i in values) result[i] = 0 as T; // map preserves holes
        }
        return result;
    }
    for (let i = 0; i < result.length; i++) {
        if (i in values) result[i] = (Number(values[i]) / length) as T;
    }
    return result;
}
