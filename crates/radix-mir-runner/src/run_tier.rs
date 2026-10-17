//! Run-tier verdict for the MIR runner: does the entry run to completion?
//!
//! TARGET: the `runner` column of the target coverage matrices.
//!
//! BOUNDARY: [`super::classify_runner_lowerability`] answers the structural
//! question (can the runner dispatch these MIR shapes) without executing. This
//! module answers the behavioural one by running the program in process, with
//! the execution bounded so the tier stays a fast classifier: the unbounded
//! program-execution path is reserved for `faber run`.

use crate::{
    BufferHost, Host, RUNNER_ISSUE_STEP_BUDGET_EXHAUSTED, RUNNER_ISSUE_WALL_CLOCK_TIMEOUT,
    RunnerError, Value, run_entry_with_limits,
};
use radix_lexer::Span;
use radix_mir::{CapabilityGap, Lowerability, MirDiagnosticKind, MirProvider, ValidatedMir};

use crate::capability::{SLUG_POLICY, SLUG_UNSUPPORTED};

const SLUG_RUNTIME_PANIC: &str = "runtime-panic";
/// The tier stopped the program itself (step budget), rather than the program
/// failing: its own shape so the histogram never reads it as a capability
/// verdict.
const SLUG_STEP_BUDGET: &str = "coverage-step-budget-exhausted";
/// The tier stopped the program itself (wall-clock deadline).
const SLUG_WALL_CLOCK: &str = "coverage-wall-clock-exceeded";

/// Step budget for one corpus program under the run tier.
///
/// One step is one MIR basic-block visit. The matrix runs every exemplum in
/// process, so the tier carries its own bound: a program that needs more than
/// this many steps is unbounded work (a loop with no exit), not an exemplum.
const RUN_TIER_STEP_BUDGET: u64 = 10_000_000;

/// Wall-clock deadline for one corpus program under the run tier. The
/// deadline stops input-dependent work that stays under the step budget but
/// takes real time; the runner checks it cooperatively every 1024 steps.
const RUN_TIER_WALL_CLOCK: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug)]
enum RunTierHostTrap {
    Exit(i32),
    Abort(String),
}

struct RunTierTrapHost<'a> {
    inner: &'a mut BufferHost,
}

impl Host for RunTierTrapHost<'_> {
    fn scribe(&mut self, kind: MirDiagnosticKind, text: &str) -> Result<(), RunnerError> {
        self.inner.scribe(kind, text)
    }

    fn read_line(&mut self) -> Result<Option<String>, RunnerError> {
        self.inner.read_line()
    }

    fn read_all(&mut self) -> Result<Option<String>, RunnerError> {
        self.inner.read_all()
    }

    fn abort(&mut self, reason: &str) -> ! {
        std::panic::resume_unwind(Box::new(RunTierHostTrap::Abort(reason.to_owned())));
    }

    fn provider(&mut self, provider: &MirProvider) -> Result<Value, RunnerError> {
        self.inner.provider(provider)
    }

    fn exit(&mut self, code: i32) -> ! {
        std::panic::resume_unwind(Box::new(RunTierHostTrap::Exit(code)));
    }

    fn sermo_route(
        &mut self,
        route: &str,
        opener: &Value,
        span: Span,
    ) -> Option<Result<Vec<Value>, Value>> {
        crate::serve_route(self, route, opener, span)
    }

    fn argumenta(&self) -> Result<&[String], RunnerError> {
        self.inner.argumenta()
    }

    fn env_get(&self, name: &str) -> Result<Option<String>, RunnerError> {
        self.inner.env_get(name)
    }

    fn env_set(&mut self, name: &str, value: &str) -> Result<(), RunnerError> {
        self.inner.env_set(name, value)
    }

    fn cwd(&self) -> Result<String, RunnerError> {
        self.inner.cwd()
    }

    fn set_cwd(&mut self, path: &str) -> Result<(), RunnerError> {
        self.inner.set_cwd(path)
    }

    fn pid(&self) -> Result<i64, RunnerError> {
        self.inner.pid()
    }
}

