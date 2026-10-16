# Compiler Source Presentation

Use this reference when creating, reviewing or polishing source comments in the
compiler crates.

Comments are an editorial layer over executable truth. They give a reader
orientation without asking prose to reconstruct execution. Production comments
have three jobs:

1. **Charter**: state the file's stable purpose and boundary (the `//!` header).
2. **Roadsign**: mark conceptual regions in a long file.
3. **Rationale**: explain a non-obvious constraint or design decision.

Source files carry an idiomatic charter. A parser helper that implements a
subtle production names it in a `GRAMMAR:` note, the convention used across
`radix-parser` and its siblings. A generated file's charter names its source of
truth.

## Workflow

For each scoped file:

1. Read the file, its direct producers and consumers, and its focused tests.
2. Identify the stable responsibility before editing prose.
3. Settle correctness and structure first; comments describe the code as it
   stands and do not defend a broken boundary.
4. Add or refine only the charter, useful roadsigns and grounded rationale.
5. Remove narration, field paraphrase, volatile inventories, movable pass
   numbers and unsupported history.
6. Review neighboring charters when ownership across files changed.
7. Check that rustdoc links still resolve.

Ordinary local work reviews only the touched charter and nearby rationale. Do
not start a whole-codebase documentation pass unless that is the assignment.

## Evidence rule

Claims may rest on declarations, types, callers, validators, assertions, tests
and repository history. A claim's strength must not exceed the evidence read.

- Mechanical evidence supports structure and obvious behavior.
- Direct callers and tests support a local contract.
- Architectural ownership needs neighboring modules.
- Rejected alternatives and history need an explicit record.

No comment is better than invented rationale. If a file's responsibility or
backstory is unclear, preserve the source and say so.

## Completion checks

- The charter states stable ownership, not current contents.
- Roadsigns exist where a long file needs them.
- Inline comments explain constraints or decisions, not the next line.
- No prose duplicates a changing set of targets, passes, operations or callers.
- Neighboring files do not claim the same responsibility in conflicting terms.
