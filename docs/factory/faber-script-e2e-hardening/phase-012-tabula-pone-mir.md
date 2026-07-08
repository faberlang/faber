# Phase 012 - Tabula `pone` MIR Runtime Op

## Target

`../radix/crates/exempla/corpus/tabula/methodi-accessus.fab` fails in script mode on
two `puncta.pone(key, value)` calls:

```text
unsupported MIR lowering: method call before runtime/provider MIR lowering
unsupported MIR lowering: method call before runtime/provider MIR lowering
```

The remaining `tabula` access methods in the fixture already map to MIR
collection ops: `accipe`, `habet`, `dele`, `longitudo`, and `vacua`.

## Invariant

`tabula<K,V>.pone(key, value)` is the canonical in-place map setter. MIR should
represent it as one narrow collection runtime op that mutates the existing map,
validates the key/value types, and returns `vacuum`.

## Scope

- Promote the existing `tabula.pone` registry row from codegen-only to a MIR
  collection runtime op.
- Validate receiver shape, key type, value type, and `vacuum` result.
- Implement script-stepper evaluation by inserting the key/value pair into the
  existing `Value::Map`.
- Add focused MIR lowering and stepper tests for the access fixture.
- Inspect S-expression, Wasm, and LLVM MIR-boundary emitters and add only the
  existing runtime-call name plumbing where applicable.

## Out of Scope

- Additional map utility names such as `accipeAut`, `claves`, or `valores`.
- Cursor/view APIs for maps.
- Changing `tabula` target codegen behavior outside MIR-boundary plumbing.

## Validation

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix <focused filters>`
- `cargo run -- run
  ../radix/crates/exempla/corpus/tabula/methodi-accessus.fab`
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture`
- Targeted S-expression, Wasm, and LLVM `radix emit` checks for
  `../radix/crates/exempla/corpus/tabula/methodi-accessus.fab`, recording any backend
  gaps.
