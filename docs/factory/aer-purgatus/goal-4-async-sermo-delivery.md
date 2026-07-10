# Delivery: Honest Async `Sermo` And Host Boundary

**Status**: partial — async codegen, proof-pair route migration, runtime producer handoff, and private host removal delivered; public host selection still open
**Date**: 2026-07-09
**Campaign**: [`CAMPAIGN.md`](CAMPAIGN.md)
**Primary repos**: `faber-runtime`, `radix`, `faber`, `norma`
**Existing authority**: `radix/docs/factory/async-ad-lowering/goal.md` and its
ledger
**Factory checkpoint**: an `@ futura` Faber caller dispatches a standard
library `ad` route without blocking an executor thread; sync and async wrappers
use the same route, host contract, frame protocol, and collector policy.

## Interpreted Unit

Complete the async correction at the actual boundary instead of adding more
`async fn` wrappers around synchronous work. The delivery must reconcile the
existing async-ad-lowering goal, whose runtime receive primitives exist but
whose codegen/stdlib phases remain pending, with the live runtime and package
bridge.

The current defect spans four layers:

1. `Sermo` uses `Rc<RefCell<_>>`, so a host worker cannot safely produce frames;
2. polling an async receive can call `ensure_runtime_response_inner`, which
   performs sleep, filesystem, and process work synchronously;
3. generated Rust constructs the private macOS Radix host per conversation and
   Faber discovers dependencies by searching emitted source text;
4. Norma expresses “async” with alternate route names and non-`@ futura`
   wrappers, so generated callers still block.

This goal corrects all four together. A green wake-registration test without a
nonblocking producer path is not completion.

## 2026-07-09 Partial Delivery Note

Delivered slices:

- Radix `55aeed472` lowers `@ futura` / `incipiet` `sermo ↦ T` conversions to
  async runtime materializers while preserving sync helper emission.
- Norma `4cf0c5e` migrates the required proof pairs:
  `solum.lege` / `@ futura solum.leget<T>` share `solum:lege`, and
  `tempus.expectet` / `@ futura tempus.dormiet` share `tempus:dormiet`.
- Examples `76a482f` adds async corpus proofs for those two same-route pairs.
- Faber runtime `3ae9583` replaces `Rc<RefCell<_>>` conversation state with
  `Arc<Mutex<_>>` plus a blocking receive condition variable, moves built-in
  route work onto producer threads, and makes unsupported routes resolve to an
  error terminal instead of pending indefinitely.
- Faber runtime `092b67c` threads materialization target shape into route
  producers for `textus`, `lista<textus>`, `octeti`, and scalar materializers.
- Faber runtime `89f2708` routes built-in producers through the public
  `HostDispatch` / `ResponseSender` / `Cancellation` contract, exports those
  types from the runtime crate root, enforces one terminal frame per sender
  lease, and turns last-sender drop before terminal into a producer-dropped
  error terminal.
- Radix `5daeefb36` deletes generated `__faber_attach_sermo` calls and the
  private macOS host shim from Rust `ad` codegen.
- Faber `4979410` removes generated Cargo dependency/features for the private
  `radix/hosts/macos-arm64` host path and keeps the package E2E proof running
  through runtime route production.
- Faber `651f6cc` adds the manifest schema for explicit
  `[target.rust] host = "native"` and rejects host policy on non-Rust target
  tables.

Validation run:

- `timeout 300 cargo test -p radix async_sermo_materializer_uses_async_runtime_helper -- --format terse`
- `timeout 300 cargo test -p radix async_entry_sermo_materializer_uses_block_on_and_async_helper -- --format terse`
- `timeout 180 cargo run --manifest-path faber/Cargo.toml -- check examples/corpus/ad/async-solum-leget.fab`
- `timeout 180 cargo run --manifest-path faber/Cargo.toml -- check examples/corpus/ad/async-tempus-dormiet.fab`
- `timeout 180 cargo run --manifest-path faber/Cargo.toml -- check examples/corpus/ad/solum-lege-generic.fab`
- `timeout 180 cargo run --manifest-path faber/Cargo.toml -- emit -t rust examples/corpus/ad/async-solum-leget.fab`
- `timeout 180 cargo run --manifest-path faber/Cargo.toml -- emit -t rust examples/corpus/ad/async-tempus-dormiet.fab`
- `timeout 180 norma/scripta/check-source`
- `timeout 240 cargo test frame -- --format terse` in `faber-runtime`
- `timeout 240 cargo clippy --all-targets -- -D warnings` in `faber-runtime`
- `timeout 240 cargo test -- --format terse` in `faber-runtime`
- `timeout 240 cargo test frame -- --format terse` in `faber-runtime`
- `timeout 300 cargo test -p radix ad -- --format terse`
- `timeout 180 cargo test -p radix --test hygiene`
- `timeout 300 cargo test --lib generated_package_ad_avoids_private_host_bridge_dependency -- --format terse` in `faber`
- `timeout 240 cargo test --lib rust_target_manifest_accepts_native_host_policy -- --format terse` in `faber`
- `timeout 240 cargo test --lib non_rust_target_manifest_rejects_host_policy -- --format terse` in `faber`
- `timeout 180 cargo test --test hygiene` in `faber`