/// Classify whether the MIR runner can run `incipit` to completion.
///
/// Uses a trap-aware [`BufferHost`] wrapper so Faber-level `exit` and `abort`
/// are classified as script outcomes instead of opaque Rust panics. This is the
/// runner **run** tier, not stdout verification.
///
/// The execution is bounded ([`RUN_TIER_STEP_BUDGET`], [`RUN_TIER_WALL_CLOCK`]):
/// the tier is a fast in-process classifier, and the unbounded
/// program-execution path is reserved for `faber run`.
#[must_use]
pub fn classify_run_tier(validated: &ValidatedMir<'_>) -> Lowerability {
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut host = BufferHost::default();
        let mut trap_host = RunTierTrapHost { inner: &mut host };
        run_entry_with_limits(
            validated,
            &mut trap_host,
            Some(RUN_TIER_STEP_BUDGET),
            Some(std::time::Instant::now() + RUN_TIER_WALL_CLOCK),
        )
    }));

    match run {
        Ok(Ok(())) => Lowerability::Capable,
        Ok(Err(errors)) => classify_run_errors(&errors),
        Err(payload) => classify_panic_payload(payload.as_ref()),
    }
}

/// Classify the runner's own errors for the run tier.
///
/// A stop the classifier imposed on itself is reported under its own slug: the
/// tier says it stopped, not that the target cannot run the program. Every
/// other error keeps its runner issue as the shape.
fn classify_run_errors(errors: &[RunnerError]) -> Lowerability {
    for error in errors {
        let slug = match error.issue.as_str() {
            RUNNER_ISSUE_STEP_BUDGET_EXHAUSTED => Some(SLUG_STEP_BUDGET),
            RUNNER_ISSUE_WALL_CLOCK_TIMEOUT => Some(SLUG_WALL_CLOCK),
            _ => None,
        };
        if let Some(slug) = slug {
            return Lowerability::Rejected(vec![CapabilityGap::UnsupportedMirShape {
                shape: error.message.clone(),
                slug,
            }]);
        }
    }
    let shape = errors
        .first()
        .map_or_else(|| "runner_failed".to_owned(), |error| error.issue.clone());
    Lowerability::Rejected(vec![CapabilityGap::UnsupportedMirShape {
        shape,
        slug: SLUG_UNSUPPORTED,
    }])
}

/// Classify one caught panic payload from the runner.
///
/// [`RunTierHostTrap`] payloads are the harness's own script outcomes. Any
/// other payload is a genuine panic in the runner or the lowered program, and
/// is reported as [`CapabilityGap::RuntimePanic`] so a crash can never be read
/// as a target-capability verdict.
fn classify_panic_payload(payload: &(dyn std::any::Any + Send)) -> Lowerability {
    match payload.downcast_ref::<RunTierHostTrap>() {
        Some(RunTierHostTrap::Exit(0)) => Lowerability::Capable,
        Some(RunTierHostTrap::Exit(code)) => policy_reject(format!("runner exit: {code}")),
        Some(RunTierHostTrap::Abort(reason)) => policy_reject(format!("runner abort: {reason}")),
        None => Lowerability::Rejected(vec![CapabilityGap::RuntimePanic {
            shape: format!("runner panic: {}", panic_text(payload)),
            slug: SLUG_RUNTIME_PANIC,
        }]),
    }
}

/// Panic payload text. Rust panics carry `&str` or `String`; anything else is
/// named by kind so the row still says why the classification aborted.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_owned())
}

fn policy_reject(shape: impl Into<String>) -> Lowerability {
    Lowerability::Rejected(vec![CapabilityGap::TargetPolicyRejection {
        shape: shape.into(),
        slug: SLUG_POLICY,
    }])
}

#[cfg(test)]
#[path = "run_tier_test.rs"]
mod tests;
