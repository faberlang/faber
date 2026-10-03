export class Intervallum<T extends number> {
  readonly start: T;
  readonly end: T;
  readonly inclusive: boolean;
  readonly step: number;

  constructor(start: T, end: T, inclusive = false, step = 1) {
    this.start = start;
    this.end = end;
    this.inclusive = inclusive;
    this.step = step;
  }

  private exclusiveEnd(): number {
    return this.inclusive ? this.end + 1 : this.end;
  }

  longitudo(): number {
    return this.toArray().length;
  }

  continet(value: T): boolean {
    return value >= this.start && (this.inclusive ? value <= this.end : value < this.end);
  }

  inter(other: Intervallum<T>): Intervallum<T> | null {
    const start = Math.max(this.start, other.start) as T;
    const endExclusive = Math.min(this.exclusiveEnd(), other.exclusiveEnd());
    return endExclusive <= start ? null : new Intervallum<T>(start, endExclusive as T, false);
  }

  union(other: Intervallum<T>): Intervallum<T> | null {
    const start = Math.min(this.start, other.start) as T;
    const endExclusive = Math.max(this.exclusiveEnd(), other.exclusiveEnd());
    if (Math.max(this.start, other.start) > Math.min(this.exclusiveEnd(), other.exclusiveEnd())) return null;
    return new Intervallum<T>(start, endExclusive as T, false);
  }

  toArray(): T[] {
    const out: T[] = [];
    const stride = this.step;
    if (!(stride > 0)) throw new Error("range step must be positive");
    if (this.start <= this.end) {
      for (let value: number = this.start; this.inclusive ? value <= this.end : value < this.end; value += stride) out.push(value as T);
    } else {
      for (let value: number = this.start; this.inclusive ? value >= this.end : value > this.end; value -= stride) out.push(value as T);
    }
    return out;
  }

  [Symbol.iterator](): Iterator<T> {
    return this.toArray()[Symbol.iterator]();
  }

  static coerce(value: any, range: Intervallum<number>): any {
    if (value instanceof Intervallum) {
      const start = Math.max(value.start, range.start);
      const endExclusive = Math.min(value.inclusive ? value.end + 1 : value.end, range.inclusive ? range.end + 1 : range.end);
      const end = range.inclusive ? endExclusive - 1 : endExclusive;
      return new Intervallum(start, end, range.inclusive);
    }
    const upper = range.inclusive ? range.end : range.end - 1;
    return Math.min(Math.max(value, range.start), upper);
  }
}
