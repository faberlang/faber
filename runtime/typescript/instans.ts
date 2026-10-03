export type InstansPrecision = "s" | "ms" | "us" | "ns";

export class Instans {
  private readonly epochMillis: number;
  private readonly fractionNanos: string;
  private readonly precision: InstansPrecision;

  private constructor(epochMillis: number, fractionNanos: string, precision: InstansPrecision) {
    const padded = fractionNanos.padEnd(9, "0").slice(0, 9);
    if (precision === "s") {
      this.epochMillis = Math.trunc(epochMillis / 1000) * 1000;
      this.fractionNanos = "000000000";
    } else if (precision === "ms") {
      this.epochMillis = Math.trunc(epochMillis);
      this.fractionNanos = `${String(new Date(epochMillis).getUTCMilliseconds()).padStart(3, "0")}000000`;
    } else if (precision === "us") {
      this.epochMillis = Math.trunc(epochMillis);
      this.fractionNanos = `${padded.slice(0, 6)}000`;
    } else {
      this.epochMillis = Math.trunc(epochMillis);
      this.fractionNanos = padded;
    }
    this.precision = precision;
  }

  static parse(value: unknown, precision: InstansPrecision = "s", fallback?: Instans): Instans {
    if (value instanceof Instans) return value.withPrecision(precision);
    const text = String(value);
    const match = text.match(/^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(?:\.(\d{1,9}))?(Z|[+-]\d{2}:?\d{2})$/);
    if (!match) {
      if (fallback !== undefined) return fallback;
      throw new Error("instans conversio failed");
    }
    const zone = match[3] === "Z" || match[3].includes(":") ? match[3] : `${match[3].slice(0, 3)}:${match[3].slice(3)}`;
    const millis = Date.parse(`${match[1]}${match[2] ? `.${match[2].slice(0, 3).padEnd(3, "0")}` : ""}${zone}`);
    if (Number.isNaN(millis)) {
      if (fallback !== undefined) return fallback;
      throw new Error("instans conversio failed");
    }
    return new Instans(millis, match[2] ?? "", precision);
  }

  static fromEpoch(value: number | bigint, precision: InstansPrecision = "s", fallback?: Instans): Instans {
    const max = 9223372036854775807n;
    const min = -max - 1n;
    const epoch = BigInt(value);
    if (epoch > max || epoch < min) {
      if (fallback !== undefined) return fallback;
      throw new Error("numerus to instans conversion out of range");
    }
    const unit = precision === "s" ? 1000000000n : precision === "ms" ? 1000000n : precision === "us" ? 1000n : 1n;
    let nanos = epoch * unit;
    if (nanos > max) nanos = max;
    else if (nanos < min) nanos = min;
    let seconds = nanos / 1000000000n;
    let fraction = nanos % 1000000000n;
    if (fraction < 0n) { seconds -= 1n; fraction += 1000000000n; }
    return new Instans(Number(seconds) * 1000 + Math.floor(Number(fraction) / 1000000), String(fraction).padStart(9, "0"), precision);
  }

  withPrecision(precision: InstansPrecision): Instans {
    return new Instans(this.epochMillis, this.fractionNanos, precision);
  }

  equals(other: Instans): boolean { return this.key() === other.key(); }
  valueOf(): number { return this.epochMillis; }
  toString(): string { return this.text(); }

  text(): string {
    const base = new Date(this.epochMillis);
    const whole = base.toISOString().slice(0, 19);
    if (this.precision === "s") return `${whole}Z`;
    if (this.precision === "ms") return `${whole}.${String(base.getUTCMilliseconds()).padStart(3, "0")}Z`;
    if (this.precision === "us") return `${whole}.${this.fractionNanos.slice(0, 6)}Z`;
    return `${whole}.${this.fractionNanos}Z`;
  }

  private key(): string {
    if (this.precision === "s") return `${Math.trunc(this.epochMillis / 1000)}`;
    if (this.precision === "ms") return `${Math.trunc(this.epochMillis)}`;
    if (this.precision === "us") return `${Math.trunc(this.epochMillis)}:${this.fractionNanos.slice(3, 6)}`;
    return `${Math.trunc(this.epochMillis)}:${this.fractionNanos.slice(3, 9)}`;
  }
}