Remaining blockers before closeout:

- `faber-runtime/src/frame.rs` now uses the public `HostDispatch` /
  `ResponseSender` contract for built-in route production. Cancellation is
  observable by senders and suppresses content after cancellation, but full
  caller-drop cancellation, shutdown, bounded queues, and late-worker
  suppression remain part of the native-host/runtime closeout.
- `faber/src/package/cargo.rs` still detects Tokio/executor need from emitted
  Rust text. The private macOS host scan/path is gone and
  `[target.rust] host = "native"` parses/validates, but the final structured
  `RustRuntimePlan`, route-requirement enforcement, and native-host dependency
  selection are still open.
- The public native host adapter, bounded-worker queue policy, cancellation
  suppression of late worker frames, concurrent timer proof, and package host
  selection remain unimplemented.
- `timeout 240 cargo test --lib package -- --format terse` in `faber` currently
  fails two unrelated `norma:json` package tests because open Goal 2 residuals
  still expect `json.solve`/`json.pange` to round-trip through `valor` instead
  of the formal `json` document type.

## Normalized Spec

### Architectural invariant

After dispatch acceptance, the caller only observes a frame queue. Host work
never runs from `Future::poll`, frame materialization, or a queue lock; every
accepted request reaches exactly one terminal outcome or an explicit caller
cancellation.

### Responsibility boundary

| Layer | Owns | Must not own |
| --- | --- | --- |
| Norma source | route vocabulary, sync/`@ futura` API pairs, typed opener and result | executor, threads, host crate selection |
| Radix | `ad` HIR/codegen, nearest-callable async posture, materializer selection, route requirements | concrete native host construction, package dependency guessing |
| `faber-runtime` | thread-safe `Sermo`, frame protocol, dispatch/producer traits, sync/async receive adapters, shared collectors, builtin runtime routes | OS-specific filesystem/process implementation, Tokio |
| public native host adapter | nonblocking implementation of native standard routes and bounded blocking workers | Faber syntax/codegen policy |
| Faber package builder | structured runtime plan, explicit host selection, generated dependencies/features, E2E build/run | sniffing emitted Rust strings, private Radix paths |

### Runtime state model

Replace the conversation's `Rc<RefCell<SermoInner>>` with a thread-safe shared
core, normally `Arc<Mutex<SermoState>>` plus a `Condvar`. `Meus`, `Tuus`,
`Sermo`, and a host response sender share that core. Keep the mutex hold small:
copy/move queue state under lock, then perform conversion, wake, I/O, or host
dispatch after releasing it.

The state has explicit dimensions rather than overlapping booleans:

```text
dispatch: New | Starting | Accepted | Rejected
outbound: Open | Closed | Cancelled
inbound:  Open | Terminal(done | error | cancel)
queue:    FIFO<Scrinium>
waiters:  registered task wakers
consumer: Idle | Blocking | Async
```

The existing `runtime_response_generated`, `incoming_drained`, `detached`, and
terminal fields may be replaced; do not preserve contradictory combinations
for compatibility.

Required transitions:

| Event | Precondition | State/effect |
| --- | --- | --- |
| open | none | create `New/Open/Open`, enqueue/capture opener request |
| dispatch | `New` and outbound request closed/ready | enter `Starting`; invoke selected dispatcher once outside lock with an unarmed producer lease |
| accept | dispatcher returned acceptance with producer | atomically arm the lease and enter `Accepted`; producers own terminal obligation |
| reject/no route | dispatcher returned structured error | `Rejected`; runtime enqueues one error terminal immediately |
| push item | `Accepted` + inbound open | enqueue correlated item; wake async and blocking receivers |
| finish | `Accepted` + inbound open | enqueue one terminal, mark terminal, wake all |
| cancel | caller live + inbound open | mark cancellation, notify host token, enqueue/record cancel terminal, wake all |
| receive | matching consumer lease + queue nonempty | pop FIFO; never dispatch or run host work |
| receiver drop | materialization unfinished | execute cancellation policy and unregister that waiter |

