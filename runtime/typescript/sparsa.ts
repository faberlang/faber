/** Faber sparsa carrier, moved from the TypeScript emitter without changing checks or traps. */
// @ts-ignore TS5097: Node loads the source extension; consumer tsc does not.
import * as tensor from "./tensor.ts";

export class Sparsa<T> {
    shape: number[];
    entries: Map<number, T>;
    defaultValue: T;
    constructor(shape: number[], entries: Map<string, T> | Map<number, T> = new Map(), defaultValue: T = 0 as T) {
        if (shape.some((dim) => dim < 0)) {
            throw new Error("sparsa shape dimension must be non-negative");
        }
        this.shape = shape.slice();
        // Keys are flat row-major offsets; legacy string keys (JSON coordinates) normalize here.
        this.entries = new Map<number, T>();
        for (const [key, value] of entries) {
            this.entries.set(typeof key === "string" ? Sparsa.offset(shape, JSON.parse(key) as number[]) : key, value);
        }
        this.defaultValue = defaultValue;
    }
    private static elementCount(shape: number[]): number {
        if (shape.some((dim) => dim < 0)) {
            throw new Error("sparsa shape dimension must be non-negative");
        }
        return shape.reduce((total, dim) => total * dim, 1);
    }
    private static offset(shape: number[], indices: number[]): number {
        let offset = 0;
        let stride = 1;
        for (let axis = shape.length - 1; axis >= 0; axis--) {
            offset += indices[axis] * stride;
            stride *= shape[axis];
        }
        return offset;
    }
    static empty<T>(shape: number[] = [], defaultValue: T = 0 as T): Sparsa<T> {
        return new Sparsa<T>(shape, new Map(), defaultValue);
    }
    static fromTensor<T>(dense: tensor.Tensor<T>, defaultValue: T = 0 as T): Sparsa<T> {
        const entries = new Map<number, T>();
        const data = dense.data;
        for (let i = 0; i < data.length; i++) {
            const value = data[i];
            if (value !== defaultValue) {
                entries.set(i, value);
            }
        }
        return new Sparsa<T>(dense.shape, entries, defaultValue);
    }
    private validate(indices: number[]): void {
        if (indices.length !== this.shape.length) {
            throw new Error("sparsa index rank does not match shape rank");
        }
        for (let axis = 0; axis < indices.length; axis++) {
            const idx = indices[axis];
            if (idx < 0) {
                throw new Error("sparsa index must be non-negative");
            }
            if (idx >= this.shape[axis]) {
                throw new Error("sparsa index out of bounds");
            }
        }
    }
    longitudo(): number {
        return this.shape.length;
    }
    magnitudines(): Array<number> {
        return this.shape.slice();
    }
    nonnihil(): number {
        return this.entries.size;
    }
    accipe(indices: number[]): T {
        this.validate(indices);
        return this.entries.get(Sparsa.offset(this.shape, indices)) ?? this.defaultValue;
    }
    ponde(indices: number[], value: T): void {
        this.validate(indices);
        const key = Sparsa.offset(this.shape, indices);
        if (value === this.defaultValue) {
            this.entries.delete(key);
        } else {
            this.entries.set(key, value);
        }
    }
    densata(): tensor.Tensor<T> {
        const data = new Array(Sparsa.elementCount(this.shape)).fill(this.defaultValue);
        for (const [key, value] of this.entries) {
            data[key] = value;
        }
        return new tensor.Tensor<T>(data, this.shape);
    }
}
