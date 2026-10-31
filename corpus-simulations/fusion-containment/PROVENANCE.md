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

The existing Latin source has not been rewritten. The requested English source
rendering is pending because the in-tree `faber convert --from la --to en`
command refuses the source with `SEM057.call_marker_on_read_only` at imported
mutable output arguments. No manual translation is represented as converter
output, and no language/compiler change was made as part of this publication.