### Temporal invariants

1. Dispatch occurs at most once per conversation.
2. `Future::poll` is bounded to lock/check/register/return and never sleeps,
   accesses a filesystem, starts a process, or performs network I/O.
3. A host cannot enqueue after a terminal frame.
4. An accepted dispatcher transfers a terminal-completion obligation to one or
   more response producers.
5. Dropping the last live producer before a terminal frame enqueues a
   structured `producer_dropped` error unless the caller already cancelled.
6. Unknown or unsupported routes reject immediately; they never leave a
   receiver pending forever.
7. Cancellation is monotonic and wakes every registered async/blocking waiter.
8. Every frame keeps the conversation id/call correlation created at open.
9. No user conversion or waker callback executes while the state lock is held.
10. Sync and async materialization feed the same collector state machine and
    therefore accept/reject identical frame sequences.
11. The inbound direction is single-consumer. A competing raw receiver or
    materializer fails with `receiver_busy`; frames are never split
    nondeterministically between collectors.

Tests should model transitions directly. A state such as `Accepted + no live
producer + inbound Open` is forbidden and must become an error terminal.

### Host dispatch contract

Define a runtime-owned portable trait (names may follow local convention):

```rust
pub trait HostDispatch: Send + Sync {
    fn start(
        &self,
        request: SermoRequest,
        responses: ResponseSender,
        cancellation: Cancellation,
    ) -> Result<(), DispatchError>;
}
```

Contract details:

- `start` is a nonblocking handoff. It may do bounded validation and queue work,
  but must return without waiting for I/O, a timer, or a child process.
- `Ok(())` transfers the terminal obligation to `ResponseSender` clones.
- `Err` means no producer owns the request; the runtime converts it immediately
  to a correlated error terminal.
- `ResponseSender` exposes item/done/error operations that enforce one terminal
  and reject post-terminal sends.
- the last sender's drop guard enforces the producer-dropped invariant;
- cancellation is observable by queued/running host work and is idempotent;
- a dispatcher is installed once in generated application state and shared by
  conversations, not constructed inside every `ad` expression.

Dispatch handoff has an explicit early-response rule. The runtime enters
`Starting` and supplies an **unarmed** shared producer lease before calling
`start` outside the conversation lock. Send operations during `Starting` may be
buffered, because a queued worker can reply before `start` returns. On
`Ok(())`, the runtime atomically arms the lease, enters `Accepted`, publishes
buffered frames, and applies a recorded last-sender drop as
`producer_dropped`. On `Err`, it disarms every clone and enters `Rejected`; any
attempted response before that error is a host protocol violation and the
rejection terminal reports it. This prevents both a sender-drop terminal from
racing ahead of a normal rejection and an early valid response from being
lost.

Builtin runtime-only routes such as `runtime:echo` use the same dispatch
contract through a small runtime dispatcher. Remove
`ensure_runtime_response_inner`; there is no receive-time fallback switch.

### Portable native host decision

Create a small public native application host crate under the public runtime
repository, preferably:

```text
faber-runtime/hosts/native/   package = "faber-host-native"
```

It implements `HostDispatch` for the native Norma routes required by the
application emitter. It is deliberately separate from the private
`radix/hosts/macos-arm64` Wasm host proof and has no Wasmtime dependency.

Blocking OS APIs run on a bounded worker facility owned by this host:

- timers sleep on a worker, not an executor thread;
- filesystem reads/writes and stdin operations run on workers;
- process creation/wait/stdout capture runs on workers;
- fast stdout/stderr writes may still use the same handoff for one consistent
  ordering/cancellation contract.

The initial implementation may use a fixed-size standard-thread pool; the
runtime protocol must not depend on Tokio. Queue saturation returns a
structured dispatch error rather than growing unbounded threads or blocking
the caller indefinitely. Pool shutdown either drains or terminally fails all
accepted work.

The private macOS host may adapt to the same runtime trait later, but generated
public Faber packages must never depend on its private path.

### Explicit package host selection

