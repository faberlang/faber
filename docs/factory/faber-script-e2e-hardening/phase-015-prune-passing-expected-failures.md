# Phase 015 - Prune Passing Expected Failures

## Target

The script e2e harness keeps a structured `SCRIPT_EXPECTED_FAILURES` list. During
recent implementation phases, several entries moved from expected failure to
passing script execution but remained in the list because the harness still stops
first on unrelated unexpected failures.

Known passing entries to prune:

- `assignatio/assignatio.fab`
- `ego/ego.fab`
- `in/in.fab`
- `redde/redde.fab`
- `sub/sub.fab`
- `tabula/methodi-accessus.fab`

## Invariant

A path that now passes script execution must not remain classified as an expected
failure. The structured expected-failure list is a ratchet, not a historical
ledger.

## Scope

- Remove the known passing paths from `SCRIPT_EXPECTED_FAILURES`.
- Run the focused expected-failure taxonomy test.
- Run the script e2e harness to confirm pass/run/output counts are unchanged and
  the expected-failure bucket count shrinks.
- Update the factory ledger.

## Out of Scope

- Reclassifying still-failing paths.
- Changing run floors.
- Fixing additional MIR or stepper behavior.

## Validation

- `timeout 120 cargo test -p exempla script_expected_failure -- --nocapture`
- `timeout 300 cargo test -p exempla exempla_script_e2e -- --ignored
  --nocapture`
