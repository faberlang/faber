# Phase 018 - Classify Norma And Reference Failures

## Target

Several remaining unclassified script e2e failures are not script-stepper
implementation obligations:

- Fixtures importing `norma:*`, which the factory goal explicitly excludes from
  script-mode implementation.
- `intervallum` reference files with top-level declarations and no runnable
  `incipit`.
- An explicitly negative sized-family type fixture.

The goal document already names a `norma-import` bucket, but the harness taxonomy
does not yet include it.

## Invariant

Expected-failure classifications must describe why a fixture is outside the
current script-mode obligation. They must not hide runnable MIR debt.

## Scope

- Add the `norma-import` bucket to the script e2e taxonomy.
- Classify only inspected `norma:*` import fixtures under `norma-import`.
- Classify inspected `intervallum` reference files under `no-entry-reference`.
- Classify `typi/sized-family-error.fab` under `frontend-negative`.
- Run taxonomy and script e2e validation.

## Out of Scope

- Making `norma:*` imports work in script mode.
- Classifying tensor, JSON valor, aggregate, optional-chain, or other real MIR
  debt.
- Changing run or output floors.

## Validation

- `timeout 120 cargo test -p exempla script_expected_failure -- --nocapture`
- `timeout 300 cargo test -p exempla exempla_script_e2e -- --ignored
  --nocapture`
