# Provenance and review

Reviewed 2026-10-30 for publication. The Faber package and its Latin source
files originated in this repository's `acceptance/fusion-containment/` project
and were moved here without duplicated copies. The Faber implementation is
project-authored; no third-party program source or external dataset is included.
The prescribed magnetic field and Boris integration are described in the
README. Its cited numerical paper is a reference only; no paper text or code is
reproduced.

`fixtures/magna-256.args` records the deterministic package arguments, and
`fixtures/magna-256.expected` is the reference-route stdout from the same Faber
source. The private Python oracle and route receipts remain under
`acceptance/fusion-containment/`; no oracle implementation or benchmark harness
is shipped here. Receipt paths and hashes are cited in the README.

The Latin package sources remain primary. `english/canonical.fab` is the
converter-generated English-reader rendering of `src/canonical.fab`, produced
with `faber convert --from la --to en --stdout`; it is outside `src/` and does
not replace or duplicate the package entry. The in-tree converter still refuses
`src/main.fab` with `SEM057.call_marker_on_read_only` at imported mutable output
arguments. That remaining tooling gap is documented in the README; no manual
translation is represented as converter output, and no Faber language change
was made as part of this publication.