Extend `[target.rust]` with an explicit host policy. Canonical delivery spelling:

```toml
[target.rust]
host = "native"
```

`host = "native"` adds the public `faber-host-native` dependency and initializes
one dispatcher. An absent host means runtime-only routes; if structured route
analysis finds a non-runtime `ad` requirement, package validation fails with a
diagnostic asking for a host selection. Unknown host values fail manifest
deserialization/validation. Do not silently default to the developer's OS.

Faber builds a structured `RustRuntimePlan` (exact name local) from:

- analyzed HIR route requirements;
- whether an async entry/function requires an executor;
- the manifest's host choice;
- declared target dependencies.

`generate_cargo_toml` consumes that plan. Delete
`generated_code_needs_host_bridge`, `generated_code_needs_tokio`, the default
`__faber_host_macos_arm64` feature, and the private Radix path lookup. Emitted
source strings are output, never dependency-analysis input.

### Radix async lowering

Thread callable posture through Rust expression emission explicitly. Extend
`ExprEmitPolicy` or its focused successor with whether suspension is legal at
the current conversion site. The posture comes from the nearest callable:

- `HirFunction.is_async` for function bodies;
- `HirProgram.entry_is_async` for entry statements;
- a nested callable resets posture to its own annotation rather than blindly
  inheriting the lexical outer function.

The `ad` expression still creates and dispatches a `Sermo` synchronously. The
conversion site selects the receiver:

| Source context | `sermo ↦ T` lowering |
| --- | --- |
| ordinary callable | shared collector driven by blocking receive |
| `@ futura` callable | same collector driven by async receive and `.await` |
| async entry | same async path |
| suspension where posture is unknown/forbidden | compile diagnostic, never hidden `block_on` |

Failable conversion selects the failable materializer in both postures. A
`Sermo` stored and converted later uses the posture at the conversion site, not
the posture where the `ad` handle was constructed.

Remove generated `__faber_attach_sermo` and concrete `HostKernel` references.
Generated code dispatches through runtime application context/dispatcher APIs
whose types are public and target-neutral.

### One collector policy

Replace the copy-paired sync/async materializer bodies with a protocol-neutral
collector per result family. A collector consumes one `Scrinium` at a time and
returns `Continue`, `Complete(T)`, or `Fail(FrameError)`. Two thin drivers own
waiting only:

```text
blocking driver: recv_blocking -> collector.push
async driver:    recv_async.await -> collector.push
```

Required collectors preserve today's intended policies for `vacuum`,
`textus`, `octeti`, `valor`, `lista<T>`, scalar `T`, and `instans`. Any current
sync/async discrepancy is resolved in the collector tests, not duplicated in
both drivers. Genus/tabula materialization deferred in Norma comments must use
the same extension seam rather than another special receive loop.

The async driver owns a cancellation-on-drop guard until a terminal result is
observed. Waker registration must replace an existing equivalent waker instead
of growing without bound; completed/cancelled receivers unregister promptly.

### Norma route and API contract

Sync and async API pairs name **the same `ad` route**. Tense or public function
name may express posture; route spelling expresses capability only.

Canonical shape:

```fab
functio lege(...) → textus {
    redde ad "solum:lege" { ... } ↦ textus
}

@ futura
functio leget(...) → textus {
    redde ad "solum:lege" { ... } ↦ textus
}
```

Do not preserve alternate `*:...t` host routes as compatibility aliases. The
host sees one capability and may schedule it without knowing which Faber
wrapper called it.

Migration scope is all homogeneous built-in sync/async pairs in active Norma
host modules that the public native host supports, initially at least
`tempus`, `solum`, `consolum`, and `processus`. Inventory every distinct
sync/async route pair before edits. Stateful/network/database surfaces whose
cancellation or lifetime contract is not yet representable receive an explicit
campaign follow-up rather than a fake `@ futura` wrapper.

### Error, cancellation, and shutdown contract

- dispatch rejection, unsupported route, queue saturation, producer drop,
  host error, protocol error, and caller cancellation use distinct stable issue
  codes;
- a failable Faber conversion receives these as its alternate exit;
- a non-failable conversion may panic/terminate according to the existing
  generated-runtime policy, but it cannot hang;
- dropping an async materialization future requests cancellation and does not
  wait synchronously for worker completion;
- a worker that cannot interrupt an OS call suppresses later result frames once
  cancellation wins;
- application/host shutdown wakes all conversations and resolves their
  terminal obligation.

