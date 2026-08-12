# Public Faber Repository

This repository owns Faber's public project surface and generated-language API
packages. It does not own compiler or `faber` CLI implementation.

## Ownership

- `runtime/{rust,typescript,go,swift}`: generated-language support packages.
- `packages/`: public Faber packages.
- `crates/http-transport`: public Rust transport package.
- `README.md`, `catalog.json`, `docs/`: public routing and policy.
- Private compiler, CLI, product tests, release control, and product fixtures:
  `faberlang/radix`.
- Concrete host effects and device sessions: `faberlang/hosts`.
- Package-store mutation and lock writing: `faberlang/cista`.

Keep target packages independent of private Radix source and concrete host
implementations. The Cargo package name `faber-runtime` is a public package
identity; it does not imply that the retired `faber-runtime` repository still
exists.

## Validation

Run the focused package command for the changed target:

```sh
cargo test --manifest-path runtime/rust/Cargo.toml
cargo test --manifest-path crates/http-transport/Cargo.toml
cargo test --manifest-path packages/http/rust/Cargo.toml
(cd runtime/go && go test ./...)
(cd runtime/swift && swift test)
(cd runtime/typescript && npx tsc --noEmit)
```

Do not restore a root Cargo workspace, compiler source, product release scripts,
or a forwarding runtime facade here.
