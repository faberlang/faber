# Contributing

Public contributions are welcome for target-language APIs, public packages,
documentation, examples, and reproducible language reports.

Compiler and `faber` CLI implementation source is private, so implementation
patches for those components cannot be accepted here. File a report with:

1. Faber version and platform.
2. The smallest source fixture that reproduces the behavior.
3. The exact command.
4. Expected and observed output.

Target-package changes must keep private Radix source and concrete host effects
out of the package graph. Add the smallest focused test that proves the change.