### Non-goals

- No promise that all operating-system calls are intrinsically asynchronous;
  the promise is that executor threads and `Future::poll` do not block on them.
- No adoption of Tokio inside `faber-runtime`.
- No preservation of async-suffixed route aliases.
- No full remote/distributed gateway protocol redesign.
- No automatic host selection from build-machine architecture.
- No completion claim for stateful network/database routes not included in the
  reconciled route inventory.

## Repo-Aware Baseline

### Facts

- The async-ad-lowering ledger records Phase 1a and the runtime async receive
  core as delivered; Rust codegen and Norma pair migration remain pending.
- `Sermo`, `Meus`, and `Tuus` share `Rc<RefCell<SermoInner>>`.
- `sermo_recv_ready` calls `ensure_runtime_response_inner` when the queue is
  empty. That switch performs `thread::sleep`, filesystem work, and process
  work synchronously.
- an unrecognized route can leave async receive pending without a terminal
  outcome.
- sync and async materializers contain near-copy receive/collection loops.
- generated Rust emits `__faber_attach_sermo`, constructs
  `faber_host_macos_arm64::HostKernel`, and attaches it per conversation.
- Faber's Cargo generator searches emitted text to decide whether to add the
  host and Tokio, and enables a private Radix host path by default.
- `ManifestTarget` currently has bindings and dependencies but no host policy.
- HIR already preserves `HirFunction.is_async` and
  `HirProgram.entry_is_async`; the missing work is posture propagation into
  expression/conversion emission.
- active Norma modules explicitly document that “async” route variants still
  block and use distinct route names without `@ futura`.

### Architectural correction

Most underlying pieces exist: async annotations reach HIR, the runtime has
waker-aware receive, frame materializers define result policies, and hosts
already understand routes. The correction redirects them around one explicit
dispatch seam:

```text
Norma wrapper
   │ same route
   v
Radix-generated dispatch ─> HostDispatch.start ─> worker/event source
   │                                                 │
   └─ sync/async collector driver <─ frame queue <────┘
```

## Authority Reconciliation

Before implementation, update the existing async-ad-lowering goal and ledger
in `radix` so they point to this delivery as the expanded integration plan.
Preserve its completed evidence; do not reset delivered phases. Replace any
remaining assumption that waker-aware receive alone proves async behavior.

Suggested ledger mapping:

| Existing phase | Campaign delivery |
| --- | --- |
| Phase 0 contract | reconciled by A0, retain completed evidence |
| Phase 1a posture propagation | retain HIR work; finish in A3 codegen context |
| Phase 2 runtime core | reopen only for thread-safe dispatch/cancellation correction in A1–A2 |
| Phase 3 codegen | A3 |
| Phase 4 stdlib pairs | A5 |
| Phase 5 docs/closeout | A6 |

## Stage Graph

```text
A0 authority + route inventory + protocol tests
                    │
          ┌─────────┴─────────┐
          v                   v
A1 thread-safe Sermo     A3 codegen posture/runtime plan design
          │                   │
          v                   │
A2 dispatch + collectors     │
          │                   │
          ├─────────┬─────────┘
          v         v
A4 native host   Faber structured package integration
          └─────────┬─────────┘
                    v
A5 Norma same-route @futura migration
                    │
                    v
A6 ecosystem E2E, cancellation, docs, release closeout
```

| Stage | Entry condition | Work and output | Exit gate |
| --- | --- | --- | --- |
| A0 — Reconcile | campaign selects Goal 4 | Update authority ledger; inventory all Norma route pairs and host implementations; add executable state/protocol tests and exact native-host scope. | One authoritative phase map; no unowned duplicate route pair. |
| A1 — Conversation core | A0 | Move to thread-safe state/condvar/wakers; encode transitions, correlation, terminals, cancellation, sender-drop behavior. | model/unit tests prove temporal invariants under races and no forbidden state persists. |
| A2 — Dispatch/collect | A1 | Add runtime dispatch traits, builtin dispatcher, response sender, blocking/async drivers, shared collectors; delete receive-time execution. | poll-purity and sync/async sequence-equivalence tests pass; unknown route terminates. |
| A3 — Compiler/package plan | A0 contract locked | Propagate nearest-callable posture, emit async materialization, collect structured route/executor requirements, parse explicit host manifest policy. | focused Rust snapshots have `.await` only in async contexts; generated Cargo comes from a plan, not text scans. |
| A4 — Native host | A2 + A3 interface | Add public host crate and bounded workers for selected routes; initialize one shared host in generated applications; delete private host dependency. | timer/fs/process overlap proof shows executor responsiveness; generated package contains no Radix path. |
| A5 — Norma migration | A3 + A4 | Convert inventoried homogeneous pairs to same route + `@ futura`; delete async route aliases/comments that describe fake async. | source audit proves paired routes identical and async functions annotated; sync and async E2E results match. |
| A6 — Closeout | A5 | cancellation/shutdown/load tests, package E2E, docs/exempla, ledger closure, release preparation. | all selected host modules meet protocol; no receive-time I/O, string-sniff dependency, or private host reference remains. |

