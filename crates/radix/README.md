# crates/radix (RTR3a stub)

This directory is a **workspace-excluded version-source stub**.

The module compiler lives in [`crates/radix-module`](../radix-module/).
`scripta/assemble-dev-kit` and `scripta/generate-release-manifest` still
read the component version from `crates/radix/Cargo.toml`.

RTR3c replaces this stub with the thin library-only `radix` façade.
The developer binary `radix` and `tool` stay in `radix-module` until RTR4.
