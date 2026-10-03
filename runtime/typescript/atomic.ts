/** Structural atomic-cell contract. The host supplies the concrete cell. */
export interface Atomic<T> {
  load(): T;
  store(value: T): void;
  exchange(value: T): T;
  compare_exchange(oldValue: T, newValue: T): boolean;
}
