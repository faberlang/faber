/** Faber matrix carrier, moved from the TypeScript emitter without changing checks or traps. */
export class Matrix<T> {
    readonly data: T[];
    readonly shape: number[];
    constructor(data: T[], shape: number[]) {
        this.data = data.slice();
        this.shape = shape.slice();
    }
    addita(other: Matrix<T>): Matrix<T> {
        if (this.shape.length !== other.shape.length || this.shape.some((dim, idx) => dim !== other.shape[idx])) {
            throw new Error("matrix addita shape mismatch");
        }
        const data = new Array<T>(this.data.length);
        for (let idx = 0; idx < data.length; idx++) {
            if (idx in this.data) data[idx] = ((this.data[idx] as any) + (other.data[idx] as any)) as T; // map preserves holes
        }
        return new Matrix<T>(data as T[], this.shape);
    }
    subtrahe(other: Matrix<T>): Matrix<T> {
        if (this.shape.length !== other.shape.length || this.shape.some((dim, idx) => dim !== other.shape[idx])) {
            throw new Error("matrix subtrahe shape mismatch");
        }
        const data = new Array<T>(this.data.length);
        for (let idx = 0; idx < data.length; idx++) {
            if (idx in this.data) data[idx] = ((this.data[idx] as any) - (other.data[idx] as any)) as T; // map preserves holes
        }
        return new Matrix<T>(data as T[], this.shape);
    }
    accipe(indices: number[]): T {
        if (indices.length !== this.shape.length || indices.some((idx, axis) => idx < 0 || idx >= this.shape[axis])) {
        throw new Error("matrix accipe invalid index");
        }
        let offset = 0;
        let stride = 1;
        for (let axis = this.shape.length - 1; axis >= 0; axis--) {
        offset += indices[axis] * stride;
        stride *= this.shape[axis];
        }
        return this.data[offset];
    }
    applica(vector: T[]): T[] {
        if (this.shape.length !== 2 || vector.length !== this.shape[1]) {
            throw new Error("matrix applica shape mismatch");
        }
        const rows = this.shape[0];
        const columns = this.shape[1];
        const result = new Array<T>(rows);
        for (let row = 0; row < rows; row++) {
            let total: any = 0;
            for (let column = 0; column < columns; column++) {
                total += (this.data[row * columns + column] as any) * (vector[column] as any);
            }
            result[row] = total as T;
        }
        return result;
    }
    [Symbol.iterator](): Iterator<T> {
        return this.data[Symbol.iterator]();
    }
}
