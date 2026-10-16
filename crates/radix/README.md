# crates/radix

Thin library-only compiler façade over [`crates/radix-module`](../radix-module/)
and [`crates/radix-program`](../radix-program/).

Admitted entries live under `radix::module`, `radix::program`, and
`radix::services`. This package has no `[[bin]]`, no clap, and no process
exit. The `faber` command-line tool lives in [`crates/faber`](../faber/).