A1 and the design/test portion of A3 may run in parallel in separate repos after
A0. A4 cannot implement against a provisional producer contract. A5 begins
only after one generated package proves the full runtime/host path.

## Implementation Work

### Workstream A — Runtime protocol (`faber-runtime`)

Primary surfaces:

- `src/frame.rs`, split into focused state/dispatch/collector modules if needed;
- frame unit/live tests;
- public exports needed by generated code and host adapters.

Acceptance:

- no `Rc<RefCell<_>>` remains in the cross-thread conversation path;
- no `ensure_runtime_response_inner` or equivalent receive-time host switch;
- locks are never held across host calls, conversions, waits, or wakes;
- blocking waits use a condition variable/event, not polling sleeps;
- async poll registers/replaces a waker and returns quickly;
- sender/drop/cancellation races are deterministic under repeated tests;
- all materializer families share collector policies.

### Workstream B — Radix codegen and route analysis (`radix`)

Primary surfaces:

- Rust expression emission policy/context and conversio materializers;
- `codegen/frame_shim.rs` deletion/replacement;
- route-requirement query over HIR;
- async-ad goal/ledger and focused Rust snapshot tests.

Acceptance:

- async posture follows the nearest callable/entry;
- no hidden `block_on` appears inside `@ futura` code;
- sync functions retain blocking semantics without alternate routes;
- failable/non-failable conversions select matching async helpers;
- generated source contains no concrete host crate/type;
- route inventory is structured data available to Faber packaging.

### Workstream C — Package integration (`faber`)

Primary surfaces:

- `src/package/manifest.rs` host policy;
- `src/package/cargo.rs` structured runtime plan;
- package build/E2E fixtures and docs.

Acceptance:

- `[target.rust].host` is validated and documented;
- non-runtime routes without a selected host fail before Cargo emission;
- only selected dependencies are written;
- source-text dependency sniffers and private host paths are deleted;
- a runtime-only package does not depend on the native host;
- an async-only package receives the executor dependency from HIR posture.

### Workstream D — Public native host (`faber-runtime/hosts/native`)

Implement the dispatch contract for the A0-selected Norma route set with a
bounded queue/pool and response producers.

Acceptance:

- route handlers return from `start` before blocking work;
- queue saturation, shutdown, and unsupported route are terminal errors;
- cancellation suppresses late frames and releases queued work where possible;
- panicking worker/handler becomes a terminal host error rather than a lost
  sender/hang;
- timer, filesystem, and process tests use temp resources and explicit
  timeouts;
- no dependency on the private Radix workspace.

### Workstream E — Norma API correction (`norma`)

Inventory `tempus`, `solum`, `consolum`, `processus`, and other live host
modules. For supported homogeneous pairs, add/verify `@ futura`, use the same
route literal, and update docs/tests. Delete obsolete host aliases after all
callers change.

Acceptance:

- route-pair audit has no selected suffix-only distinction;
- sync and async variants preserve parameter/result/error types;
- sync wrapper blocks only in its blocking driver; async wrapper yields;
- unsupported/stateful pairs are explicitly routed as follow-up work, not
  silently counted complete.

## Checkpoints And Gates

### Batching / split decision

Use contract-preserving repository checkpoints:

1. authority/protocol tests;
2. runtime state + dispatch + collectors;
3. compiler posture + structured package plan;
4. native host + one vertical route proof;
5. Norma route batches and ecosystem closeout.

Runtime public traits and host adapter may land in separate commits only when
the runtime tests include a fake asynchronous producer. Never land generated
code that references an unpublished/unavailable private dependency.

### Mandatory behavioral gates

