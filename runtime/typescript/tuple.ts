/** Composite-key maps compare tuple keys by their JSON spelling, not identity. */
const keys = new WeakMap<object, Map<string, unknown>>();

/** The canonical index must exist before Map's constructor calls set. */
export class TupleMap<K, V> extends Map<K, V> {
  private canon(): Map<string, unknown> {
    let index = keys.get(this);
    if (index === undefined) {
      index = new Map();
      keys.set(this, index);
    }
    return index;
  }

  private find(key: K): K {
    return (this.canon().get(JSON.stringify(key)) as K | undefined) ?? key;
  }

  override get(key: K): V | undefined { return super.get(this.find(key)); }
  override has(key: K): boolean { return super.has(this.find(key)); }

  override set(key: K, value: V): this {
    const id = JSON.stringify(key);
    const index = this.canon();
    if (!index.has(id)) index.set(id, key);
    return super.set(index.get(id) as K, value);
  }

  override delete(key: K): boolean {
    const found = this.find(key);
    this.canon().delete(JSON.stringify(key));
    return super.delete(found);
  }

  override clear(): void { this.canon().clear(); super.clear(); }
}
