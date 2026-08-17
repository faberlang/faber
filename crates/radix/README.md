# crates/radix

Thin library-only compiler façade over [`crates/radix-module`](../radix-module/)
and [`crates/radix-program`](../radix-program/).

Admitted entries live under `radix::module`, `radix::program`, and
`radix::services`. This package has no `[[bin]]`, no clap, and no process
exit. The developer binary `radix` and `tool` stay in `radix-module` until
RTR4.

`scripta/assemble-dev-kit` and `scripta/generate-release-manifest` read the
component version from this `Cargo.toml`.