| Gate | Proof |
| --- | --- |
| Poll purity | a pending receive poll performs no handler/I/O work and completes within a tight bounded test |
| Responsiveness | while one timer/fs/process route is blocked on a worker, an independent async task makes progress on the executor |
| Wake correctness | item, terminal, reject, cancel, sender drop, and shutdown each wake a pending receiver |
| Terminal totality | unsupported route, saturated queue, worker panic, and dropped producer all resolve with one terminal |
| Cancellation | dropping an async materializer requests cancel, produces no late visible result, and releases waiter state |
| Policy parity | identical frame sequences yield identical sync/async values or errors for every collector family |
| Consumer ownership | a second receiver fails deterministically and cannot steal a frame from the active collector |
| Route identity | selected Norma sync/async pairs contain the same route literal |
| Package honesty | runtime plan, manifest selection, and Cargo dependencies agree without emitted-text scans |
| Portability | generated public package has no path/reference to private `radix/hosts` |

### Concurrency test posture

Use deterministic barriers/channels and bounded timeouts, not unbounded sleeps.
Repeat race-sensitive unit tests where the test harness supports it. At least
cover:

- terminal versus cancellation;
- final producer drop versus terminal send;
- early response/final producer drop while dispatch is still `Starting`;
- waker replacement versus item arrival;
- competing inbound consumer acquisition;
- blocking receiver and async receiver on independent conversations;
- shutdown with queued and active work;
- late worker completion after cancellation.

### Release decision

`release-prep`. The runtime frame API, generated package dependency model,
manifest schema, Norma route vocabulary, and async behavior are public
contracts. Prepare coordinated versions/release notes for `faber-runtime`,
Faber CLI, Norma, and the compiler/toolchain. A version/tag is not the first
factory checkpoint; release only after the vertical and ecosystem gates close.

## Validation

Every test command uses an explicit timeout. Exact filters may be refined in A0
without weakening the lanes.

Runtime and native host:

```bash
cd ../faber-runtime
timeout 240 cargo test frame -- --format terse
timeout 240 cargo test async -- --format terse
timeout 240 cargo test --manifest-path hosts/native/Cargo.toml -- --format terse
timeout 240 cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Compiler:

```bash
cd ../radix
timeout 300 cargo test -p radix ad -- --format terse
timeout 300 cargo test -p radix async -- --format terse
timeout 180 cargo test -p radix --test hygiene
timeout 300 ./scripta/test
cargo fmt --all -- --check
git diff --check
```

Package integration:

```bash
cd ../faber
timeout 240 cargo test --lib package
timeout 300 cargo test --test package_e2e
timeout 180 cargo test --test hygiene
cargo fmt --all -- --check
git diff --check
```

Norma and vertical applications:

```bash
cd ../norma
timeout 180 ./scripta/check-source
git diff --check

cd ../faber
timeout 300 cargo run -- run ../examples/fixtures/async-ad-native
```

The factory creates `examples/fixtures/async-ad-native` in A4 and records this
invocation in both the async ledger and campaign evidence.

## Companion Skill Plan

- `correctness`: state transitions, terminal obligation, cancellation/drop
  races, lock/waker discipline, and protocol errors.
- `red-green`: state-model, poll-purity, cancellation, and responsiveness tests
  before replacing the runtime fallback.
- `consequences`: audit every active Norma route and generated-package consumer
  before deleting aliases/private host wiring.
- `optimization`: read-only review of locks, queue bounds, allocations, worker
  saturation, and executor interaction before closeout.
- `cleanliness`: split state, dispatch, receive drivers, and collectors; prevent
  `frame.rs` and codegen policy from becoming new monoliths.
- `polish`: per-file pass across runtime, compiler, package, host, and Norma
  primary surfaces before release preparation.

## Open Questions

No blocking architecture question remains. Factory A0 must answer from the live
route inventory:

1. the exact initial native-host route list and which stateful surfaces need a
   follow-up;
2. the bounded worker count/queue defaults and whether manifest tuning is
   required now or deferred;
3. whether the committed `examples/fixtures/async-ad-native` proof needs more
   than the timer/filesystem/process routes required by A4.

Stop and return to campaign routing if:

- a proposed runtime design still runs host work from receive/poll;
- host production cannot be thread-safe without weakening the frame protocol;
- package selection would require a private Radix path or implicit OS default;
- a supposedly async Norma wrapper contains hidden blocking or `block_on`;
- a route cannot define cancellation/terminal ownership;
- a compatibility alias is proposed for obsolete async route names.
