/** Structurally the `result.Result` generated code unwraps (kept local: no cross-module import). */
type Checked<T> = { ok: true; value: T } | { ok: false; error: string };
/** Faber tensor carrier, moved from the TypeScript emitter without changing checks or traps. */
export class Tensor<T> {
    data: T[];
    shape: number[];
    constructor(data: T[], shape: number[]) {
        this.data = data.slice();
        this.shape = shape.slice();
    }
    private static elementCount(shape: number[]): number {
        let count = 1;
        for (let axis = 0; axis < shape.length; axis++) {
            const dim = shape[axis];
            if (dim < 0) {
                throw new Error("tensor shape dimension must be non-negative");
            }
            count *= dim;
        }
        return count;
    }
    private static offset(shape: number[], indices: number[]): number | null {
        if (indices.some((idx) => idx < 0)) {
            throw new Error("tensor accipe invalid index");
        }
        if (indices.length !== shape.length) {
            return null;
        }
        let offset = 0;
        let stride = 1;
        for (let axis = shape.length - 1; axis >= 0; axis--) {
            const idx = indices[axis];
            const dim = shape[axis];
            if (idx >= dim) {
                return null;
            }
            offset += idx * stride;
            stride *= dim;
        }
        return offset;
    }
    private static broadcastShape(left: number[], right: number[]): number[] {
        const rank = Math.max(left.length, right.length);
        const shape = new Array(rank);
        for (let i = 0; i < rank; i++) {
            const l = left[left.length - rank + i] ?? 1;
            const r = right[right.length - rank + i] ?? 1;
            if (l !== r && l !== 1 && r !== 1) {
                throw new Error("tensor broadcast shape mismatch");
            }
            shape[i] = Math.max(l, r);
        }
        return shape;
    }
    // flatMap skips holes at every level; the `in` guards keep that exact behavior.
    private static flatten(source: unknown, out: any[]): void {
        if (!Array.isArray(source)) {
            out.push(source);
            return;
        }
        for (let axis = 0; axis < source.length; axis++) {
            if (axis in source) {
                Tensor.flatten(source[axis], out);
            }
        }
    }
    static empty<T>(shape: number[] = []): Tensor<T> {
        return new Tensor<T>([], shape);
    }
    static fromArray<T>(source: unknown, shape: number[], convert: (value: any) => T = (value) => value as T, fallback?: Tensor<T>): Tensor<T> {
        try {
            const data: any[] = [];
            Tensor.flatten(source, data);
            for (let i = 0; i < data.length; i++) {
                data[i] = convert(data[i]);
            }
            if (Tensor.elementCount(shape) !== data.length) {
                throw new Error("tensor conversio element count does not match shape");
            }
            return new Tensor<T>(data, shape);
        } catch (error) {
            if (fallback === undefined) {
                throw error;
            }
            return fallback;
        }
    }
    crea(fill: T, shape: number[]): Tensor<T> {
        return new Tensor<T>(new Array(Tensor.elementCount(shape)).fill(fill), shape);
    }
    strue(data: T[], shape: number[]): Tensor<T> {
        if (Tensor.elementCount(shape) !== data.length) {
            throw new Error("tensor structa element count does not match shape");
        }
        return new Tensor<T>(data, shape);
    }
    longitudo(): number {
        return this.shape.length;
    }
    magnitudines(): Array<number> {
        return this.shape.slice();
    }
    forma(shape: number[]): Tensor<T> {
        if (Tensor.elementCount(shape) !== this.data.length) {
            throw new Error("tensor forma (reshape) element count mismatch");
        }
        return new Tensor<T>(this.data, shape);
    }
    accipe(indices: number[]): T | null {
        const offset = Tensor.offset(this.shape, indices);
        return offset == null ? null : this.data[offset];
    }
    /** The failable method form `t.accipe(idx)`: the element, or the runner's text error. */
    accipeChecked(indices: number[]): Checked<T> {
        const offset = Tensor.offset(this.shape, indices);
        if (offset == null) {
            return { ok: false, error: "tensor accipe invalid index" };
        }
        return { ok: true, value: this.data[offset] };
    }
    /** The failable method form `t.ponde(idx, v)`: stores, or the runner's text error. */
    pondeChecked(indices: number[], value: T): Checked<void> {
        const offset = Tensor.offset(this.shape, indices);
        if (offset == null) {
            return { ok: false, error: "tensor ponde invalid index" };
        }
        this.data[offset] = value;
        return { ok: true, value: undefined };
    }
    ponde(indices: number[], value: T): void {
        const offset = Tensor.offset(this.shape, indices);
        if (offset == null) {
            throw new Error("tensor ponde invalid index");
        }
        this.data[offset] = value;
    }
    reple(value: T): void {
        this.data.fill(value);
    }
    planata(): T[] {
        return this.data.slice();
    }
    sectio(start: number, end: number): Tensor<T> {
        if (start < 0 || end < 0 || end < start) {
            throw new Error("tensor sectio invalid slice bounds");
        }
        const inner = Tensor.elementCount(this.shape.slice(1));
        const shape = [end - start, ...this.shape.slice(1)];
        return new Tensor<T>(this.data.slice(start * inner, end * inner), shape);
    }
    materialize(): Tensor<T> {
        return new Tensor<T>(this.data, this.shape);
    }
    private elementwise(other: Tensor<T>, op: (left: any, right: any) => any): Tensor<T> {
        const shape = Tensor.broadcastShape(this.shape, other.shape);
        const data = new Array(Tensor.elementCount(shape));
        const rank = shape.length;
        if (this.shape.length === rank && other.shape.length === rank &&
            this.shape.every((dim, axis) => dim === shape[axis]) &&
            other.shape.every((dim, axis) => dim === shape[axis])) {
            for (let i = 0; i < data.length; i++) {
                data[i] = op(this.data[i], other.data[i]);
            }
            return new Tensor<T>(data as T[], shape);
        }
        // Per-result-axis source strides; 0 marks a stretched (broadcast) axis.
        const leftStrides = new Array(rank);
        const rightStrides = new Array(rank);
        for (let axis = 0; axis < rank; axis++) {
            let stride = 1;
            for (let inner = axis + 1; inner < rank; inner++) {
                stride *= shape[inner];
            }
            const leftAxis = axis - (rank - this.shape.length);
            leftStrides[axis] = leftAxis >= 0 && this.shape[leftAxis] !== 1 ? stride : 0;
            const rightAxis = axis - (rank - other.shape.length);
            rightStrides[axis] = rightAxis >= 0 && other.shape[rightAxis] !== 1 ? stride : 0;
        }
        const counters = new Array(rank).fill(0);
        let left = 0;
        let right = 0;
        for (let i = 0; i < data.length; i++) {
            data[i] = op(this.data[left], other.data[right]);
            for (let axis = rank - 1; axis >= 0; axis--) {
                if (++counters[axis] < shape[axis]) {
                    left += leftStrides[axis];
                    right += rightStrides[axis];
                    break;
                }
                counters[axis] = 0;
                left -= leftStrides[axis] * (shape[axis] - 1);
                right -= rightStrides[axis] * (shape[axis] - 1);
            }
        }
        return new Tensor<T>(data as T[], shape);
    }
    addita(other: Tensor<T>): Tensor<T> {
        return this.elementwise(other, (left, right) => left + right);
    }
    subtrahe(other: Tensor<T>): Tensor<T> {
        return this.elementwise(other, (left, right) => left - right);
    }
    multiplica(other: Tensor<T>): Tensor<T> {
        return this.elementwise(other, (left, right) => left * right);
    }
    summa(): T {
        return this.data.reduce((total: any, value: any) => total + value, 0) as T;
    }
    media(): number {
        return this.data.length === 0 ? 0 : this.data.reduce((total: any, value: any) => total + value, 0) / this.data.length;
    }
    matmul(other: Tensor<T>): Tensor<T> {
        if (this.shape.length !== 2 || other.shape.length !== 2) {
            throw new Error("matmul requires rank-2 tensor");
        }
        const [m, k1] = this.shape;
        const [k2, n] = other.shape;
        if (k1 !== k2) {
            throw new Error("matmul inner dimension mismatch");
        }
        const data = new Array(m * n).fill(0);
        for (let row = 0; row < m; row++) {
            const leftBase = row * k1;
            const outBase = row * n;
            for (let k = 0; k < k1; k++) {
                const left = this.data[leftBase + k] as any;
                const rightBase = k * n;
                for (let column = 0; column < n; column++) {
                    data[outBase + column] += left * (other.data[rightBase + column] as any);
                }
            }
        }
        return new Tensor<T>(data as T[], [m, n]);
    }
    transpone(): Tensor<T> {
        if (this.shape.length <= 1) {
            return new Tensor<T>(this.data, this.shape);
        }
        if (this.shape.length !== 2) {
            throw new Error("transpone requires rank-1 or rank-2 tensor");
        }
        const [rows, cols] = this.shape;
        const data = new Array(rows * cols);
        for (let col = 0; col < cols; col++) {
            for (let row = 0; row < rows; row++) {
                data[col * rows + row] = this.data[row * cols + col];
            }
        }
        return new Tensor<T>(data as T[], [cols, rows]);
    }
    activatio_softmax(): Tensor<T> {
        if (this.data.length === 0 || this.shape.length === 0) {
            throw new Error("softmax empty tensor");
        }
        const lastDim = this.shape[this.shape.length - 1];
        const batch = this.data.length / lastDim;
        const data = new Array(this.data.length);
        const row = new Float64Array(lastDim);
        for (let b = 0; b < batch; b++) {
            const base = b * lastDim;
            let maxVal = -Infinity;
            for (let i = 0; i < lastDim; i++) {
                const value = Number(this.data[base + i]);
                if (!Number.isFinite(value)) {
                    throw new Error("softmax non-finite input");
                }
                if (value > maxVal) maxVal = value;
                row[i] = value;
            }
            let expSum = 0;
            for (let i = 0; i < lastDim; i++) {
                const expVal = Math.exp(row[i] - maxVal);
                row[i] = expVal;
                expSum += expVal;
            }
            for (let i = 0; i < lastDim; i++) {
                data[base + i] = (row[i] / expSum) as T;
            }
        }
        return new Tensor<T>(data as T[], this.shape);
    }
}
